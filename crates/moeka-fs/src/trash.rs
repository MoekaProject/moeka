use std::io;
use std::path::{Path, PathBuf};

use crate::local::LocalFilesystem;

#[derive(Debug)]
struct UnsafeTarget;

impl std::fmt::Display for UnsafeTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("unsafe deletion target")
    }
}

impl std::error::Error for UnsafeTarget {}

fn unsafe_target() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, UnsafeTarget)
}

pub fn is_unsafe_target(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|error| error.is::<UnsafeTarget>())
}

/// Resolve ancestors without following a final symlink.
pub fn deletion_path(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err(unsafe_target());
    }
    let absolute = std::path::absolute(path)?;
    let name = absolute.file_name().ok_or_else(unsafe_target)?;
    let parent = absolute.parent().ok_or_else(unsafe_target)?;
    Ok(LocalFilesystem.canonicalize(parent)?.join(name))
}

pub fn move_to_trash(path: &Path, check: &impl Fn() -> io::Result<()>) -> io::Result<()> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let home = home.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "cannot establish the home directory deletion boundary",
        )
    })?;
    delete_with(path, &home, check, native_trash)
}

fn delete_with(
    path: &Path,
    home: &Path,
    check: &impl Fn() -> io::Result<()>,
    delete: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    check()?;
    let path = deletion_path(path)?;
    check()?;
    let home = LocalFilesystem.canonicalize(home)?;
    if path == home {
        return Err(unsafe_target());
    }
    check()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = LocalFilesystem.symlink_metadata(&path)?;
        if metadata.is_dir() {
            check()?;
            let parent = path.parent().ok_or_else(unsafe_target)?;
            if LocalFilesystem.metadata(parent)?.dev() != metadata.dev() {
                return Err(unsafe_target());
            }
        }
    }
    check()?;
    delete(&path)
}

fn native_trash(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use trash::TrashContext;
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut context = TrashContext::new();
        context.set_delete_method(DeleteMethod::NsFileManager);
        context.delete(path).map_err(io::Error::other)
    }
    #[cfg(target_os = "linux")]
    {
        trash::delete(path).map_err(io::Error::other)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native Trash requires a platform adapter on this host",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_targets_and_expired_operations_never_reach_native_trash() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir(&home).unwrap();
        for path in [
            Path::new(""),
            Path::new("."),
            Path::new(".."),
            home.as_path(),
        ] {
            let error =
                delete_with(path, &home, &|| Ok(()), |_| panic!("must not delete")).unwrap_err();
            assert!(is_unsafe_target(&error));
        }
        let file = root.path().join("keep");
        std::fs::write(&file, b"keep").unwrap();
        let error = delete_with(
            &file,
            &home,
            &|| Err(io::Error::new(io::ErrorKind::TimedOut, "cancelled")),
            |_| panic!("must not delete"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(std::fs::read(&file).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn trash_receives_a_symlink_not_its_home_directory_target_and_never_falls_back() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&home, &link).unwrap();
        let expected = deletion_path(&link).unwrap();
        let error = delete_with(&link, &home, &|| Ok(()), |path| {
            assert_eq!(path, expected);
            assert!(path.symlink_metadata().unwrap().file_type().is_symlink());
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Trash refused",
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(home.is_dir());
    }
}
