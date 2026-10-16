//! Shared Objective-C bridging helpers for the macOS backends.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

pub(crate) fn nsstring(text: &str) -> *mut Object {
    unsafe {
        msg_send![class!(NSString),
            stringWithBytes: text.as_ptr()
            length: text.len()
            encoding: 4usize] // NSUTF8StringEncoding
    }
}

pub(crate) fn nsstring_to_string(value: *mut Object) -> Option<String> {
    if value.is_null() {
        return None;
    }

    unsafe {
        let utf8: *const std::os::raw::c_char = msg_send![value, UTF8String];
        if utf8.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(utf8)
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

/// A `file://` NSURL for an absolute path using only a lexical directory hint.
pub(crate) fn file_url(path: &Path) -> *mut Object {
    file_url_with_directory(path, path.as_os_str().as_encoded_bytes().ends_with(b"/"))
}

/// Callers with worker-cached metadata can supply the actual directory flag.
/// Relative paths are rejected rather than asking Foundation to resolve the CWD.
pub(crate) fn file_url_with_directory(path: &Path, is_directory: bool) -> *mut Object {
    if !path.is_absolute() {
        return std::ptr::null_mut();
    }
    let Some(text) = path.to_str().filter(|text| !text.contains('\0')) else {
        return std::ptr::null_mut();
    };
    unsafe { msg_send![class!(NSURL), fileURLWithPath: nsstring(text) isDirectory: is_directory] }
}

pub(crate) fn path_from_url(url: *mut Object) -> Option<PathBuf> {
    if url.is_null() {
        return None;
    }

    let path: *mut Object = unsafe { msg_send![url, path] };
    nsstring_to_string(path).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_preserve_missing_paths_and_explicit_directory_hints() {
        objc::rc::autoreleasepool(|| unsafe {
            let path = Path::new("/Volumes/moeka-unavailable-url-test/missing # 文件.md");
            let url = file_url(path);
            assert!(!url.is_null());
            assert_eq!(path_from_url(url).as_deref(), Some(path));
            let is_directory: bool = msg_send![url, hasDirectoryPath];
            assert!(!is_directory);

            let url = file_url_with_directory(path, true);
            let is_directory: bool = msg_send![url, hasDirectoryPath];
            assert!(is_directory);
            let url = file_url(Path::new("/Volumes/moeka-unavailable-url-test/folder/"));
            let is_directory: bool = msg_send![url, hasDirectoryPath];
            assert!(is_directory);
        });
    }

    #[test]
    fn file_urls_reject_unrepresentable_paths() {
        use std::os::unix::ffi::OsStrExt;
        assert!(file_url(Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff"))).is_null());
        assert!(file_url(Path::new("/tmp/invalid\0path")).is_null());
        assert!(file_url(Path::new("relative/path")).is_null());
    }
}
