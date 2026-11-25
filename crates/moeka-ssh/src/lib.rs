//! OpenSSH transport and verified, user-scoped remote server installation.

use std::io::{self, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

pub mod bundle;
pub mod filesystem;
pub mod transport;

const MAX_BOOTSTRAP_OUTPUT: usize = 16 * 1024;
const MAX_PAYLOAD_BYTES: usize = 128 * 1024 * 1024;
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(30);
static BOOTSTRAP_SLOTS: AtomicUsize = AtomicUsize::new(0);

struct BootstrapPermit;

impl BootstrapPermit {
    fn acquire() -> io::Result<Arc<Self>> {
        BOOTSTRAP_SLOTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < 4).then_some(active + 1)
            })
            .map(|_| Arc::new(Self))
            .map_err(|_| {
                io::Error::new(io::ErrorKind::WouldBlock, "SSH bootstrap capacity is busy")
            })
    }
}

impl Drop for BootstrapPermit {
    fn drop(&mut self) {
        BOOTSTRAP_SLOTS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) struct BootstrapProcess(pub(crate) Child);

impl Drop for BootstrapProcess {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[derive(Clone, Debug)]
pub struct SshOptions {
    pub destination: String,
    pub port: Option<u16>,
    pub config_file: Option<std::path::PathBuf>,
}

impl SshOptions {
    pub fn command(&self) -> io::Result<Command> {
        if self.destination.is_empty()
            || self.destination.starts_with('-')
            || self
                .destination
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
            || self.port == Some(0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid SSH destination",
            ));
        }
        let mut command = Command::new("ssh");
        command.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ConnectionAttempts=1",
        ]);
        if let Some(port) = self.port {
            command.args(["-p", &port.to_string()]);
        }
        if let Some(path) = &self.config_file {
            command.arg("-F").arg(path);
        }
        command.arg("--").arg(&self.destination);
        Ok(command)
    }

    fn shell(&self, script: &str) -> io::Result<Command> {
        let mut command = self.command()?;
        command.arg(format!("sh -c {}", quote_shell(script)?));
        Ok(command)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerTarget {
    LinuxX86_64,
    LinuxAarch64,
    MacosX86_64,
    MacosAarch64,
}

impl ServerTarget {
    pub fn parse(os: &str, arch: &str) -> io::Result<Self> {
        match (os, arch) {
            ("Linux", "x86_64") => Ok(Self::LinuxX86_64),
            ("Linux", "aarch64" | "arm64") => Ok(Self::LinuxAarch64),
            ("Darwin", "x86_64") => Ok(Self::MacosX86_64),
            ("Darwin", "arm64" | "aarch64") => Ok(Self::MacosAarch64),
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("unsupported remote platform: {os}/{arch}"),
            )),
        }
    }

    /// `case` pattern over `uname -s`/`uname -m` accepted for this target.
    fn uname_pattern(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "Linux/x86_64",
            Self::LinuxAarch64 => "Linux/aarch64|Linux/arm64",
            Self::MacosX86_64 => "Darwin/x86_64",
            Self::MacosAarch64 => "Darwin/arm64|Darwin/aarch64",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "linux-x86_64",
            Self::LinuxAarch64 => "linux-aarch64",
            Self::MacosX86_64 => "macos-x86_64",
            Self::MacosAarch64 => "macos-aarch64",
        }
    }
}

pub struct ServerPayload<'a> {
    pub target: ServerTarget,
    pub version: &'a str,
    pub sha256: &'a str,
    pub bytes: &'a [u8],
}

impl ServerPayload<'_> {
    fn verify(&self) -> io::Result<()> {
        if self.bytes.is_empty()
            || self.bytes.len() > MAX_PAYLOAD_BYTES
            || self.version.is_empty()
            || !self
                .version
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b".-_".contains(&ch))
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid remote server manifest",
            ));
        }
        let digest = format!("{:x}", Sha256::digest(self.bytes));
        if digest != self.sha256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "remote server payload digest mismatch",
            ));
        }
        Ok(())
    }

    fn relative_directory(&self) -> String {
        format!("{}-{}-{}", self.version, self.target.name(), self.sha256)
    }
}

pub fn detect_target(options: &SshOptions) -> io::Result<ServerTarget> {
    let output = run_bootstrap(
        options.shell("printf 'MOEKA_REMOTE_PLATFORM\\n'; uname -s; uname -m")?,
        &[],
        BOOTSTRAP_TIMEOUT,
    )?;
    let text = std::str::from_utf8(&output)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut lines = text.lines();
    if lines.next() != Some("MOEKA_REMOTE_PLATFORM") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected SSH bootstrap output",
        ));
    }
    let os = lines.next().unwrap_or_default();
    let arch = lines.next().unwrap_or_default();
    if lines.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected SSH platform response",
        ));
    }
    ServerTarget::parse(os, arch)
}

