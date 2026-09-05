//! Opening files, folders and links through the host desktop environment.
//!
//! macOS goes through `NSWorkspace` so the app never forks a helper process;
//! other platforms use `opener`, which maps to `ShellExecuteW`/`explorer.exe
//! /select,` on Windows and `xdg-open`/D-Bus `FileManager1` on Linux.

use std::error::Error;
use std::fmt;
use std::path::Path;

/// Schemes whose handler would execute document-controlled code.
const BLOCKED_SCHEMES: [&str; 2] = ["javascript", "data"];

#[derive(Debug)]
pub(crate) enum ShellError {
    BlockedScheme(String),
    Open(String),
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlockedScheme(scheme) => write!(f, "refused to open unsafe `{scheme}:` link"),
            Self::Open(message) => write!(f, "{message}"),
        }
    }
}

impl Error for ShellError {}

/// Returns the URL scheme with whitespace and control characters stripped, so
/// that obfuscations like `java\tscript:` cannot slip past the block list.
fn normalized_scheme(url: &str) -> Option<String> {
    let (scheme, _) = url.split_once(':')?;
    // A real scheme never contains these; anything else is a relative path.
    if scheme.contains(['/', '?', '#']) {
        return None;
    }

    let normalized: String = scheme
        .chars()
        .filter(|character| !character.is_whitespace() && !character.is_control())
        .collect();
    (!normalized.is_empty()).then_some(normalized)
}

/// Returns the scheme when it is one we refuse to hand to the system.
fn blocked_scheme(url: &str) -> Option<String> {
    normalized_scheme(url).filter(|scheme| {
        BLOCKED_SCHEMES
            .iter()
            .any(|blocked| scheme.eq_ignore_ascii_case(blocked))
    })
}

