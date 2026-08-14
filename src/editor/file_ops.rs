use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn is_atomic_write_temp_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = name
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    let mut parts = stem.rsplitn(3, '.');
    let Some(id) = parts.next() else {
        return false;
    };
    let Some(process_id) = parts.next() else {
        return false;
    };
    parts.next().is_some() && id.parse::<u64>().is_ok() && process_id.parse::<u32>().is_ok()
}

pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return atomic_write(&fs::canonicalize(path)?, content);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    #[cfg(target_os = "windows")]
    {
        return fs::write(path, content);
    }

    #[cfg(not(target_os = "windows"))]
    atomic_write_by_rename(path, content)
}

#[cfg(not(target_os = "windows"))]
fn atomic_write_by_rename(path: &Path, content: &[u8]) -> io::Result<()> {
    atomic_write_by_rename_with(path, content, |from, to| fs::rename(from, to))
}

#[cfg(not(target_os = "windows"))]
fn atomic_write_by_rename_with(
    path: &Path,
    content: &[u8],
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let temp_path = unique_temp_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        if let Ok(metadata) = fs::metadata(path) {
            let _ = file.set_permissions(metadata.permissions());
        }
        file.write_all(content)?;
        sync_file_if_supported(&file)?;
        drop(file);

        rename(&temp_path, path)
    })();

    let result = result.or_else(|atomic_error| {
        write_without_atomic_replace(path, content).map_err(|fallback_error| {
            io::Error::new(
                fallback_error.kind(),
                format!(
                    "atomic write failed ({atomic_error}); direct-write fallback failed ({fallback_error})"
                ),
            )
        })
    });

    let _ = fs::remove_file(&temp_path);
    result
}

#[cfg(not(target_os = "windows"))]
fn write_existing_file_in_place(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(content)?;
    file.set_len(content.len() as u64)?;
    sync_file_if_supported(&file)
}

#[cfg(not(target_os = "windows"))]
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

#[cfg(not(target_os = "windows"))]
fn sync_file_if_supported(file: &fs::File) -> io::Result<()> {
    accept_unsupported_sync(file.sync_all())
}

#[cfg(not(target_os = "windows"))]
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

fn unique_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_atomic_write_temp_paths() {
        assert!(is_atomic_write_temp_path(Path::new(
            "/vault/.note.md.123.4.tmp"
        )));
        assert!(!is_atomic_write_temp_path(Path::new("/vault/.note.md.tmp")));
        assert!(!is_atomic_write_temp_path(Path::new(
            "/vault/note.md.123.4.tmp"
        )));
    }

    use tempfile::tempdir;

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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn falls_back_to_in_place_write_when_replace_is_unsupported() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("network-note.md");
        fs::write(&path, "old content that is longer").unwrap();

        atomic_write_by_rename_with(&path, b"new", |_, _| {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "network filesystem does not support atomic replace",
            ))
        })
        .unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn creates_final_file_directly_when_new_file_rename_is_unsupported() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("new-network-note.md");

        atomic_write_by_rename_with(&path, b"new", |_, _| {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "network filesystem does not support rename",
            ))
        })
        .unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(not(target_os = "windows"))]
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
