//! Security-scoped bookmarks: re-acquiring access to user-picked folders.
//!
//! Inside the App Store sandbox the powerbox grants access to a folder only
//! for the current launch. A bookmark captured while access is live can be
//! resolved on the next launch to get it back. Outside the sandbox the same
//! calls succeed and are simply redundant, so both channels take one path.

#[cfg(target_os = "macos")]
pub(crate) use macos_impl::{bookmark_for, start_access, stop_access};
#[cfg(not(target_os = "macos"))]
pub(crate) use stub_impl::{bookmark_for, start_access, stop_access};

/// Bookmarks are a macOS sandbox concept; elsewhere plain paths keep working.
#[cfg(not(target_os = "macos"))]
mod stub_impl {
    use std::path::{Path, PathBuf};

    pub(crate) fn bookmark_for(_path: &Path) -> Option<String> {
        None
    }

    pub(crate) fn start_access(_bookmark: &str) -> Option<PathBuf> {
        None
    }

    pub(crate) fn stop_access(_path: &Path) {}
}

#[cfg(target_os = "macos")]
mod macos_impl {

    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    use crate::platform::macos::{file_url_with_directory, path_from_url};
    const CREATE_WITH_SECURITY_SCOPE: usize = 1 << 11;
    const RESOLVE_WITHOUT_UI: usize = 1 << 8;
    const RESOLVE_WITHOUT_MOUNTING: usize = 1 << 9;
    const RESOLVE_WITH_SECURITY_SCOPE: usize = 1 << 10;

    /// A retained `NSURL` whose security scope is currently open.
    struct ScopedUrl(*mut Object);

    // The pointer is only ever handed back to `stopAccessingSecurityScopedResource`
    // and `release`, both of which are thread-safe on NSURL.
    unsafe impl Send for ScopedUrl {}

    impl Drop for ScopedUrl {
        fn drop(&mut self) {
            unsafe {
                let _: () = msg_send![self.0, stopAccessingSecurityScopedResource];
                let _: () = msg_send![self.0, release];
            }
        }
    }

    fn open_scopes() -> &'static Mutex<HashMap<PathBuf, ScopedUrl>> {
        static SCOPES: OnceLock<Mutex<HashMap<PathBuf, ScopedUrl>>> = OnceLock::new();
        SCOPES.get_or_init(Mutex::default)
    }

    /// Captures the current access to `path` as a base64 bookmark blob.
    pub(crate) fn bookmark_for(path: &Path) -> Option<String> {
        unsafe {
            let url = file_url_with_directory(path, true);
            if url.is_null() {
                return None;
            }

            let mut error: *mut std::ffi::c_void = std::ptr::null_mut();
            let data: *mut Object = msg_send![url,
            bookmarkDataWithOptions: CREATE_WITH_SECURITY_SCOPE
            includingResourceValuesForKeys: std::ptr::null_mut::<Object>()
            relativeToURL: std::ptr::null_mut::<Object>()
            error: &mut error];
            if data.is_null() {
                return None;
            }

            let bytes: *const u8 = msg_send![data, bytes];
            let length: usize = msg_send![data, length];
            if bytes.is_null() || length == 0 {
                return None;
            }

            Some(BASE64.encode(std::slice::from_raw_parts(bytes, length)))
        }
    }

    /// Resolves `bookmark` and holds its scope open until [`stop_access`].
    ///
    /// Returns the folder's current location, which differs from the path stored
    /// alongside the bookmark when the user moved or renamed it.
    pub(crate) fn start_access(bookmark: &str) -> Option<PathBuf> {
        let blob = BASE64.decode(bookmark).ok()?;

        let (path, url) = unsafe {
            let data: *mut Object = msg_send![class!(NSData),
            dataWithBytes: blob.as_ptr()
            length: blob.len()];
            if data.is_null() {
                return None;
            }

            let mut stale: bool = false;
            let mut error: *mut std::ffi::c_void = std::ptr::null_mut();
            let url: *mut Object = msg_send![class!(NSURL),
            URLByResolvingBookmarkData: data
            options: RESOLVE_WITH_SECURITY_SCOPE | RESOLVE_WITHOUT_UI | RESOLVE_WITHOUT_MOUNTING
            relativeToURL: std::ptr::null_mut::<Object>()
            bookmarkDataIsStale: &mut stale
            error: &mut error];
            if url.is_null() {
                return None;
            }

            let started: bool = msg_send![url, startAccessingSecurityScopedResource];
            if !started {
                return None;
            }

            let _: *mut Object = msg_send![url, retain];
            let Some(path) = path_from_url(url) else {
                let _: () = msg_send![url, stopAccessingSecurityScopedResource];
                let _: () = msg_send![url, release];
                return None;
            };
            (path, ScopedUrl(url))
        };

        open_scopes()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.clone(), url);
        Some(path)
    }

    /// Releases the scope opened for `path`, if any.
    pub(crate) fn stop_access(path: &Path) {
        open_scopes()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(path);
    }
}