#[cfg(not(target_os = "macos"))]
fn is_web_url(url: &str) -> bool {
    normalized_scheme(url).is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// Opens a link with the system handler for its scheme.
pub(crate) fn open_url(url: &str) -> Result<(), ShellError> {
    if let Some(scheme) = blocked_scheme(url) {
        return Err(ShellError::BlockedScheme(scheme));
    }

    backend::open_url(url)
}

/// Opens a file or folder with its default application.
pub(crate) fn open_path(path: &Path) -> Result<(), ShellError> {
    backend::open_path(path)
}

/// Shows a path selected inside the system file manager.
pub(crate) fn reveal(path: &Path) -> Result<(), ShellError> {
    backend::reveal(path)
}

#[cfg(target_os = "macos")]
mod backend {
    use std::path::Path;

    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    use super::ShellError;
    use crate::platform::macos::{file_url, nsstring};

    fn workspace() -> *mut Object {
        unsafe { msg_send![class!(NSWorkspace), sharedWorkspace] }
    }

    fn invalid_path(path: &Path) -> ShellError {
        ShellError::Open(format!("`{}` is not a valid path", path.display()))
    }

    // `NSWorkspace` hands the request to LaunchServices and returns, so these
    // run on the caller's thread without the re-entrancy hazard that made the
    // Windows path need a helper thread.
    pub(super) fn open_url(url: &str) -> Result<(), ShellError> {
        unsafe {
            let target: *mut Object = msg_send![class!(NSURL), URLWithString: nsstring(url)];
            if target.is_null() {
                return Err(ShellError::Open(format!("`{url}` is not a valid URL")));
            }

            let opened: bool = msg_send![workspace(), openURL: target];
            opened
                .then_some(())
                .ok_or_else(|| ShellError::Open(format!("no application can open `{url}`")))
        }
    }

    pub(super) fn open_path(path: &Path) -> Result<(), ShellError> {
        unsafe {
            let target = file_url(path);
            if target.is_null() {
                return Err(invalid_path(path));
            }

            let opened: bool = msg_send![workspace(), openURL: target];
            opened.then_some(()).ok_or_else(|| {
                ShellError::Open(format!("no application can open `{}`", path.display()))
            })
        }
    }

    pub(super) fn reveal(path: &Path) -> Result<(), ShellError> {
        let path = path.to_owned();
        if file_url(&path).is_null() {
            return Err(invalid_path(&path));
        }

        unsafe {
            let operation = block::ConcreteBlock::new(move || {
                let target = file_url(&path);
                if target.is_null() {
                    return;
                }
                let urls: *mut Object = msg_send![class!(NSArray), arrayWithObject: target];
                let _: () = msg_send![workspace(), activateFileViewerSelectingURLs: urls];
            })
            .copy();
            let queue: *mut Object = msg_send![class!(NSOperationQueue), mainQueue];
            let _: () = msg_send![queue, addOperationWithBlock: &*operation];
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod backend {
    use std::path::Path;

    use opener::OpenError;

    use super::{ShellError, is_web_url};

    /// Runs a blocking shell call on a thread of its own.
    ///
    /// `ShellExecuteW` pumps the calling thread's message queue while it works.
    /// On the UI thread that re-enters the window procedure in the middle of a
    /// gpui update, and `App` is a `RefCell`, so the second borrow aborts the
    /// process.
    fn away_from_ui(
        call: impl FnOnce() -> Result<(), OpenError> + Send + 'static,
    ) -> Result<(), ShellError> {
        match std::thread::spawn(call).join() {
            Ok(result) => result.map_err(|error| ShellError::Open(error.to_string())),
            Err(_) => Err(ShellError::Open("the system handler crashed".to_owned())),
        }
    }

    pub(super) fn open_url(url: &str) -> Result<(), ShellError> {
        // `$BROWSER` should only override real web links, not custom app schemes.
        let web = is_web_url(url);
        let url = url.to_owned();
        away_from_ui(move || {
            if web {
                opener::open_browser(url)
            } else {
                opener::open(url)
            }
        })
    }

    pub(super) fn open_path(path: &Path) -> Result<(), ShellError> {
        let path = path.to_owned();
        away_from_ui(move || opener::open(path))
    }

    pub(super) fn reveal(path: &Path) -> Result<(), ShellError> {
        let path = path.to_owned();
        away_from_ui(move || opener::reveal(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_blocked_url(url: &str) -> bool {
        blocked_scheme(url).is_some()
    }

    #[test]
    fn blocks_code_executing_schemes_regardless_of_obfuscation() {
        assert!(is_blocked_url("javascript:alert(1)"));
        assert!(is_blocked_url("JavaScript:alert(1)"));
        assert!(is_blocked_url("  javascript:alert(1)"));
        assert!(is_blocked_url("java\tscript:alert(1)"));
        assert!(is_blocked_url("java\nscript:alert(1)"));
        assert!(is_blocked_url("data:text/html,<script>"));
        assert!(is_blocked_url("DATA:text/html,<script>"));
    }

    #[test]
    fn allows_web_and_custom_application_schemes() {
        assert!(!is_blocked_url("https://moeka.app/"));
        assert!(!is_blocked_url("http://example.com"));
        assert!(!is_blocked_url("mailto:hi@example.com"));
        assert!(!is_blocked_url("obsidian://open?vault=notes"));
        assert!(!is_blocked_url("file:///Users/me/note.md"));
    }

    #[test]
    fn relative_references_are_not_treated_as_schemes() {
        assert!(!is_blocked_url("./notes/javascript:file.md"));
        assert!(!is_blocked_url("notes/a?javascript:b"));
        assert!(!is_blocked_url("#javascript:anchor"));
        assert!(!is_blocked_url("plain-text-without-colon"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn only_http_and_https_honor_the_browser_override() {
        assert!(is_web_url("https://moeka.app/"));
        assert!(is_web_url("HTTP://example.com"));
        assert!(!is_web_url("mailto:hi@example.com"));
        assert!(!is_web_url("obsidian://open"));
    }
}
