use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};

use crate::platform::fs_task::check_deadline;

struct SaveState {
    latest: AtomicU64,
    write_lock: Mutex<()>,
    auto_save_paused: AtomicBool,
}

fn document_saves() -> &'static Mutex<std::collections::HashMap<PathBuf, Arc<SaveState>>> {
    static SAVES: OnceLock<Mutex<std::collections::HashMap<PathBuf, Arc<SaveState>>>> =
        OnceLock::new();
    SAVES.get_or_init(Default::default)
}

#[derive(Clone)]
pub(crate) struct DocumentSave {
    state: Arc<SaveState>,
    version: u64,
    path: PathBuf,
}

impl DocumentSave {
    pub(crate) fn is_latest(&self) -> bool {
        self.state.latest.load(Ordering::SeqCst) == self.version
    }

    /// Acknowledge only the awaited runner result, never a late worker result.
    pub(crate) fn observe_result(&self, result: &io::Result<bool>) {
        if !matches!(result, Ok(true)) || !self.is_latest() {
            return;
        }
        let _guard = match self.state.write_lock.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        if self.is_latest() {
            self.state.auto_save_paused.store(false, Ordering::SeqCst);
        }
    }

    pub(crate) fn auto_save_allowed(path: &Path) -> bool {
        document_saves()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
            .is_none_or(|state| !state.auto_save_paused.load(Ordering::SeqCst))
    }

    pub(crate) fn new(path: PathBuf) -> Self {
        let mut saves = document_saves()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Retain uncertain outcomes after the worker/token exits: another edit
        // must not turn a timed-out save into an automatic retry.
        saves.retain(|_, state| {
            Arc::strong_count(state) > 1 || state.auto_save_paused.load(Ordering::SeqCst)
        });
        let state = saves
            .entry(path.clone())
            .or_insert_with(|| {
                Arc::new(SaveState {
                    latest: AtomicU64::new(0),
                    write_lock: Mutex::new(()),
                    auto_save_paused: AtomicBool::new(false),
                })
            })
            .clone();
        let version = state.latest.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        Self {
            state,
            version,
            path,
        }
    }

    /// False means a newer request took precedence before this one could write.
    pub(crate) fn write(self, content: &[u8]) -> io::Result<bool> {
        self.write_with(content, save_document)
    }

    /// Reserve before dispatch so new edits cannot cancel an in-flight waiter.
    pub(crate) fn reserve_auto_save(&self) -> io::Result<()> {
        if !self.is_latest()
            || self
                .state
                .auto_save_paused
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "automatic saves to {} are paused until an explicit save succeeds; a previous write may be incomplete",
                    self.path.display()
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn write_with(
        self,
        content: &[u8],
        write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
    ) -> io::Result<bool> {
        if !self.is_latest() {
            return Ok(false);
        }
        let _guard = match self.state.write_lock.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!(
                        "a previous save to {} is still running; its outcome must be resolved before saving again",
                        self.path.display()
                    ),
                ));
            }
        };
        if !self.is_latest() {
            return Ok(false);
        }
        check_deadline()?;
        // Cancellation or an I/O error can hide a partially completed write.
        // Only an acknowledged success re-enables automatic writes to this path.
        self.state.auto_save_paused.store(true, Ordering::SeqCst);
        write(&self.path, content)?;
        check_deadline()?;
        // The caller must acknowledge the runner result; a worker finishing
        // after its caller timed out or was cancelled is not a confirmed save.
        Ok(true)
    }
}

fn save_document(path: &Path, content: &[u8]) -> io::Result<()> {
    check_deadline()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    check_deadline()?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    check_deadline()?;
    file.write_all(content)?;
    check_deadline()?;
    file.flush()
}

