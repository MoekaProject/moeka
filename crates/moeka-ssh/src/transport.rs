use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::process::ChildStderr;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender, TrySendError},
};
use std::thread;
use std::time::{Duration, Instant};

use moeka_fs::protocol::{read_raw_frame, write_frame};
use serde::{Serialize, de::DeserializeOwned};

use crate::{BootstrapProcess, MAX_BOOTSTRAP_OUTPUT, SshConnection};

static CONNECTION_SLOTS: AtomicUsize = AtomicUsize::new(0);

struct ConnectionPermit;

impl ConnectionPermit {
    fn acquire() -> io::Result<Arc<Self>> {
        CONNECTION_SLOTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < 8).then_some(active + 1)
            })
            .map(|_| Arc::new(Self))
            .map_err(|_| {
                io::Error::new(io::ErrorKind::WouldBlock, "SSH connection capacity is busy")
            })
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        CONNECTION_SLOTS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Outgoing {
    bytes: Vec<u8>,
    completion: SyncSender<io::Result<()>>,
}

#[derive(Default)]
struct DiagnosticTail {
    bytes: VecDeque<u8>,
    error: Option<String>,
}

/// Blocking adapter for the application's bounded filesystem executor.
/// A timeout invalidates the connection; requests are never replayed.
pub struct FramedTransport {
    process: Option<BootstrapProcess>,
    outgoing: Option<SyncSender<Outgoing>>,
    incoming: Option<Receiver<io::Result<Option<Vec<u8>>>>>,
    diagnostics: Arc<Mutex<DiagnosticTail>>,
    permit: Option<Arc<ConnectionPermit>>,
}

impl FramedTransport {
    pub fn new(connection: SshConnection) -> io::Result<Self> {
        let permit = ConnectionPermit::acquire()?;
        let SshConnection {
            process,
            mut input,
            mut output,
            diagnostics,
        } = connection;
        let (outgoing, writer) = mpsc::sync_channel::<Outgoing>(1);
        let (reader, incoming) = mpsc::sync_channel(2);
        let writer_permit = permit.clone();
        thread::Builder::new()
            .name("moeka-ssh-write".into())
            .spawn(move || {
                let _permit = writer_permit;
                while let Ok(message) = writer.recv() {
                    let result = input.write_all(&message.bytes).and_then(|_| input.flush());
                    let failed = result.is_err();
                    let _ = message.completion.send(result);
                    if failed {
                        break;
                    }
                }
            })?;
        let reader_permit = permit.clone();
        thread::Builder::new()
            .name("moeka-ssh-read".into())
            .spawn(move || {
                let _permit = reader_permit;
                loop {
                    let frame = read_raw_frame(&mut output);
                    let finished = !matches!(&frame, Ok(Some(_)));
                    if reader.send(frame).is_err() || finished {
                        break;
                    }
                }
            })?;
        let diagnostics = drain_diagnostics(diagnostics, permit.clone())?;
        Ok(Self {
            process: Some(process),
            outgoing: Some(outgoing),
            incoming: Some(incoming),
            diagnostics,
            permit: Some(permit),
        })
    }

