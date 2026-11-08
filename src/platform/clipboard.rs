//! System file clipboard integration.

use std::path::{Path, PathBuf};

/// Keep the exported file alive after publication, including after app exit.
/// The OS temporary directory owns its eventual cleanup; dropping it immediately
/// would leave Finder/Explorer with a dangling file reference before paste.
pub(crate) struct ExportFile {
    directory: tempfile::TempDir,
    path: PathBuf,
}

impl ExportFile {
    pub(crate) fn prepare(name: &str, bytes: &[u8]) -> std::io::Result<Self> {
        if Path::new(name).file_name().and_then(|name| name.to_str()) != Some(name) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                crate::i18n::runtime::RuntimeText::InvalidPath.to_string(),
            ));
        }
        let directory = tempfile::Builder::new()
            .prefix("moeka-clipboard-")
            .tempdir()?;
        let path = directory.path().join(name);
        std::fs::write(&path, bytes)?;
        Ok(Self { directory, path })
    }

    pub(crate) fn publish(self) -> anyhow::Result<()> {
        // A native write may have taken effect even if its acknowledgement
        // fails, so never remove a potentially published file on that path.
        let _ = self.directory.keep();
        write_file(&self.path)
    }
}

#[cfg(target_os = "macos")]
fn write_file(path: &Path) -> anyhow::Result<()> {
    use objc::{class, msg_send, sel, sel_impl};
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    unsafe {
        let url: *mut objc::runtime::Object = msg_send![
            class!(NSURL),
            fileURLWithFileSystemRepresentation: path.as_ptr()
            isDirectory: false
            relativeToURL: std::ptr::null_mut::<objc::runtime::Object>()
        ];
        anyhow::ensure!(
            !url.is_null(),
            crate::i18n::runtime::RuntimeText::InvalidPath.to_string()
        );
        let pasteboard: *mut objc::runtime::Object =
            msg_send![class!(NSPasteboard), generalPasteboard];
        anyhow::ensure!(
            !pasteboard.is_null(),
            crate::i18n::runtime::RuntimeText::ClipboardFailed.to_string()
        );
        let objects: *mut objc::runtime::Object = msg_send![class!(NSArray), arrayWithObject: url];
        let _: isize = msg_send![pasteboard, clearContents];
        let success: bool = msg_send![pasteboard, writeObjects: objects];
        anyhow::ensure!(
            success,
            crate::i18n::runtime::RuntimeText::ClipboardFailed.to_string()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn write_file(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::CF_HDROP,
        },
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow},
    };

    let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
    name.extend([0, 0]);
    // DROPFILES: pFiles, POINT, fNC, fWide, followed by a double-NUL path list.
    let mut bytes = Vec::with_capacity(20 + name.len() * 2);
    for field in [20u32, 0, 0, 0, 1] {
        bytes.extend(field.to_le_bytes());
    }
    for unit in name {
        bytes.extend(unit.to_le_bytes());
    }
    unsafe {
        let owner = CreateWindowExW(
            0,
            windows_sys::w!("STATIC"),
            windows_sys::w!("Moeka Clipboard"),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        anyhow::ensure!(
            !owner.is_null(),
            crate::i18n::runtime::rt!(ClipboardFailed, std::io::Error::last_os_error())
        );
        let result = (|| {
            anyhow::ensure!(
                OpenClipboard(owner) != 0,
                crate::i18n::runtime::rt!(ClipboardFailed, std::io::Error::last_os_error())
            );
            let result = (|| {
                let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
                anyhow::ensure!(
                    !memory.is_null(),
                    crate::i18n::runtime::RuntimeText::ClipboardFailed.to_string()
                );
                let pointer = GlobalLock(memory);
                if pointer.is_null() {
                    GlobalFree(memory);
                    anyhow::bail!(crate::i18n::runtime::RuntimeText::ClipboardFailed.to_string());
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
                GlobalUnlock(memory);
                if EmptyClipboard() == 0 || SetClipboardData(CF_HDROP as u32, memory).is_null() {
                    let error = std::io::Error::last_os_error();
                    GlobalFree(memory);
                    anyhow::bail!(crate::i18n::runtime::rt!(ClipboardFailed, error));
                }
                Ok(())
            })();
            CloseClipboard();
            result
        })();
        DestroyWindow(owner);
        result
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn write_file(path: &Path) -> anyhow::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    anyhow::ensure!(
        super::capabilities::CHILD_PROCESSES,
        crate::i18n::runtime::RuntimeText::FeatureUnavailable.to_string()
    );
    let uri = super::paths::path_to_file_url(path).ok_or_else(|| {
        anyhow::anyhow!(crate::i18n::runtime::RuntimeText::InvalidPath.to_string())
    })?;
    let (program, args): (&str, &[&str]) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ("wl-copy", &["--type", "text/uri-list"])
    } else {
        ("xclip", &["-selection", "clipboard", "-t", "text/uri-list"])
    };
    // Helpers fork to retain clipboard ownership. Do not wait for EOF on a
    // stderr pipe inherited by that long-lived owner.
    let errors = tempfile::NamedTempFile::new()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(errors.reopen()?)
        .spawn()
        .map_err(|error| {
            anyhow::anyhow!(crate::i18n::runtime::rt!(HelperUnavailable, program, error))
        })?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!(crate::i18n::runtime::rt!(ClipboardFailed, program)))?
        .write_all(format!("{uri}\r\n").as_bytes());
    let status = child.wait()?;
    input?;
    anyhow::ensure!(
        status.success(),
        crate::i18n::runtime::rt!(
            ClipboardFailed,
            program,
            std::fs::read_to_string(errors.path())?
        )
    );
    Ok(())
}

/// Paths of files copied to the system clipboard, newest selection first.
///
/// Returns an empty list when the clipboard holds no file references, which
/// lets callers fall back to interpreting the clipboard text.
#[cfg(target_os = "macos")]
pub(crate) fn file_paths() -> Vec<PathBuf> {
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let pasteboard: *mut objc::runtime::Object =
            msg_send![class!(NSPasteboard), generalPasteboard];
        if pasteboard.is_null() {
            return Vec::new();
        }

        let items: *mut objc::runtime::Object = msg_send![pasteboard, pasteboardItems];
        if items.is_null() {
            return Vec::new();
        }

        let count: usize = msg_send![items, count];
        let file_url_type = nsstring("public.file-url");
        let mut paths = Vec::new();

        for index in 0..count {
            let item: *mut objc::runtime::Object = msg_send![items, objectAtIndex: index];
            if item.is_null() {
                continue;
            }

            let url_string: *mut objc::runtime::Object =
                msg_send![item, stringForType: file_url_type];
            if url_string.is_null() {
                continue;
            }

            let url: *mut objc::runtime::Object =
                msg_send![class!(NSURL), URLWithString: url_string];
            if url.is_null() {
                continue;
            }

            let path_string: *mut objc::runtime::Object = msg_send![url, path];
            if let Some(path) = path_from_nsstring(path_string) {
                paths.push(path);
            }
        }

        paths
    }
}

