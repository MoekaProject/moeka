use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalRequest, KillTerminalResponse,
    ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    TerminalExitStatus, TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};

use super::file_ops;

const DEFAULT_OUTPUT_BYTE_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub(crate) struct AcpClientServices {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    roots: Vec<PathBuf>,
    unsaved: Mutex<HashMap<PathBuf, String>>,
    terminals: Mutex<HashMap<String, ManagedTerminal>>,
    next_terminal_id: AtomicU64,
}

struct ManagedTerminal {
    child: Arc<Mutex<Child>>,
    output: Arc<Mutex<TerminalOutputBuffer>>,
}

struct TerminalOutputBuffer {
    text: String,
    byte_limit: usize,
    truncated: bool,
}

impl AcpClientServices {
    pub(crate) fn new(roots: Vec<PathBuf>, unsaved: HashMap<PathBuf, String>) -> Self {
        let roots = roots
            .into_iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .collect();
        Self {
            inner: Arc::new(ServiceInner {
                roots,
                unsaved: Mutex::new(unsaved),
                terminals: Mutex::new(HashMap::new()),
                next_terminal_id: AtomicU64::new(1),
            }),
        }
    }

    pub(crate) fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> agent_client_protocol::Result<ReadTextFileResponse> {
        let path = self.resolve_existing_path(&request.path)?;
        let unsaved = self.inner.unsaved.lock().map_err(lock_error)?;
        let content = unsaved
            .get(&path)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| std::fs::read_to_string(&path))
            .map_err(agent_client_protocol::Error::into_internal_error)?;
        let start = request.line.unwrap_or(1).max(1).saturating_sub(1) as usize;
        let content = if request.line.is_some() || request.limit.is_some() {
            let lines = content.split_inclusive('\n');
            match request.limit {
                Some(limit) => lines.skip(start).take(limit as usize).collect::<String>(),
                None => lines.skip(start).collect::<String>(),
            }
        } else {
            content
        };
        Ok(ReadTextFileResponse::new(content))
    }

    pub(crate) fn write_text_file(
        &self,
        request: WriteTextFileRequest,
    ) -> agent_client_protocol::Result<WriteTextFileResponse> {
        let path = self.resolve_writable_path(&request.path)?;
        file_ops::atomic_write(&path, request.content.as_bytes())
            .map_err(agent_client_protocol::Error::into_internal_error)?;
        self.inner
            .unsaved
            .lock()
            .map_err(lock_error)?
            .insert(path, request.content);
        Ok(WriteTextFileResponse::new())
    }

    pub(crate) fn create_terminal(
        &self,
        request: CreateTerminalRequest,
    ) -> agent_client_protocol::Result<CreateTerminalResponse> {
        let cwd = request
            .cwd
            .as_deref()
            .map(|path| self.resolve_existing_directory(path))
            .transpose()?
            .or_else(|| self.inner.roots.first().cloned())
            .ok_or_else(|| agent_client_protocol::util::internal_error("no workspace root"))?;
        let mut command = Command::new(&request.command);
        command
            .args(&request.args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        for variable in request.env {
            command.env(variable.name, variable.value);
        }
        let mut child = command
            .spawn()
            .map_err(agent_client_protocol::Error::into_internal_error)?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let output = Arc::new(Mutex::new(TerminalOutputBuffer {
            text: String::new(),
            byte_limit: request
                .output_byte_limit
                .and_then(|limit| usize::try_from(limit).ok())
                .unwrap_or(DEFAULT_OUTPUT_BYTE_LIMIT),
            truncated: false,
        }));
        if let Some(stdout) = stdout {
            spawn_output_reader(stdout, output.clone());
        }
        if let Some(stderr) = stderr {
            spawn_output_reader(stderr, output.clone());
        }
        let terminal_id = format!(
            "moeka-terminal-{}",
            self.inner.next_terminal_id.fetch_add(1, Ordering::Relaxed)
        );
        self.inner.terminals.lock().map_err(lock_error)?.insert(
            terminal_id.clone(),
            ManagedTerminal {
                child: Arc::new(Mutex::new(child)),
                output,
            },
        );
        Ok(CreateTerminalResponse::new(terminal_id))
    }

    pub(crate) fn terminal_output(
        &self,
        request: TerminalOutputRequest,
    ) -> agent_client_protocol::Result<TerminalOutputResponse> {
        let (child, output) = self.terminal_handles(&request.terminal_id.0)?;
        let exit_status = child
            .lock()
            .map_err(lock_error)?
            .try_wait()
            .map_err(agent_client_protocol::Error::into_internal_error)?
            .map(exit_status);
        let output = output.lock().map_err(lock_error)?;
        Ok(
            TerminalOutputResponse::new(output.text.clone(), output.truncated)
                .exit_status(exit_status),
        )
    }

    pub(crate) fn wait_for_terminal_exit(
        &self,
        request: WaitForTerminalExitRequest,
    ) -> agent_client_protocol::Result<WaitForTerminalExitResponse> {
        let (child, _) = self.terminal_handles(&request.terminal_id.0)?;
        loop {
            if let Some(status) = child
                .lock()
                .map_err(lock_error)?
                .try_wait()
                .map_err(agent_client_protocol::Error::into_internal_error)?
            {
                return Ok(WaitForTerminalExitResponse::new(exit_status(status)));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub(crate) fn kill_terminal(
        &self,
        request: KillTerminalRequest,
    ) -> agent_client_protocol::Result<KillTerminalResponse> {
        let (child, _) = self.terminal_handles(&request.terminal_id.0)?;
        let mut child = child.lock().map_err(lock_error)?;
        if child
            .try_wait()
            .map_err(agent_client_protocol::Error::into_internal_error)?
            .is_none()
        {
            terminate_process_group(&mut child)
                .map_err(agent_client_protocol::Error::into_internal_error)?;
        }
        Ok(KillTerminalResponse::new())
    }

    pub(crate) fn release_terminal(
        &self,
        request: ReleaseTerminalRequest,
    ) -> agent_client_protocol::Result<ReleaseTerminalResponse> {
        let terminal = self
            .inner
            .terminals
            .lock()
            .map_err(lock_error)?
            .remove(request.terminal_id.0.as_ref())
            .ok_or_else(|| agent_client_protocol::util::internal_error("unknown terminal"))?;
        let mut child = terminal.child.lock().map_err(lock_error)?;
        if child
            .try_wait()
            .map_err(agent_client_protocol::Error::into_internal_error)?
            .is_none()
        {
            let _ = terminate_process_group(&mut child);
            let _ = child.wait();
        }
        Ok(ReleaseTerminalResponse::new())
    }

    fn terminal_handles(
        &self,
        terminal_id: &str,
    ) -> agent_client_protocol::Result<(Arc<Mutex<Child>>, Arc<Mutex<TerminalOutputBuffer>>)> {
        let terminals = self.inner.terminals.lock().map_err(lock_error)?;
        let terminal = terminals
            .get(terminal_id)
            .ok_or_else(|| agent_client_protocol::util::internal_error("unknown terminal"))?;
        Ok((terminal.child.clone(), terminal.output.clone()))
    }

    fn resolve_existing_path(&self, path: &Path) -> agent_client_protocol::Result<PathBuf> {
        if !path.is_absolute() {
            return Err(agent_client_protocol::util::internal_error(
                "ACP file path must be absolute",
            ));
        }
        let canonical = std::fs::canonicalize(path)
            .map_err(agent_client_protocol::Error::into_internal_error)?;
        self.ensure_in_roots(&canonical)?;
        Ok(canonical)
    }

    fn resolve_existing_directory(&self, path: &Path) -> agent_client_protocol::Result<PathBuf> {
        let canonical = self.resolve_existing_path(path)?;
        if !canonical.is_dir() {
            return Err(agent_client_protocol::util::internal_error(
                "ACP terminal cwd is not a directory",
            ));
        }
        Ok(canonical)
    }

    fn resolve_writable_path(&self, path: &Path) -> agent_client_protocol::Result<PathBuf> {
        if !path.is_absolute() {
            return Err(agent_client_protocol::util::internal_error(
                "ACP file path must be absolute",
            ));
        }
        if path.exists() {
            return self.resolve_existing_path(path);
        }
        let parent = path
            .parent()
            .ok_or_else(|| agent_client_protocol::util::internal_error("invalid file path"))?;
        let parent = std::fs::canonicalize(parent)
            .map_err(agent_client_protocol::Error::into_internal_error)?;
        self.ensure_in_roots(&parent)?;
        let file_name = path
            .file_name()
            .ok_or_else(|| agent_client_protocol::util::internal_error("invalid file path"))?;
        Ok(parent.join(file_name))
    }

    fn ensure_in_roots(&self, path: &Path) -> agent_client_protocol::Result<()> {
        if self.inner.roots.iter().any(|root| path.starts_with(root)) {
            Ok(())
        } else {
            Err(agent_client_protocol::util::internal_error(
                "ACP path is outside the session workspace roots",
            ))
        }
    }
}

impl Drop for ServiceInner {
    fn drop(&mut self) {
        if let Ok(mut terminals) = self.terminals.lock() {
            for (_, terminal) in terminals.drain() {
                if let Ok(mut child) = terminal.child.lock() {
                    let _ = terminate_process_group(&mut child);
                    let _ = child.wait();
                }
            }
        }
    }
}

fn spawn_output_reader(
    mut reader: impl Read + Send + 'static,
    output: Arc<Mutex<TerminalOutputBuffer>>,
) {
    std::thread::spawn(move || {
        let mut bytes = [0u8; 8192];
        loop {
            let Ok(read) = reader.read(&mut bytes) else {
                break;
            };
            if read == 0 {
                break;
            }
            if let Ok(mut output) = output.lock() {
                output.push(&String::from_utf8_lossy(&bytes[..read]));
            }
        }
    });
}

impl TerminalOutputBuffer {
    fn push(&mut self, text: &str) {
        self.text.push_str(text);
        if self.text.len() <= self.byte_limit {
            return;
        }
        let mut start = self.text.len().saturating_sub(self.byte_limit);
        while start < self.text.len() && !self.text.is_char_boundary(start) {
            start += 1;
        }
        self.text.drain(..start);
        self.truncated = true;
    }
}

fn exit_status(status: ExitStatus) -> TerminalExitStatus {
    let exit_code = status.code().and_then(|code| u32::try_from(code).ok());
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|signal| signal.to_string())
    };
    #[cfg(not(unix))]
    let signal = None;
    TerminalExitStatus::new()
        .exit_code(exit_code)
        .signal(signal)
}

fn terminate_process_group(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as i32);
        let result = unsafe { libc::kill(process_group, libc::SIGKILL) };
        if result == 0 {
            return Ok(());
        }
    }
    child.kill()
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> agent_client_protocol::Error {
    agent_client_protocol::util::internal_error(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{SessionId, TerminalId};

    #[test]
    fn rejects_paths_outside_workspace_root() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let services = AcpClientServices::new(vec![workspace.path().to_path_buf()], HashMap::new());

        assert!(services.resolve_existing_path(outside.path()).is_err());
    }

    #[test]
    fn terminal_output_truncates_on_utf8_boundary() {
        let mut output = TerminalOutputBuffer {
            text: String::new(),
            byte_limit: 5,
            truncated: false,
        };
        output.push("a中b中");

        assert!(output.text.is_char_boundary(0));
        assert!(output.text.len() <= 5);
        assert!(output.truncated);
    }

    #[test]
    fn creates_reads_and_releases_terminal() {
        let workspace = tempfile::tempdir().unwrap();
        let services = AcpClientServices::new(vec![workspace.path().to_path_buf()], HashMap::new());
        let session = SessionId::new("test");
        let created = services
            .create_terminal(
                CreateTerminalRequest::new(session.clone(), "/bin/sh")
                    .args(vec!["-c".to_string(), "printf hello".to_string()])
                    .cwd(Some(workspace.path().to_path_buf())),
            )
            .unwrap();
        services
            .wait_for_terminal_exit(WaitForTerminalExitRequest::new(
                session.clone(),
                created.terminal_id.clone(),
            ))
            .unwrap();
        let output = services
            .terminal_output(TerminalOutputRequest::new(
                session.clone(),
                created.terminal_id.clone(),
            ))
            .unwrap();
        assert_eq!(output.output, "hello");
        services
            .release_terminal(ReleaseTerminalRequest::new(
                session,
                TerminalId::new(created.terminal_id.0.clone()),
            ))
            .unwrap();
    }
}
