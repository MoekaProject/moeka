use std::{io, path::PathBuf};

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

fn encode_paths(paths: &[PathBuf]) -> io::Result<Vec<u8>> {
    let paths = paths
        .iter()
        .map(std::path::absolute)
        .collect::<io::Result<Vec<_>>>()?;
    let payload = serde_json::to_vec(&paths).map_err(io::Error::other)?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::runtime::RuntimeText::InvalidRequest.to_string(),
        ));
    }
    Ok(payload)
}

fn decode_paths(payload: &[u8]) -> io::Result<Vec<PathBuf>> {
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            crate::i18n::runtime::RuntimeText::InvalidRequest.to_string(),
        ));
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if paths.iter().any(|path| !path.is_absolute()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            crate::i18n::runtime::RuntimeText::AbsolutePathRequired.to_string(),
        ));
    }
    Ok(paths)
}

#[cfg(windows)]
pub(crate) use backend::{InstanceRole, acquire, show_startup_error};

#[cfg(windows)]
mod backend {
    use super::{MAX_MESSAGE_BYTES, decode_paths, encode_paths};
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GENERIC_WRITE, GetLastError,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
        PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, MB_ICONERROR, MB_OK, MessageBoxW,
    };

    const MUTEX_NAME: &str = "Local\\MoekaMarkdownEditorSingleInstance";
    const PIPE_NAME: &str = r"\\.\pipe\MoekaMarkdownEditorOpenPaths";

    pub(crate) enum InstanceRole {
        Primary(Receiver<Vec<PathBuf>>, InstanceGuard),
        SecondaryForwarded,
    }

    pub(crate) struct InstanceGuard(usize);

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0 as *mut c_void);
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(crate) fn acquire(paths: &[PathBuf]) -> io::Result<InstanceRole> {
        let mutex_name = wide(MUTEX_NAME);
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr()) };
        if mutex.is_null() {
            return Err(io::Error::last_os_error());
        }

        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(mutex) };
            forward_paths(paths)?;
            return Ok(InstanceRole::SecondaryForwarded);
        }

        let pipe = match create_pipe() {
            Ok(pipe) => pipe,
            Err(error) => {
                unsafe { CloseHandle(mutex) };
                return Err(error);
            }
        };
        let (sender, receiver) = mpsc::sync_channel(32);
        let pipe_handle = pipe as usize;
        if let Err(error) = std::thread::Builder::new()
            .name("moeka-open-files".into())
            .spawn(move || listen_for_paths(sender, pipe_handle))
        {
            unsafe {
                CloseHandle(pipe);
                CloseHandle(mutex);
            }
            return Err(error);
        }
        Ok(InstanceRole::Primary(
            receiver,
            InstanceGuard(mutex as usize),
        ))
    }

    pub(crate) fn show_startup_error(error: &io::Error) {
        let message = wide(&crate::i18n::runtime::rt!(FileOpenDelivery, error).to_string());
        let title = wide(crate::i18n::application_display_name(
            crate::i18n::runtime::language(),
        ));
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    fn forward_paths(paths: &[PathBuf]) -> io::Result<()> {
        // Relative paths belong to the launching process, not the existing window's CWD.
        let payload = encode_paths(paths)?;
        let pipe_name = wide(PIPE_NAME);
        unsafe {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let pipe = loop {
                if WaitNamedPipeW(pipe_name.as_ptr(), 100) != 0 {
                    let pipe = CreateFileW(
                        pipe_name.as_ptr(),
                        GENERIC_WRITE,
                        0,
                        std::ptr::null(),
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        std::ptr::null_mut(),
                    );
                    if pipe != INVALID_HANDLE_VALUE {
                        break pipe;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        crate::i18n::runtime::RuntimeText::FileOpenDelivery.to_string(),
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            };
            // Explorer starts this process with foreground permission. Transfer it
            // to the existing instance before delivering the open request; granting
            // only the connected server PID avoids authorizing unrelated processes.
            let mut primary_pid = 0;
            if GetNamedPipeServerProcessId(pipe, &mut primary_pid) != 0 && primary_pid != 0 {
                AllowSetForegroundWindow(primary_pid);
            }
            let length = (payload.len() as u32).to_le_bytes();
            let result = write_all(pipe, &length).and_then(|_| write_all(pipe, &payload));
            CloseHandle(pipe);
            result
        }
    }

    fn create_pipe() -> io::Result<*mut c_void> {
        let pipe_name = wide(PIPE_NAME);
        let pipe = unsafe {
            CreateNamedPipeW(
                pipe_name.as_ptr(),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                MAX_MESSAGE_BYTES as u32,
                0,
                std::ptr::null(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(pipe)
        }
    }

    fn listen_for_paths(sender: mpsc::SyncSender<Vec<PathBuf>>, first_pipe: usize) {
        let mut pipe = first_pipe as *mut c_void;
        loop {
            let connected = unsafe {
                ConnectNamedPipe(pipe, std::ptr::null_mut()) != 0
                    || GetLastError() == ERROR_PIPE_CONNECTED
            };
            if connected {
                let mut length = [0u8; 4];
                if unsafe { read_exact(pipe, &mut length) } {
                    let length = u32::from_le_bytes(length) as usize;
                    if length <= MAX_MESSAGE_BYTES {
                        let mut payload = vec![0u8; length];
                        if unsafe { read_exact(pipe, &mut payload) } {
                            match decode_paths(&payload) {
                                Ok(paths) => {
                                    if sender.send(paths).is_err() {
                                        unsafe {
                                            DisconnectNamedPipe(pipe);
                                            CloseHandle(pipe);
                                        }
                                        break;
                                    }
                                }
                                Err(error) => {
                                    log_error!("invalid external file-open request: {error}")
                                }
                            }
                        } else {
                            log_error!("invalid external file-open request");
                        }
                    } else {
                        log_error!("external file-open request exceeds 1 MiB");
                    }
                }
            }
            unsafe {
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            }
            pipe = match create_pipe() {
                Ok(pipe) => pipe,
                Err(error) => {
                    log_error!("file-open listener stopped: {error}");
                    break;
                }
            };
        }
    }

    unsafe fn write_all(handle: *mut c_void, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            let mut written = 0u32;
            if unsafe {
                WriteFile(
                    handle,
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    crate::i18n::runtime::RuntimeText::FileOpenDelivery.to_string(),
                ));
            }
            bytes = &bytes[written as usize..];
        }
        Ok(())
    }

    unsafe fn read_exact(handle: *mut c_void, mut bytes: &mut [u8]) -> bool {
        while !bytes.is_empty() {
            let mut read = 0u32;
            if unsafe {
                ReadFile(
                    handle,
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            } == 0
                || read == 0
            {
                return false;
            }
            bytes = &mut bytes[read as usize..];
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_preserve_multiple_unicode_paths_and_sender_working_directory() {
        let paths = vec![
            PathBuf::from("notes/示例 file.md"),
            PathBuf::from("other.md"),
        ];
        let decoded = decode_paths(&encode_paths(&paths).unwrap()).unwrap();
        assert_eq!(
            decoded,
            paths
                .iter()
                .map(|path| std::path::absolute(path).unwrap())
                .collect::<Vec<_>>()
        );
        assert!(decoded.iter().all(|path| path.is_absolute()));
        assert!(
            decode_paths(&encode_paths(&[]).unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_relative_and_oversized_requests_are_rejected() {
        assert!(decode_paths(b"not json").is_err());
        assert!(decode_paths(br#"["relative.md"]"#).is_err());
        assert!(decode_paths(&vec![b' '; MAX_MESSAGE_BYTES + 1]).is_err());
        assert!(encode_paths(&[PathBuf::from("a".repeat(MAX_MESSAGE_BYTES))]).is_err());
    }
}
