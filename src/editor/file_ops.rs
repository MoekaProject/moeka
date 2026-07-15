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
    let temp_path = unique_temp_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&temp_path, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
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
