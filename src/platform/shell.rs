//! Opening files, folders and links through the host desktop environment.
//!
//! macOS goes through `NSWorkspace` so the app never forks a helper process;
//! other platforms use `opener`, which maps to `ShellExecuteW`/`explorer.exe
//! /select,` on Windows and `xdg-open`/D-Bus `FileManager1` on Linux.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::platform::fs_task;

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

/// Legacy synchronous open; UI callers should use `open_path_async`.
/// macOS requires an absolute path to avoid Foundation resolving the CWD.
pub(crate) fn open_path(path: &Path) -> Result<(), ShellError> {
    backend::open_path(path)
}

/// Legacy reveal; UI callers should use `reveal_async` for validation/errors.
/// macOS requires an absolute path to avoid Foundation resolving the CWD.
pub(crate) fn reveal(path: &Path) -> Result<(), ShellError> {
    backend::reveal(path)
}

/// Opens a local path without waiting for the filesystem on the calling thread.
pub(crate) async fn open_path_async(path: PathBuf) -> io::Result<()> {
    run_file_action(FileAction::Open, path).await
}

/// Validates and reveals a path without blocking the UI. Finder's reveal API
/// acknowledges dispatch, not eventual selection; it has no completion/error API.
pub(crate) async fn reveal_async(path: PathBuf) -> io::Result<()> {
    run_file_action(FileAction::Reveal, path).await
}

#[derive(Clone, Copy)]
enum FileAction {
    Open,
    Reveal,
}

fn prepare_file_action(path: &Path) -> io::Result<(PathBuf, bool)> {
    if path.as_os_str().is_empty() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid empty or NUL-containing path",
        ));
    }
    let path = std::path::absolute(path)?;
    let metadata = std::fs::metadata(&path)?;
    fs_task::check_deadline()?;
    Ok((path, metadata.is_dir()))
}

async fn run_file_action(action: FileAction, path: PathBuf) -> io::Result<()> {
    // Only this future owns the strong token. A cancelled/timed-out future must
    // not leave a native launch queued for much later on the main thread.
    #[cfg(target_os = "macos")]
    let lifetime = std::sync::Arc::new(());
    #[cfg(target_os = "macos")]
    let request = backend::NativeRequest::new(&lifetime);
    let operation = match action {
        FileAction::Open => "open file with system application",
        FileAction::Reveal => "reveal in file manager",
    };
    let result = fs_task::run(operation, path.clone(), move || {
        let (path, is_directory) = prepare_file_action(&path)?;
        #[cfg(target_os = "macos")]
        return backend::run_file_action(action, path, is_directory, request);
        #[cfg(not(target_os = "macos"))]
        backend::run_file_action(action, path, is_directory)
    })
    .await;
    #[cfg(target_os = "macos")]
    drop(lifetime);
    result
}

