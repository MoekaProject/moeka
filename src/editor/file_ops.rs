use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

struct SaveState {
    latest: AtomicU64,
    write_lock: Mutex<()>,
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

    pub(crate) fn new(path: PathBuf) -> Self {
        static SAVES: OnceLock<Mutex<std::collections::HashMap<PathBuf, Weak<SaveState>>>> =
            OnceLock::new();
        let saves = SAVES.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
        let mut saves = saves
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        saves.retain(|_, state| state.strong_count() > 0);
        let state = saves.get(&path).and_then(Weak::upgrade).unwrap_or_else(|| {
            let state = Arc::new(SaveState {
                latest: AtomicU64::new(0),
                write_lock: Mutex::new(()),
            });
            saves.insert(path.clone(), Arc::downgrade(&state));
            state
        });
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

    fn write_with(
        self,
        content: &[u8],
        write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
    ) -> io::Result<bool> {
        let _guard = self
            .state
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.is_latest() {
            return Ok(false);
        }
        write(&self.path, content)?;
        Ok(true)
    }
}

fn save_document(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(content)?;
    file.flush()
}

pub(crate) fn copy_file_data(source: &Path, target: &Path, overwrite: bool) -> io::Result<u64> {
    let mut input = fs::File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(overwrite)
        .create_new(!overwrite)
        .open(target)?;
    let copied = io::copy(&mut input, &mut output)?;
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
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return atomic_write(&fs::canonicalize(path)?, content);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    atomic_write_with_tempfile(path, content)
}

fn atomic_write_with_tempfile(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::Builder::new()
        .prefix(".moeka-save-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    if let Ok(metadata) = fs::metadata(path) {
        let _ = file.as_file().set_permissions(metadata.permissions());
    }
    file.write_all(content)?;
    sync_file_if_supported(file.as_file())?;
    match file.persist(path) {
        Ok(_) => Ok(()),
        Err(atomic_error) => {
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
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(content)?;
    file.set_len(content.len() as u64)?;
    sync_file_if_supported(&file)
}

fn write_without_atomic_replace(path: &Path, content: &[u8]) -> io::Result<()> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let result = file
                .write_all(content)
                .and_then(|()| sync_file_if_supported(&file));
            drop(file);
            if result.is_err() {
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
    fn document_saves_to_the_same_path_wait_for_inflight_writes() {
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
            let second = DocumentSave::new(path.clone());
            let second_write = scope.spawn(|| second.write(b"second"));
            finish_tx.send(()).unwrap();
            assert!(first_write.join().unwrap().unwrap());
            assert!(second_write.join().unwrap().unwrap());
        });
        assert_eq!(fs::read(path).unwrap(), b"second");
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
