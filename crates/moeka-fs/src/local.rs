use std::fs::{self, File, Metadata, OpenOptions, ReadDir};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Explicitly local I/O for application-owned state and native handle adapters.
/// Workspace callers use the resource-aware backend, not this namespace.
pub mod native {
    pub use std::fs::{
        DirBuilder, DirEntry, File, FileType, Metadata, OpenOptions, Permissions, ReadDir,
    };
    use std::io;
    use std::path::{Path, PathBuf};

    use super::LocalFilesystem;

    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        LocalFilesystem.read(path.as_ref())
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        LocalFilesystem.read_to_string(path.as_ref())
    }

    pub fn write(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()> {
        std::fs::write(path, content)
    }

    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        LocalFilesystem.metadata(path.as_ref())
    }

    pub fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        LocalFilesystem.symlink_metadata(path.as_ref())
    }

    pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
        LocalFilesystem.read_dir(path.as_ref())
    }

    pub fn read_link(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        LocalFilesystem.read_link(path.as_ref())
    }

    pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        LocalFilesystem.canonicalize(path.as_ref())
    }

    pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::create_dir(path)
    }

    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        LocalFilesystem.create_dir_all(path.as_ref())
    }

    pub fn rename(source: impl AsRef<Path>, target: impl AsRef<Path>) -> io::Result<()> {
        std::fs::rename(source, target)
    }

    pub fn copy(source: impl AsRef<Path>, target: impl AsRef<Path>) -> io::Result<u64> {
        std::fs::copy(source, target)
    }

    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir(path)
    }

    pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    pub fn set_permissions(path: impl AsRef<Path>, permissions: Permissions) -> io::Result<()> {
        std::fs::set_permissions(path, permissions)
    }

    pub fn hard_link(source: impl AsRef<Path>, target: impl AsRef<Path>) -> io::Result<()> {
        std::fs::hard_link(source, target)
    }
}

/// Executes on the current host, including when called by the SSH server.
#[derive(Clone, Copy, Default)]
pub struct LocalFilesystem;

#[derive(Debug)]
pub enum Observation {
    File(Vec<u8>),
    Directory,
    Missing,
    Unavailable(io::Error),
}

impl Observation {
    pub fn from_error(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::Missing
        } else {
            Self::Unavailable(error)
        }
    }
}