#[cfg(target_os = "macos")]
mod backend {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Weak, mpsc};
    use std::time::{Duration, Instant};

    use block::ConcreteBlock;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    use super::{FileAction, ShellError};
    use crate::platform::macos::{file_url, file_url_with_directory, nsstring, nsstring_to_string};

    pub(super) struct NativeRequest {
        lifetime: Weak<()>,
        deadline: Instant,
    }

    impl NativeRequest {
        pub(super) fn new(lifetime: &Arc<()>) -> Self {
            Self {
                lifetime: Arc::downgrade(lifetime),
                deadline: Instant::now() + Duration::from_secs(5),
            }
        }

        fn may_launch(&self) -> bool {
            self.lifetime.strong_count() > 0 && Instant::now() < self.deadline
        }
    }

    pub(super) fn run_file_action(
        action: FileAction,
        path: PathBuf,
        is_directory: bool,
        request: NativeRequest,
    ) -> io::Result<()> {
        if path.to_str().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                invalid_path(&path),
            ));
        }
        crate::platform::fs_task::check_deadline()?;
        let deadline = request.deadline;
        let (sender, receiver) = mpsc::sync_channel(1);
        objc::rc::autoreleasepool(|| unsafe {
            let launch = ConcreteBlock::new(move || {
                if !request.may_launch() {
                    let _ = sender.try_send(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native file action expired before launch",
                    )));
                    return;
                }
                // Passing the cached directory flag avoids NSURL's implicit stat.
                let target = file_url_with_directory(&path, is_directory);
                if target.is_null() {
                    let _ = sender.try_send(Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        invalid_path(&path),
                    )));
                    return;
                }
                match action {
                    FileAction::Open => {
                        let sender = sender.clone();
                        let completion =
                            ConcreteBlock::new(move |_: *mut Object, error: *mut Object| {
                                let result = if error.is_null() {
                                    Ok(())
                                } else {
                                    let description: *mut Object =
                                        msg_send![error, localizedDescription];
                                    Err(io::Error::other(
                                        nsstring_to_string(description).unwrap_or_else(|| {
                                            "the system application could not open the file".into()
                                        }),
                                    ))
                                };
                                let _ = sender.try_send(result);
                            })
                            .copy();
                        let configuration: *mut Object =
                            msg_send![class!(NSWorkspaceOpenConfiguration), configuration];
                        let _: () = msg_send![workspace(),
                            openURL: target
                            configuration: configuration
                            completionHandler: &*completion];
                    }
                    FileAction::Reveal => {
                        let urls: *mut Object = msg_send![class!(NSArray), arrayWithObject: target];
                        let _: () = msg_send![workspace(), activateFileViewerSelectingURLs: urls];
                        let _ = sender.try_send(Ok(()));
                    }
                }
            })
            .copy();
            let queue: *mut Object = msg_send![class!(NSOperationQueue), mainQueue];
            let _: () = msg_send![queue, addOperationWithBlock: &*launch];
        });
        receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the native file action did not respond within 5 s; it may already have launched",
                ),
                mpsc::RecvTimeoutError::Disconnected => io::Error::other("the native file action was interrupted"),
            })?
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_file_requests_do_not_launch_after_cancellation_or_deadline() {
            let lifetime = Arc::new(());
            let mut request = NativeRequest::new(&lifetime);
            assert!(request.may_launch());
            request.deadline = Instant::now();
            assert!(!request.may_launch());
            let request = NativeRequest::new(&lifetime);
            drop(lifetime);
            assert!(!request.may_launch());
        }
    }

    fn workspace() -> *mut Object {
        unsafe { msg_send![class!(NSWorkspace), sharedWorkspace] }
    }

    fn invalid_path(path: &Path) -> ShellError {
        ShellError::Open(format!("`{}` is not a valid path", path.display()))
    }

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
        if !path.is_absolute() || path.to_str().is_none_or(|text| text.contains('\0')) {
            return Err(invalid_path(path));
        }
        // Existing export callers already run on workers; AppKit still needs
        // its launch dispatched onto the main queue.
        let is_main: bool = unsafe { msg_send![class!(NSThread), isMainThread] };
        if !is_main {
            let lifetime = Arc::new(());
            return run_file_action(
                FileAction::Open,
                path.to_owned(),
                path.as_os_str().as_encoded_bytes().ends_with(b"/"),
                NativeRequest::new(&lifetime),
            )
            .map_err(|error| ShellError::Open(error.to_string()));
        }
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
        if !path.is_absolute() || path.to_str().is_none_or(|text| text.contains('\0')) {
            return Err(invalid_path(path));
        }
        let path = path.to_owned();
        unsafe {
            let reveal = ConcreteBlock::new(move || {
                let target = file_url(&path);
                if target.is_null() {
                    return;
                }
                let urls: *mut Object = msg_send![class!(NSArray), arrayWithObject: target];
                let _: () = msg_send![workspace(), activateFileViewerSelectingURLs: urls];
            })
            .copy();
            let queue: *mut Object = msg_send![class!(NSOperationQueue), mainQueue];
            let _: () = msg_send![queue, addOperationWithBlock: &*reveal];
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod backend {
    use std::io;
    use std::path::{Path, PathBuf};

    use opener::OpenError;

    use super::{FileAction, ShellError, is_web_url};

    // These entry points are only called by the bounded filesystem worker, so
    // ShellExecute cannot re-enter GPUI and no UI thread waits on a thread join.
    pub(super) fn run_file_action(
        action: FileAction,
        path: PathBuf,
        _is_directory: bool,
    ) -> io::Result<()> {
        crate::platform::fs_task::check_deadline()?;
        match action {
            FileAction::Open => opener::open(path).map_err(io::Error::other),
            FileAction::Reveal => {
                #[cfg(target_os = "windows")]
                {
                    let mut argument = std::ffi::OsString::from("/select,");
                    argument.push(path.as_os_str());
                    std::process::Command::new("explorer.exe")
                        .arg(argument)
                        .spawn()?;
                    Ok(())
                }
                #[cfg(not(target_os = "windows"))]
                opener::reveal(path).map_err(io::Error::other)
            }
        }
    }

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
        #[cfg(target_os = "windows")]
        {
            use std::ffi::OsString;
            use std::process::Command;

            let mut select_argument = OsString::from("/select,");
            select_argument.push(path.as_os_str());
            return match std::thread::spawn(move || {
                Command::new("explorer.exe").arg(select_argument).spawn()
            })
            .join()
            {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(error)) => Err(ShellError::Open(error.to_string())),
                Err(_) => Err(ShellError::Open("the system handler crashed".to_owned())),
            };
        }

        #[cfg(not(target_os = "windows"))]
        {
            let path = path.to_owned();
            away_from_ui(move || opener::reveal(path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::time::Duration;

    struct TestWaker(std::thread::Thread);
    impl Wake for TestWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                return result;
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }

    #[test]
    fn async_file_actions_preserve_missing_path_errors_without_launching() {
        let dir = tempfile::tempdir().unwrap();
        fs_task::register_workspace_root(dir.path());
        let path = dir.path().join("missing-attachment.bin");
        for result in [
            block_on(open_path_async(path.clone())),
            block_on(reveal_async(path.clone())),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
            assert!(error.to_string().contains("missing-attachment.bin"));
        }
    }

    #[test]
    fn file_action_preparation_uses_worker_metadata_and_rejects_invalid_paths() {
        assert_eq!(
            prepare_file_action(Path::new("")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            prepare_file_action(Path::new("invalid\0path"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            prepare_file_action(dir.path()).unwrap(),
            (dir.path().to_owned(), true)
        );
        let file = dir.path().join("binary.dat");
        std::fs::write(&file, b"binary").unwrap();
        assert_eq!(prepare_file_action(&file).unwrap(), (file, false));
    }

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