    pub fn send<T: Serialize>(&mut self, value: &T, timeout: Duration) -> io::Result<()> {
        self.ensure_connected()?;
        if timeout.is_zero() {
            return Err(self.fail(
                io::ErrorKind::TimedOut,
                "SSH request expired before writing",
            ));
        }
        let deadline = Instant::now() + timeout;
        let mut bytes = Vec::new();
        write_frame(&mut bytes, value)?;
        let (completion, result) = mpsc::sync_channel(1);
        let outgoing = self.outgoing.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "SSH write queue is closed")
        })?;
        match outgoing.try_send(Outgoing { bytes, completion }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "SSH write queue is full",
                ));
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(self.fail(io::ErrorKind::BrokenPipe, "SSH writer disconnected"));
            }
        }
        match result.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(self.fail(error.kind(), &error.to_string())),
            Err(_) => Err(self.fail(
                io::ErrorKind::TimedOut,
                "SSH write was not acknowledged; the remote operation's outcome is unknown",
            )),
        }
    }

    pub fn receive<T: DeserializeOwned>(&mut self, timeout: Duration) -> io::Result<T> {
        self.ensure_connected()?;
        let incoming = self.incoming.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "SSH read queue is closed")
        })?;
        match incoming.recv_timeout(timeout) {
            Ok(Ok(Some(bytes))) => serde_json::from_slice(&bytes)
                .map_err(|error| self.fail(io::ErrorKind::InvalidData, &error.to_string())),
            Ok(Ok(None)) => {
                Err(self.fail(io::ErrorKind::UnexpectedEof, "SSH server closed stdout"))
            }
            Ok(Err(error)) => Err(self.fail(error.kind(), &error.to_string())),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(self.fail(io::ErrorKind::BrokenPipe, "SSH reader disconnected"))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(self.fail(
                io::ErrorKind::TimedOut,
                "SSH response timed out; requests will not be replayed",
            )),
        }
    }

    pub fn diagnostic_tail(&self) -> io::Result<String> {
        let diagnostics = self
            .diagnostics
            .lock()
            .map_err(|_| io::Error::other("SSH diagnostics lock was poisoned"))?;
        let bytes: Vec<u8> = diagnostics.bytes.iter().copied().collect();
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if let Some(error) = &diagnostics.error {
            text.push('\n');
            text.push_str(error);
        }
        Ok(text)
    }

    pub fn close(&mut self) {
        self.outgoing.take();
        self.process.take();
        self.incoming.take();
        self.permit.take();
    }

    fn ensure_connected(&self) -> io::Result<()> {
        if self.process.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "SSH connection is closed",
            ));
        }
        Ok(())
    }

    fn fail(&mut self, kind: io::ErrorKind, message: &str) -> io::Error {
        self.close();
        match self.diagnostic_tail() {
            Ok(tail) if !tail.is_empty() => io::Error::new(kind, format!("{message}: {tail}")),
            Ok(_) => io::Error::new(kind, message.to_owned()),
            Err(error) => io::Error::new(kind, format!("{message}: {error}")),
        }
    }
}

fn drain_diagnostics(
    mut reader: ChildStderr,
    permit: Arc<ConnectionPermit>,
) -> io::Result<Arc<Mutex<DiagnosticTail>>> {
    let tail = Arc::new(Mutex::new(DiagnosticTail::default()));
    let captured = tail.clone();
    thread::Builder::new()
        .name("moeka-ssh-stderr".into())
        .spawn(move || {
            let _permit = permit;
            let mut buffer = [0; 4096];
            loop {
                let result = reader.read(&mut buffer);
                if matches!(&result, Err(error) if error.kind() == io::ErrorKind::Interrupted) {
                    continue;
                }
                let Ok(mut tail) = captured.lock() else {
                    break;
                };
                match result {
                    Ok(0) => break,
                    Ok(count) => {
                        let excess =
                            (tail.bytes.len() + count).saturating_sub(MAX_BOOTSTRAP_OUTPUT);
                        tail.bytes.drain(..excess);
                        tail.bytes.extend(&buffer[..count]);
                    }
                    Err(error) => {
                        tail.error = Some(error.to_string());
                        break;
                    }
                }
            }
        })?;
    Ok(tail)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;

    fn process(script: &str) -> FramedTransport {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        FramedTransport::new(SshConnection::spawn(command).unwrap()).unwrap()
    }

    #[test]
    fn framed_exchange_drains_noisy_stderr_and_closing_rejects_reuse() {
        let mut transport = process("head -c 100000 /dev/zero >&2; exec cat");
        let values = vec!["first", "multiline\nbody", "last"];
        transport.send(&values, Duration::from_secs(5)).unwrap();
        assert_eq!(
            transport
                .receive::<Vec<String>>(Duration::from_secs(5))
                .unwrap(),
            values
        );
        assert_eq!(
            transport.diagnostic_tail().unwrap().len(),
            MAX_BOOTSTRAP_OUTPUT
        );
        transport.close();
        assert_eq!(
            transport
                .send(&values, Duration::from_secs(1))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotConnected
        );
    }

    #[test]
    fn invalid_response_and_timeouts_invalidate_instead_of_replaying() {
        let mut transport = process("printf '\\001\\000\\000\\000{'; exec sleep 30");
        assert_eq!(
            transport
                .receive::<String>(Duration::from_secs(5))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            transport
                .receive::<String>(Duration::from_secs(1))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotConnected
        );
        let mut transport = process("exec sleep 30");
        assert_eq!(
            transport
                .receive::<String>(Duration::from_millis(20))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            transport
                .send(&"must not replay", Duration::from_secs(1))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotConnected
        );
    }
}