pub(crate) fn copy_file_data(source: &Path, target: &Path, overwrite: bool) -> io::Result<u64> {
    check_deadline()?;
    let mut input = fs::File::open(source)?;
    check_deadline()?;
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(overwrite)
        .create_new(!overwrite)
        .open(target)?;
    check_deadline()?;
    let copied = io::copy(&mut input, &mut output)?;
    check_deadline()?;
    output.flush()?;
    Ok(copied)
}

pub(crate) fn is_atomic_write_temp_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with(".moeka-save-") && name.ends_with(".tmp")
}

pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    check_deadline()?;
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return atomic_write(&fs::canonicalize(path)?, content);
    }
    check_deadline()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    atomic_write_with_tempfile(path, content)
}

fn atomic_write_with_tempfile(path: &Path, content: &[u8]) -> io::Result<()> {
    check_deadline()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::Builder::new()
        .prefix(".moeka-save-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    check_deadline()?;
    if let Ok(metadata) = fs::metadata(path) {
        check_deadline()?;
        let _ = file.as_file().set_permissions(metadata.permissions());
    }
    check_deadline()?;
    file.write_all(content)?;
    sync_file_if_supported(file.as_file())?;
    check_deadline()?;
    match file.persist(path) {
        Ok(_) => Ok(()),
        Err(atomic_error) => {
            check_deadline()?;
            if matches!(
                atomic_error.error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) {
                return Err(atomic_error.error);
            }
            write_without_atomic_replace(path, content).map_err(|fallback_error| {
                io::Error::new(
                    fallback_error.kind(),
                    format!(
                        "atomic write failed ({}); direct-write fallback failed ({fallback_error})",
                        atomic_error.error
                    ),
                )
            })
        }
    }
}

fn write_existing_file_in_place(path: &Path, content: &[u8]) -> io::Result<()> {
    check_deadline()?;
    let mut file = OpenOptions::new().write(true).open(path)?;
    check_deadline()?;
    file.write_all(content)?;
    check_deadline()?;
    file.set_len(content.len() as u64)?;
    sync_file_if_supported(&file)
}

fn write_without_atomic_replace(path: &Path, content: &[u8]) -> io::Result<()> {
    check_deadline()?;
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            check_deadline()?;
            let result = file
                .write_all(content)
                .and_then(|()| sync_file_if_supported(&file));
            drop(file);
            if result.as_ref().is_err_and(|error| {
                !matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                )
            }) && check_deadline().is_ok()
            {
                let _ = fs::remove_file(path);
            }
            result
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_file() => {
            write_existing_file_in_place(path, content)
        }
        Err(error) => Err(error),
    }
}

fn sync_file_if_supported(file: &fs::File) -> io::Result<()> {
    check_deadline()?;
    accept_unsupported_sync(file.sync_all())
}