/// Verify before executing; reuse a valid installation without retransmitting it.
pub fn install(options: &SshOptions, payload: &ServerPayload<'_>) -> io::Result<bool> {
    payload.verify()?;
    let directory = payload.relative_directory();
    // Both scripts resolve a symlinked home (e.g. /home -> /data/home) once, then
    // require every directory below it to be a real, user-owned directory.
    let probe = format!(
        r#"set -eu
case "$(uname -s)/$(uname -m)" in
    {platform}) ;;
    *) printf 'MOEKA_REMOTE_PLATFORM_MISMATCH\n'; exit 0 ;;
esac
home=$(cd "$HOME" && pwd -P) || exit 71
base="$home/.local/share/moeka/remote-server"
dir="$base/{directory}"
exe="$dir/moeka-remote-server"
for parent in "$home" "$home/.local" "$home/.local/share" "$home/.local/share/moeka" "$base" "$dir"; do
    if [ -e "$parent" ] || [ -L "$parent" ]; then
        [ -d "$parent" ] && [ ! -L "$parent" ] && [ -O "$parent" ] || exit 71
    fi
done
if [ -f "$exe" ] && [ ! -L "$exe" ]; then
    {hash_command}
    if [ "$digest" = '{digest}' ]; then
        printf 'MOEKA_REMOTE_READY\n'
        exit 0
    fi
fi
if [ -e "$exe" ] || [ -L "$exe" ]; then
    printf 'Existing server failed integrity verification\n' >&2
    exit 75
fi
printf 'MOEKA_REMOTE_INSTALL\n'
"#,
        hash_command = hash_script("\"$exe\""),
        digest = payload.sha256,
        platform = payload.target.uname_pattern(),
    );
    // The probe checks the platform itself, saving a separate SSH round trip.
    let output = run_bootstrap(options.shell(&probe)?, &[], BOOTSTRAP_TIMEOUT)?;
    if output == b"MOEKA_REMOTE_PLATFORM_MISMATCH\n" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "remote server target mismatch",
        ));
    }
    if output == b"MOEKA_REMOTE_READY\n" {
        return Ok(false);
    }
    if output != b"MOEKA_REMOTE_INSTALL\n" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected remote install probe",
        ));
    }
    let upload = format!(
        r#"set -eu
umask 077
home=$(cd "$HOME" && pwd -P) || exit 71
base="$home/.local/share/moeka/remote-server"
dir="$base/{directory}"
for parent in "$home" "$home/.local" "$home/.local/share" "$home/.local/share/moeka" "$base" "$dir"; do
    if [ ! -e "$parent" ] && [ ! -L "$parent" ]; then
        mkdir "$parent" || [ -d "$parent" ]
    fi
    [ -d "$parent" ] && [ ! -L "$parent" ] && [ -O "$parent" ] || exit 71
done
exe="$dir/moeka-remote-server"
tmp=$(mktemp "$dir/.install.XXXXXXXX")
trap 'rm -f "$tmp"' EXIT HUP INT TERM
cat > "$tmp"
{hash_command}
[ "$digest" = '{digest}' ] || exit 73
chmod 700 "$tmp"
if [ -d "$exe" ] || [ -L "$exe" ]; then
    printf 'Refusing invalid server destination\n' >&2
    exit 75
fi
ln "$tmp" "$exe" 2>/dev/null || [ -f "$exe" ]
[ -f "$exe" ] && [ ! -L "$exe" ] || exit 75
{existing_hash_command}
[ "$digest" = '{digest}' ] || exit 75
printf 'MOEKA_REMOTE_INSTALLED\n'
"#,
        hash_command = hash_script("\"$tmp\""),
        existing_hash_command = hash_script("\"$exe\""),
        digest = payload.sha256,
    );
    let output = run_bootstrap(options.shell(&upload)?, payload.bytes, BOOTSTRAP_TIMEOUT)?;
    if output != b"MOEKA_REMOTE_INSTALLED\n" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "remote installation was not acknowledged",
        ));
    }
    Ok(true)
}

fn hash_script(path: &str) -> String {
    format!(
        r#"if command -v sha256sum >/dev/null 2>&1; then
    digest=$(sha256sum {path})
elif command -v shasum >/dev/null 2>&1; then
    digest=$(shasum -a 256 {path})
else
    printf 'SHA-256 tool unavailable\n' >&2
    exit 74
fi
digest=${{digest%% *}}"#
    )
}

/// Owns the SSH process; closing the connection never leaves an agent daemon.
pub struct SshConnection {
    process: BootstrapProcess,
    pub input: ChildStdin,
    pub output: ChildStdout,
    pub diagnostics: ChildStderr,
}

impl SshConnection {
    pub fn start(options: &SshOptions, payload: &ServerPayload<'_>) -> io::Result<Self> {
        install(options, payload)?;
        let directory = payload.relative_directory();
        let script = format!(
            "exec \"$HOME/.local/share/moeka/remote-server/{directory}/moeka-remote-server\" --stdio"
        );
        Self::spawn(options.shell(&script)?)
    }

