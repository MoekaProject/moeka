//! Reading file paths that were copied in the system file manager.

use std::path::PathBuf;

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

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn parses_uri_list_entries_and_skips_comments() {
        let list = "# some comment\nfile:///tmp/a.md\r\n\nfile:///tmp/my%20b.md\n";
        assert_eq!(
            parse_uri_list(list),
            vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/my b.md")]
        );
    }

    #[test]
    fn ignores_entries_that_are_not_file_urls() {
        let list = "https://moeka.app/\nfile:///tmp/a.md\n";
        assert_eq!(parse_uri_list(list), vec![PathBuf::from("/tmp/a.md")]);
    }
}