fn accept_unsupported_sync(result: io::Result<()>) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error)
            if error.kind() == io::ErrorKind::Unsupported || error.raw_os_error() == Some(45) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn content_hash(content: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in content {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        task::{Context, Poll, Wake, Waker},
        time::Duration,
    };

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
    fn recognizes_only_atomic_write_temp_paths() {
        assert!(is_atomic_write_temp_path(Path::new(
            "/vault/.moeka-save-abc123.tmp"
        )));
        assert!(!is_atomic_write_temp_path(Path::new("/vault/.note.md.tmp")));
        assert!(!is_atomic_write_temp_path(Path::new(
            "/vault/note.md.123.4.tmp"
        )));
    }

    use tempfile::tempdir;

    #[test]
    fn superseded_document_save_never_overwrites_the_latest_request() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        let old = DocumentSave::new(path.clone());
        let latest = DocumentSave::new(path.clone());
        assert!(!old.write(b"old").unwrap());
        assert!(latest.write(b"latest").unwrap());
        assert_eq!(fs::read(path).unwrap(), b"latest");
    }

    #[test]
    fn document_saves_to_the_same_path_do_not_wait_for_inflight_writes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        let first = DocumentSave::new(path.clone());
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let first_write = scope.spawn(move || {
                first.write_with(b"first", move |path, content| {
                    started_tx.send(()).unwrap();
                    finish_rx.recv().unwrap();
                    save_document(path, content)
                })
            });
            started_rx.recv().unwrap();
            let stale = DocumentSave::new(path.clone());
            let second = DocumentSave::new(path.clone());
            assert!(!stale.write(b"stale").unwrap());
            assert_eq!(
                second.write(b"second").unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            finish_tx.send(()).unwrap();
            assert!(first_write.join().unwrap().unwrap());
        });
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert!(DocumentSave::new(path.clone()).write(b"second").unwrap());
        assert_eq!(fs::read(path).unwrap(), b"second");
    }

    #[test]
    fn a_busy_document_save_does_not_block_an_unrelated_path() {
        let dir = tempdir().unwrap();
        let busy_path = dir.path().join("busy.md");
        let other_path = dir.path().join("other.md");
        let busy = DocumentSave::new(busy_path.clone());
        let held = busy.state.write_lock.lock().unwrap();
        let newer = DocumentSave::new(busy_path);
        assert_eq!(
            newer.write(b"busy").unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(!busy.is_latest());
        assert!(
            DocumentSave::new(other_path.clone())
                .write(b"other")
                .unwrap()
        );
        assert_eq!(fs::read(other_path).unwrap(), b"other");
        drop(held);
    }

    #[test]
    fn failed_save_does_not_report_success_or_invoke_an_older_request() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        let older = DocumentSave::new(path.clone());
        let failed = DocumentSave::new(path);
        assert_eq!(
            failed
                .write_with(b"new", |_, _| Err(io::ErrorKind::TimedOut.into()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(
            !older
                .write_with(b"old", |_, _| panic!("obsolete save ran"))
                .unwrap()
        );
    }

    #[test]
    fn auto_save_is_reserved_before_dispatch_and_cancellation_keeps_it_paused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        let save = DocumentSave::new(path.clone());
        save.reserve_auto_save().unwrap();
        assert!(!DocumentSave::auto_save_allowed(&path));
        drop(save);
        assert_eq!(
            DocumentSave::new(path.clone())
                .reserve_auto_save()
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock,
        );
        assert!(!path.exists());
        let explicit = DocumentSave::new(path.clone());
        let token = explicit.clone();
        let result = explicit.write(b"confirmed");
        token.observe_result(&result);
        assert!(matches!(result, Ok(true)));
        assert!(DocumentSave::auto_save_allowed(&path));
    }

    #[test]
    fn failed_save_pauses_auto_save_until_an_explicit_success_is_acknowledged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "original").unwrap();
        assert!(DocumentSave::auto_save_allowed(&path));
        let failed = DocumentSave::new(path.clone());
        let token = failed.clone();
        let result = failed.write_with(b"uncertain", |_, _| Err(io::ErrorKind::TimedOut.into()));
        token.observe_result(&result);
        drop(token);
        assert!(!DocumentSave::auto_save_allowed(&path));
        assert_eq!(
            DocumentSave::new(path.clone())
                .reserve_auto_save()
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock,
        );
        assert_eq!(fs::read(&path).unwrap(), b"original");

        let explicit = DocumentSave::new(path.clone());
        let token = explicit.clone();
        let result = explicit.write(b"explicit");
        assert!(matches!(result, Ok(true)));
        assert!(!DocumentSave::auto_save_allowed(&path));
        token.observe_result(&result);
        assert!(DocumentSave::auto_save_allowed(&path));
        assert_eq!(fs::read(path).unwrap(), b"explicit");
    }

    #[test]
    fn stale_success_does_not_resume_auto_save_after_a_newer_busy_request() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        let save = DocumentSave::new(path.clone());
        let token = save.clone();
        let result = save.write_with(b"first", |path, text| {
            let newer = DocumentSave::new(path.to_path_buf());
            assert_eq!(
                newer.write(b"newer").unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            save_document(path, text)
        });
        assert!(matches!(result, Ok(true)));
        assert!(!token.is_latest());
        token.observe_result(&result);
        assert!(!DocumentSave::auto_save_allowed(&path));
        assert_eq!(fs::read(path).unwrap(), b"first");
    }

    #[test]
    fn runner_timeout_does_not_confirm_a_late_write_or_allow_auto_retry() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        crate::platform::fs_task::register_workspace_root(dir.path());
        let save = DocumentSave::new(path.clone());
        let token = save.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let result = block_on(crate::platform::fs_task::run_with_timeout(
            "test late save",
            path.clone(),
            Duration::from_millis(100),
            move || {
                let result = save.write_with(b"late", |path, content| {
                    wait.recv_timeout(Duration::from_secs(2)).unwrap();
                    // Model a syscall that cannot be interrupted by the timeout.
                    fs::write(path, content)
                });
                finished
                    .send(result.as_ref().err().map(io::Error::kind))
                    .unwrap();
                result
            },
        ));
        assert_eq!(result.as_ref().unwrap_err().kind(), io::ErrorKind::TimedOut);
        token.observe_result(&result);
        assert!(!DocumentSave::auto_save_allowed(&path));
        assert_eq!(
            DocumentSave::new(path.clone())
                .reserve_auto_save()
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock,
        );
        release.send(()).unwrap();
        assert_eq!(
            done.recv_timeout(Duration::from_secs(2)).unwrap(),
            Some(io::ErrorKind::TimedOut)
        );
        assert_eq!(fs::read(&path).unwrap(), b"late");
        assert!(!DocumentSave::auto_save_allowed(&path));
        let explicit = DocumentSave::new(path.clone());
        let token = explicit.clone();
        let result = explicit.write(b"confirmed");
        token.observe_result(&result);
        assert!(matches!(result, Ok(true)));
        assert!(DocumentSave::auto_save_allowed(&path));
    }

    #[test]
    fn copy_file_data_copies_only_content_and_respects_existing_destinations() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source.md");
        let target = dir.path().join("target.md");
        fs::write(&source, "copy me").unwrap();
        assert_eq!(copy_file_data(&source, &target, false).unwrap(), 7);
        assert_eq!(fs::read_to_string(&target).unwrap(), "copy me");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert!(copy_file_data(&source, &target, false).is_err());
        fs::write(&source, "new").unwrap();
        assert_eq!(copy_file_data(&source, &target, true).unwrap(), 3);
        assert_eq!(fs::read_to_string(target).unwrap(), "new");
    }

    #[test]
    fn creates_parent_directories_and_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("note.md");

        atomic_write(&path, b"hello").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn atomically_replaces_existing_contents_without_leaving_temp_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "before").unwrap();

        atomic_write(&path, b"after").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "after");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn direct_write_fallback_replaces_existing_contents() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("network-note.md");
        fs::write(&path, "old content that is longer").unwrap();

        write_without_atomic_replace(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn direct_write_fallback_creates_new_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("new-network-note.md");

        write_without_atomic_replace(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn accepts_unsupported_sync_but_preserves_other_errors() {
        assert!(accept_unsupported_sync(Err(io::Error::from_raw_os_error(45))).is_ok());

        let permission_error = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(
            accept_unsupported_sync(Err(permission_error))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn failed_replacement_keeps_target_and_cleans_temporary_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("target");
        fs::create_dir(&path).unwrap();

        assert!(atomic_write(&path, b"content").is_err());

        assert!(path.is_dir());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("script");
        fs::write(&path, "before").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o744)).unwrap();

        atomic_write(&path, b"after").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o744
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlink_without_replacing_it() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let target = dir.path().join("target.md");
        let link = dir.path().join("link.md");
        fs::write(&target, "before").unwrap();
        symlink(&target, &link).unwrap();

        atomic_write(&link, b"after").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "after");
    }
}