impl LocalFilesystem {
    pub fn read_preview(&self, path: &Path, length: usize) -> io::Result<(Vec<u8>, Metadata)> {
        if length == 0 || length > crate::backend::MAX_READ_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid preview length",
            ));
        }
        let mut file = self.open(path)?;
        let before = file.metadata()?;
        if !before.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        let mut bytes = Vec::with_capacity(length);
        (&mut file).take(length as u64).read_to_end(&mut bytes)?;
        if !crate::backend::same_snapshot(&before, &file.metadata()?)
            || !crate::backend::same_snapshot(&before, &self.metadata(path)?)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file changed while reading preview",
            ));
        }
        Ok((bytes, before))
    }

    pub fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        fs::metadata(path)
    }

    pub fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        fs::symlink_metadata(path)
    }

    pub fn read_dir(&self, path: &Path) -> io::Result<ReadDir> {
        fs::read_dir(path)
    }

    pub fn open(&self, path: &Path) -> io::Result<File> {
        File::open(path)
    }

    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    pub fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    pub fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        fs::read_link(path)
    }

    /// Creates the link itself, atomically refusing any existing entry.
    /// Windows needs the link kind even when its target does not exist.
    pub fn create_symlink(
        &self,
        target: &Path,
        link: &Path,
        directory: Option<bool>,
    ) -> io::Result<()> {
        #[cfg(unix)]
        {
            let _ = directory;
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(windows)]
        {
            match directory {
                Some(true) => std::os::windows::fs::symlink_dir(target, link),
                Some(false) => std::os::windows::fs::symlink_file(target, link),
                None => Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Windows symbolic links require an explicit file or directory kind",
                )),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link, directory);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "symbolic link creation is unavailable",
            ))
        }
    }

    pub fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }

    pub fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    /// Requires a bounded caller-supplied buffer; never reads an entire file.
    pub fn read_range(&self, path: &Path, offset: u64, buffer: &mut [u8]) -> io::Result<usize> {
        let mut file = self.open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut count = 0;
        while count < buffer.len() {
            match file.read(&mut buffer[count..]) {
                Ok(0) => break,
                Ok(read) => count += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(count)
    }

    pub fn observe(
        &self,
        path: &Path,
        check: &impl Fn() -> io::Result<()>,
    ) -> io::Result<Observation> {
        check()?;
        Ok(match self.metadata(path) {
            Ok(metadata) if metadata.is_dir() => Observation::Directory,
            Ok(metadata) if metadata.is_file() => {
                check()?;
                match self.read(path) {
                    Ok(bytes) => Observation::File(bytes),
                    Err(error) => Observation::from_error(error),
                }
            }
            Ok(_) => Observation::Unavailable(io::Error::new(
                io::ErrorKind::Unsupported,
                "not a regular file",
            )),
            Err(error) => Observation::from_error(error),
        })
    }

    pub fn save_document(
        &self,
        path: &Path,
        content: &[u8],
        check: &impl Fn() -> io::Result<()>,
    ) -> io::Result<()> {
        check()?;
        if let Some(parent) = path.parent() {
            self.create_dir_all(parent)?;
        }
        check()?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        check()?;
        file.write_all(content)?;
        check()?;
        file.flush()
    }

    pub fn copy_file_data(
        &self,
        source: &Path,
        target: &Path,
        overwrite: bool,
        check: &impl Fn() -> io::Result<()>,
    ) -> io::Result<u64> {
        check()?;
        let mut input = self.open(source)?;
        check()?;
        let mut output = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(overwrite)
            .create_new(!overwrite)
            .open(target)?;
        check()?;
        let copied = io::copy(&mut input, &mut output)?;
        check()?;
        output.flush()?;
        Ok(copied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn observed_failure_is_not_absence_and_deadline_is_not_an_observation() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("note.md");
        fs::write(&file, "draft").unwrap();
        assert!(matches!(
            Observation::from_error(io::ErrorKind::PermissionDenied.into()),
            Observation::Unavailable(_)
        ));
        assert!(matches!(
            LocalFilesystem
                .observe(&directory.path().join("missing"), &|| Ok(()))
                .unwrap(),
            Observation::Missing
        ));
        let error = LocalFilesystem
            .observe(&file, &|| Err(io::ErrorKind::TimedOut.into()))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn copy_without_overwrite_and_prewrite_cancellation_preserve_destination() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, "new").unwrap();
        fs::write(&target, "keep").unwrap();
        let error = LocalFilesystem
            .copy_file_data(&source, &target, false, &|| Ok(()))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        let phases = Cell::new(0);
        let error = LocalFilesystem
            .save_document(&target, b"new", &|| {
                phases.set(phases.get() + 1);
                if phases.get() == 2 {
                    Err(io::ErrorKind::TimedOut.into())
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(fs::read(&target).unwrap(), b"keep");
    }

    #[test]
    fn ranges_handle_short_final_reads_and_large_offsets() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("note");
        fs::write(&file, b"012345").unwrap();
        let mut bytes = [0; 8];
        assert_eq!(LocalFilesystem.read_range(&file, 3, &mut bytes).unwrap(), 3);
        assert_eq!(&bytes[..3], b"345");
        assert_eq!(
            LocalFilesystem.read_range(&file, 99, &mut bytes).unwrap(),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_save_writes_through_symlink_and_preserves_inode() {
        use std::os::unix::fs::{MetadataExt, symlink};

        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("note");
        let link = directory.path().join("link");
        fs::write(&file, "old content").unwrap();
        let inode = fs::metadata(&file).unwrap().ino();
        symlink(&file, &link).unwrap();
        LocalFilesystem
            .save_document(&link, b"new", &|| Ok(()))
            .unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"new");
        assert_eq!(fs::metadata(&file).unwrap().ino(), inode);
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    }
}
