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

/// A `file://` NSURL for `path`, or null when the path is not representable.
pub(crate) fn file_url(path: &Path) -> *mut Object {
    let Some(text) = path.to_str() else {
        return std::ptr::null_mut();
    };
    unsafe { msg_send![class!(NSURL), fileURLWithPath: nsstring(text)] }
}

fn volume_resource_bool(path: &Path, key_name: &str) -> Option<bool> {
    let url = file_url(path);
    if url.is_null() {
        return None;
    }

    unsafe {
        let key = nsstring(key_name);
        let mut value: *mut Object = std::ptr::null_mut();
        let mut error: *mut Object = std::ptr::null_mut();
        let resolved: bool = msg_send![url,
            getResourceValue: &mut value
            forKey: key
            error: &mut error
        ];
        if !resolved || value.is_null() {
            return None;
        }
        Some(msg_send![value, boolValue])
    }
}

pub(crate) fn path_is_on_network_volume(path: &Path) -> bool {
    volume_resource_bool(path, "NSURLVolumeIsLocalKey") == Some(false)
        && volume_resource_bool(path, "NSURLVolumeIsRemovableKey") != Some(true)
}

pub(crate) fn path_from_url(url: *mut Object) -> Option<PathBuf> {
    if url.is_null() {
        return None;
    }

    let path: *mut Object = unsafe { msg_send![url, path] };
    nsstring_to_string(path).map(PathBuf::from)
}