    fn spawn(mut command: Command) -> io::Result<Self> {
        let mut process = BootstrapProcess(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
        );
        let input = process
            .0
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("SSH stdin unavailable"))?;
        let output = process
            .0
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("SSH stdout unavailable"))?;
        let diagnostics = process
            .0
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("SSH stderr unavailable"))?;
        Ok(Self {
            process,
            input,
            output,
            diagnostics,
        })
    }

    pub fn process_id(&self) -> u32 {
        self.process.0.id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.process.0.try_wait()
    }
}

fn quote_shell(value: &str) -> io::Result<String> {
    if value.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in remote shell argument",
        ));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

fn run_bootstrap(mut command: Command, bytes: &[u8], timeout: Duration) -> io::Result<Vec<u8>> {
    let permit = BootstrapPermit::acquire()?;
    let mut child = BootstrapProcess(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let mut stdin = child
        .0
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("bootstrap stdin unavailable"))?;
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("bootstrap stdout unavailable"))?;
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("bootstrap stderr unavailable"))?;
    let input = bytes.to_vec();
    let (writer_tx, writer_rx) = mpsc::sync_channel(1);
    let writer_permit = permit.clone();
    thread::Builder::new()
        .name("moeka-ssh-upload".into())
        .spawn(move || {
            let _permit = writer_permit;
            let result = stdin.write_all(&input);
            drop(stdin);
            let _ = writer_tx.send(result);
        })?;
    let stdout = read_bounded(stdout, permit.clone())?;
    let stderr = read_bounded(stderr, permit.clone())?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.0.try_wait()? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.0.kill();
                let _ = child.0.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "SSH bootstrap timed out; remote installation state is unknown",
                ));
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    };
    let receive = |rx: mpsc::Receiver<io::Result<Vec<u8>>>| {
        rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "SSH bootstrap pipe did not close")
            })?
    };
    let output = receive(stdout)?;
    let errors = receive(stderr)?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "SSH bootstrap failed ({status}): {}",
            String::from_utf8_lossy(&errors)
        )));
    }
    writer_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "SSH bootstrap upload did not finish",
            )
        })??;
    Ok(output)
}

fn read_bounded(
    mut reader: impl Read + Send + 'static,
    permit: Arc<BootstrapPermit>,
) -> io::Result<mpsc::Receiver<io::Result<Vec<u8>>>> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("moeka-ssh-diagnostics".into())
        .spawn(move || {
            let _permit = permit;
            let mut output = Vec::new();
            let mut buffer = [0; 4096];
            let mut overflow = false;
            let result = loop {
                match reader.read(&mut buffer) {
                    Ok(0) if overflow => {
                        break Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "SSH bootstrap output limit exceeded",
                        ));
                    }
                    Ok(0) => break Ok(output),
                    Ok(count) => {
                        let retained = count.min(MAX_BOOTSTRAP_OUTPUT.saturating_sub(output.len()));
                        output.extend_from_slice(&buffer[..retained]);
                        overflow |= retained < count;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => break Err(error),
                }
            };
            let _ = tx.send(result);
        })?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_arguments_are_literal_and_never_relax_host_identity() {
        let options = SshOptions {
            destination: "user@host".into(),
            port: Some(2222),
            config_file: Some("config with spaces".into()),
        };
        let command = options.shell("printf '%s' 'a b'").unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert!(args.contains(&"StrictHostKeyChecking=yes".into()));
        assert!(args.contains(&"BatchMode=yes".into()));
        assert_eq!(quote_shell("a'b").unwrap(), "'a'\\''b'");
        assert!(args.contains(&"config with spaces".into()));
        assert!(args.windows(2).any(|args| args == ["-p", "2222"]));
        assert!(
            SshOptions {
                destination: "-oProxyCommand=bad".into(),
                ..options
            }
            .command()
            .is_err()
        );
    }

    #[test]
    fn installation_rejects_bad_payload_before_spawning_ssh() {
        let options = SshOptions {
            destination: "unused".into(),
            port: None,
            config_file: None,
        };
        let payload = ServerPayload {
            target: ServerTarget::LinuxX86_64,
            version: "0.1.0",
            sha256: &"0".repeat(64),
            bytes: b"bad",
        };
        assert_eq!(
            install(&options, &payload).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn bootstrap_bounds_output_and_reports_nonzero_exit() {
        #[cfg(unix)]
        {
            let mut command = Command::new("sh");
            command.args(["-c", "printf ready; printf diagnostic >&2"]);
            assert_eq!(
                run_bootstrap(command, &[], Duration::from_secs(5)).unwrap(),
                b"ready"
            );
            let mut command = Command::new("sh");
            command.args(["-c", "printf denied >&2; exit 7"]);
            let error = run_bootstrap(command, &[], Duration::from_secs(5)).unwrap_err();
            assert!(error.to_string().contains("denied"));

            let mut command = Command::new("sh");
            command.args(["-c", "head -c 20000 /dev/zero"]);
            assert_eq!(
                run_bootstrap(command, &[], Duration::from_secs(5))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
    }
}