#[cfg(target_os = "macos")]
fn nsstring(text: &str) -> *mut objc::runtime::Object {
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let c_string = std::ffi::CString::new(text).expect("NSString input contains interior NUL");
        msg_send![class!(NSString), stringWithUTF8String: c_string.as_ptr()]
    }
}

#[cfg(target_os = "macos")]
fn path_from_nsstring(value: *mut objc::runtime::Object) -> Option<PathBuf> {
    use objc::{msg_send, sel, sel_impl};

    if value.is_null() {
        return None;
    }

    unsafe {
        let utf8: *const std::os::raw::c_char = msg_send![value, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(PathBuf::from(
            std::ffi::CStr::from_ptr(utf8)
                .to_string_lossy()
                .into_owned(),
        ))
    }
}

#[cfg(windows)]
pub(crate) fn file_paths() -> Vec<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::System::Ole::CF_HDROP;
    use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Vec::new();
        }

        let handle = GetClipboardData(CF_HDROP as u32);
        if handle.is_null() {
            CloseClipboard();
            return Vec::new();
        }

        let files = handle as HDROP;
        // `u32::MAX` asks for the entry count instead of a path.
        let count = DragQueryFileW(files, u32::MAX, std::ptr::null_mut(), 0);
        let mut paths = Vec::with_capacity(count as usize);

        for index in 0..count {
            // The reported length excludes the terminating NUL.
            let length = DragQueryFileW(files, index, std::ptr::null_mut(), 0);
            if length == 0 {
                continue;
            }

            let mut buffer = vec![0u16; length as usize + 1];
            let written = DragQueryFileW(files, index, buffer.as_mut_ptr(), buffer.len() as u32);
            if written == 0 {
                continue;
            }

            buffer.truncate(written as usize);
            paths.push(PathBuf::from(OsString::from_wide(&buffer)));
        }

        CloseClipboard();
        paths
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn file_paths() -> Vec<PathBuf> {
    const URI_LIST: &str = "text/uri-list";

    read_clipboard("wl-paste", &["--no-newline", "--type", URI_LIST])
        .or_else(|| read_clipboard("xclip", &["-selection", "clipboard", "-o", "-t", URI_LIST]))
        .map(|text| parse_uri_list(&text))
        .unwrap_or_default()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn read_clipboard(program: &str, args: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};

    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        // RFC 2483 reserves lines starting with `#` for comments.
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(super::paths::path_from_file_url)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn parses_uri_list_entries_and_skips_comments() {
        let list = "# some comment\nfile:///tmp/a.md\r\n\nfile:///tmp/my%20b.md\n";
        assert_eq!(
            parse_uri_list(list),
            vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/my b.md")]
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn ignores_entries_that_are_not_file_urls() {
        let list = "https://moeka.app/\nfile:///tmp/a.md\n";
        assert_eq!(parse_uri_list(list), vec![PathBuf::from("/tmp/a.md")]);
    }
    #[test]
    fn clipboard_exports_preserve_names_bytes_and_clean_unpublished_files() {
        for (name, bytes) in [
            ("note.md", b"# Title\n".as_slice()),
            ("note.html", b"<!DOCTYPE html><html></html>".as_slice()),
            ("note.pdf", b"%PDF-1.7\n\0binary".as_slice()),
            ("note.png", b"\x89PNG\r\n\x1a\n\0binary".as_slice()),
            ("note with spaces \u{4e2d}.md", b"content".as_slice()),
        ] {
            let file = ExportFile::prepare(name, bytes).unwrap();
            let path = file.path.clone();
            assert_eq!(path.file_name().unwrap(), name);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            drop(file);
            assert!(!path.exists());
        }
    }

    #[test]
    fn clipboard_exports_reject_paths_and_empty_names() {
        for name in [
            "",
            ".",
            "..",
            "../note.md",
            "/tmp/note.md",
            "folder/note.md",
        ] {
            assert!(ExportFile::prepare(name, b"content").is_err(), "{name}");
        }
    }
}
