use std::ffi::c_void;
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
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::CreateMutexW;

const MUTEX_NAME: &str = "Local\\MoekaMarkdownEditorSingleInstance";
const PIPE_NAME: &str = r"\\.\pipe\MoekaMarkdownEditorOpenPaths";
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub(crate) enum InstanceRole {
    Primary(Receiver<Vec<PathBuf>>),
    SecondaryForwarded,
    SecondaryFailed,
    PrimaryFallback,
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub(crate) fn acquire(paths: &[PathBuf]) -> InstanceRole {
    let mutex_name = wide(MUTEX_NAME);
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr()) };
    if mutex.is_null() {
        return InstanceRole::PrimaryFallback;
    }

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(mutex) };
        return if forward_paths(paths) {
            InstanceRole::SecondaryForwarded
        } else {
            InstanceRole::SecondaryFailed
        };
    }

    let (sender, receiver) = mpsc::channel();
    let mutex_handle = mutex as usize;
    std::thread::spawn(move || listen_for_paths(sender, mutex_handle));
    InstanceRole::Primary(receiver)
}

fn forward_paths(paths: &[PathBuf]) -> bool {
    let Ok(payload) = serde_json::to_vec(paths) else {
        return false;
    };
    if payload.len() > MAX_MESSAGE_BYTES {
        return false;
    }
    let pipe_name = wide(PIPE_NAME);
    unsafe {
        let mut ready = false;
        for _ in 0..20 {
            if WaitNamedPipeW(pipe_name.as_ptr(), 100) != 0 {
                ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !ready {
            return false;
        }
        let pipe = CreateFileW(
            pipe_name.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if pipe == INVALID_HANDLE_VALUE {
            return false;
        }
        let length = (payload.len() as u32).to_le_bytes();
        let ok = write_all(pipe, &length) && write_all(pipe, &payload);
        CloseHandle(pipe);
        ok
    }
}

fn listen_for_paths(sender: mpsc::Sender<Vec<PathBuf>>, mutex: usize) {
    let pipe_name = wide(PIPE_NAME);
    loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                pipe_name.as_ptr(),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                0,
                MAX_MESSAGE_BYTES as u32,
                0,
                std::ptr::null(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            break;
        }
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
                    if unsafe { read_exact(pipe, &mut payload) }
                        && let Ok(paths) = serde_json::from_slice::<Vec<PathBuf>>(&payload)
                    {
                        let _ = sender.send(paths);
                    }
                }
            }
        }
        unsafe {
            DisconnectNamedPipe(pipe);
            CloseHandle(pipe);
        }
    }
    unsafe { CloseHandle(mutex as *mut c_void) };
}

unsafe fn write_all(handle: *mut c_void, mut bytes: &[u8]) -> bool {
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
            || written == 0
        {
            return false;
        }
        bytes = &bytes[written as usize..];
    }
    true
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
