//! Blocking, host-local filesystem contract. Callers must schedule it away from UI threads.
//!
//! A revision is a content digest, not a filesystem CAS token. The check and
//! subsequent in-place write cannot exclude writers outside this backend.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::local::LocalFilesystem;
use crate::path::NativePath;

pub const MAX_READ_BYTES: usize = 64 * 1024;
pub const MAX_SAVE_BYTES: usize = 128 * 1024;
pub const MAX_BATCH: usize = 32;
const MAX_OUTCOMES: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FsError {
    pub operation: String,
    pub path: Option<NativePath>,
    pub kind: String,
    pub raw_os_error: Option<i32>,
    pub message: String,
}

impl FsError {
    pub fn from_io(operation: &str, path: Option<&Path>, error: io::Error) -> Self {
        Self {
            operation: operation.into(),
            path: path.map(NativePath::from_local),
            kind: format!("{:?}", error.kind()),
            raw_os_error: error.raw_os_error(),
            message: error.to_string(),
        }
    }

    pub fn invalid(operation: &str, message: &str) -> Self {
        Self::from_io(
            operation,
            None,
            io::Error::new(io::ErrorKind::InvalidInput, message),
        )
    }

    pub fn unsupported(operation: &str, message: &str) -> Self {
        Self::from_io(
            operation,
            None,
            io::Error::new(io::ErrorKind::Unsupported, message),
        )
    }

    pub fn unsupported_at(operation: &str, path: &Path, message: &str) -> Self {
        Self::from_io(
            operation,
            Some(path),
            io::Error::new(io::ErrorKind::Unsupported, message),
        )
    }
}

pub type FsResult<T> = Result<T, FsError>;

impl std::fmt::Display for FsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.operation)?;
        if let Some(path) = &self.path {
            write!(formatter, " '{path}'")?;
        }
        write!(formatter, ": {}", self.message)
    }
}

impl std::error::Error for FsError {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Revision {
    pub size: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub is_file: bool,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// Windows stores a link kind independently of its target; Unix does not.
    #[serde(default)]
    pub symlink_directory: Option<bool>,
    pub len: u64,
    pub readonly: bool,
    pub modified_unix_nanos: Option<i128>,
    #[serde(default)]
    pub created_unix_nanos: Option<i128>,
}

impl From<fs::Metadata> for Metadata {
    fn from(meta: fs::Metadata) -> Self {
        let modified_unix_nanos =
            meta.modified()
                .ok()
                .map(|time| match time.duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => d.as_nanos() as i128,
                    Err(e) => -(e.duration().as_nanos() as i128),
                });
        Self {
            is_file: meta.is_file(),
            is_dir: meta.is_dir(),
            is_symlink: meta.file_type().is_symlink(),
            symlink_directory: {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::FileTypeExt;
                    meta.file_type()
                        .is_symlink()
                        .then(|| meta.file_type().is_symlink_dir())
                }
                #[cfg(not(windows))]
                {
                    None
                }
            },
            len: meta.len(),
            readonly: meta.permissions().readonly(),
            modified_unix_nanos,
            created_unix_nanos: meta.created().ok().map(|time| {
                match time.duration_since(std::time::UNIX_EPOCH) {
                    Ok(duration) => duration.as_nanos() as i128,
                    Err(error) => -(error.duration().as_nanos() as i128),
                }
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub path: NativePath,
    pub metadata: FsResult<Metadata>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadChunk {
    pub bytes: Vec<u8>,
    pub offset: u64,
    pub eof: bool,
    pub revision: Revision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadPreview {
    pub bytes: Vec<u8>,
    pub metadata: Metadata,
}

/// Missing must be explicit; failed observations are never interpreted as missing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", content = "revision", rename_all = "snake_case")]
pub enum ExpectedRevision {
    Missing,
    Present(Revision),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum SaveOutcome {
    InProgress,
    Saved(Revision),
    Conflict(ExpectedRevision),
    Failed(FsError),
    /// The commit was attempted, but its effect could not be established.
    Uncertain(FsError),
    /// Also returned for expired, unknown, or pre-restart operation IDs.
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveRequest {
    pub operation_id: String,
    pub path: NativePath,
    pub expected: ExpectedRevision,
    pub bytes: Vec<u8>,
}

/// A read handle pins the opened file and verifies the streamed digest at EOF.
/// All chunks are provisional until the final chunk succeeds.
pub struct ReadSession {
    file: File,
    path: PathBuf,
    metadata: fs::Metadata,
    revision: Revision,
    streamed: Sha256,
    offset: u64,
    finished: bool,
}

/// A staged save owns a private same-directory file until commit or cancellation.
/// Declared sizes do not allocate memory or preallocate disk; bounded appends
/// consume space incrementally and filesystem capacity errors remain explicit.
/// Dropping it aborts the upload; a process crash can leave staging files behind.
pub struct StagedSave {
    operation_id: String,
    path: NativePath,
    expected: ExpectedRevision,
    total_bytes: u64,
    written: u64,
    staged: tempfile::NamedTempFile,
    hash: Sha256,
    poisoned: bool,
}

pub(crate) fn same_snapshot(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    if a.len() != b.len() || a.modified().ok() != b.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn copy_verified(input: &mut File, output: &mut File, revision: &Revision) -> io::Result<()> {
    let mut remaining = revision.size;
    let mut hash = Sha256::new();
    let mut buffer = [0; MAX_READ_BYTES];
    while remaining > 0 {
        let length = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..length])?;
        output.write_all(&buffer[..length])?;
        hash.update(&buffer[..length]);
        remaining -= length as u64;
    }
    let mut extra = [0; 1];
    let digest: [u8; 32] = hash.finalize().into();
    if input.read(&mut extra)? != 0 || digest != revision.sha256 {
        return Err(io::Error::other("staged content changed during save"));
    }
    Ok(())
}

fn verify_staged(input: &mut File, revision: &Revision) -> io::Result<()> {
    input.seek(SeekFrom::Start(0))?;
    let mut remaining = revision.size;
    let mut hash = Sha256::new();
    let mut buffer = [0; MAX_READ_BYTES];
    while remaining > 0 {
        let length = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..length])?;
        hash.update(&buffer[..length]);
        remaining -= length as u64;
    }
    let mut extra = [0; 1];
    let digest: [u8; 32] = hash.finalize().into();
    if input.read(&mut extra)? != 0 || digest != revision.sha256 {
        return Err(io::Error::other("staged content changed before save"));
    }
    input.seek(SeekFrom::Start(0))?;
    Ok(())
}

impl ReadSession {
    pub fn revision(&self) -> &Revision {
        &self.revision
    }

    pub fn next_chunk(&mut self, length: usize) -> FsResult<ReadChunk> {
        if length == 0 || length > MAX_READ_BYTES {
            return Err(FsError::invalid("next_read", "invalid chunk length"));
        }
        if self.finished {
            return Err(FsError::invalid("next_read", "read session has ended"));
        }
        let result = self.next_chunk_inner(length);
        if result.as_ref().map_or(true, |chunk| chunk.eof) {
            self.finished = true;
        }
        result
    }

    fn next_chunk_inner(&mut self, length: usize) -> FsResult<ReadChunk> {
        let remaining = self.revision.size.saturating_sub(self.offset);
        let count = (remaining.min(length as u64)) as usize;
        let mut bytes = vec![0; count];
        self.file
            .read_exact(&mut bytes)
            .map_err(|e| FsError::from_io("next_read", Some(&self.path), e))?;
        self.streamed.update(&bytes);
        let offset = self.offset;
        self.offset += count as u64;
        let eof = self.offset == self.revision.size;
        if eof {
            let mut extra = [0; 1];
            let extra_count = self
                .file
                .read(&mut extra)
                .map_err(|e| FsError::from_io("next_read", Some(&self.path), e))?;
            let current = self
                .file
                .metadata()
                .map_err(|e| FsError::from_io("next_read_metadata", Some(&self.path), e))?;
            let named = fs::metadata(&self.path)
                .map_err(|e| FsError::from_io("next_read_metadata", Some(&self.path), e))?;
            let digest: [u8; 32] = self.streamed.clone().finalize().into();
            if extra_count != 0
                || digest != self.revision.sha256
                || !same_snapshot(&self.metadata, &current)
                || !same_snapshot(&self.metadata, &named)
            {
                return Err(FsError::from_io(
                    "next_read",
                    Some(&self.path),
                    io::Error::other("file changed while streaming"),
                ));
            }
        }
        Ok(ReadChunk {
            bytes,
            offset,
            eof,
            revision: self.revision.clone(),
        })
    }
}

impl StagedSave {
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn append(&mut self, offset: u64, bytes: &[u8]) -> FsResult<()> {
        if bytes.is_empty()
            || bytes.len() > MAX_READ_BYTES
            || offset != self.written
            || (bytes.len() as u64) > self.total_bytes.saturating_sub(self.written)
        {
            return Err(FsError::invalid(
                "append_save",
                "invalid offset or chunk length",
            ));
        }
        if self.poisoned {
            return Err(FsError::invalid(
                "append_save",
                "staging session has failed",
            ));
        }
        if let Err(e) = self.staged.write_all(bytes) {
            self.poisoned = true;
            return Err(FsError::from_io(
                "append_save",
                self.path.to_local().ok().as_deref(),
                e,
            ));
        }
        self.hash.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }
}

/// All operations block. Implementations must preserve individual errors in batches.
pub trait Filesystem: Send + Sync {
    fn canonicalize(&self, path: &NativePath) -> FsResult<NativePath>;
    fn read_link(&self, path: &NativePath) -> FsResult<NativePath>;
    fn create_symlink(
        &self,
        target: &NativePath,
        link: &NativePath,
        directory: Option<bool>,
    ) -> FsResult<()>;
    fn metadata(&self, path: &NativePath, follow: bool) -> FsResult<Metadata>;
    fn metadata_batch(
        &self,
        paths: &[NativePath],
        follow: bool,
    ) -> FsResult<Vec<FsResult<Metadata>>>;
    fn read_chunk(&self, path: &NativePath, offset: u64, length: usize) -> FsResult<ReadChunk>;
    fn read_preview(&self, path: &NativePath, length: usize) -> FsResult<ReadPreview>;
    fn read_previews(
        &self,
        paths: &[NativePath],
        length: usize,
    ) -> FsResult<Vec<FsResult<ReadPreview>>> {
        if paths.len() > MAX_BATCH || length == 0 || length > MAX_READ_BYTES {
            return Err(FsError::invalid("read_previews", "invalid preview batch"));
        }
        Ok(paths
            .iter()
            .map(|path| self.read_preview(path, length))
            .collect())
    }
    fn create_file(&self, path: &NativePath) -> FsResult<()>;
    fn create_dir(&self, path: &NativePath, recursive: bool) -> FsResult<()>;
    fn copy_file(&self, source: &NativePath, target: &NativePath) -> FsResult<u64>;
    fn rename(&self, source: &NativePath, target: &NativePath) -> FsResult<()>;
    fn trash(&self, path: &NativePath) -> FsResult<()>;
    fn save(&self, request: SaveRequest) -> FsResult<SaveOutcome>;
    fn save_outcome(&self, operation_id: &str) -> SaveOutcome;
}

struct SaveRecord {
    fingerprint: [u8; 32],
    outcome: SaveOutcome,
}

#[derive(Default)]
struct SaveJournal {
    records: HashMap<String, SaveRecord>,
    order: VecDeque<String>,
}

impl SaveJournal {
    fn reserve(&mut self, id: &str, fingerprint: [u8; 32]) -> FsResult<Option<SaveOutcome>> {
        if let Some(record) = self.records.get(id) {
            if record.fingerprint != fingerprint {
                return Err(FsError::invalid(
                    "save",
                    "operation ID reused with different request",
                ));
            }
            return Ok(Some(record.outcome.clone()));
        }
        if self.records.len() >= MAX_OUTCOMES {
            let completed = self
                .order
                .iter()
                .position(|id| {
                    self.records
                        .get(id)
                        .is_some_and(|record| record.outcome != SaveOutcome::InProgress)
                })
                .ok_or_else(|| {
                    FsError::from_io(
                        "save",
                        None,
                        io::Error::new(io::ErrorKind::WouldBlock, "save outcome journal is busy"),
                    )
                })?;
            let expired = self.order.remove(completed).expect("completed entry found");
            self.records.remove(&expired);
        }
        self.order.push_back(id.to_owned());
        self.records.insert(
            id.to_owned(),
            SaveRecord {
                fingerprint,
                outcome: SaveOutcome::InProgress,
            },
        );
        Ok(None)
    }

    fn finish(&mut self, id: &str, outcome: SaveOutcome) {
        self.records
            .get_mut(id)
            .expect("in-progress record retained")
            .outcome = outcome;
    }

    fn outcome(&self, id: &str) -> SaveOutcome {
        self.records
            .get(id)
            .map(|record| record.outcome.clone())
            .unwrap_or(SaveOutcome::Unknown)
    }
}

/// Outcomes are process-local and bounded; after a restart the answer is Unknown.
#[derive(Default)]
pub struct LocalBackend {
    fs: LocalFilesystem,
    saves: Mutex<SaveJournal>,
    mutations: Mutex<()>,
}

impl LocalBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn local(&self) -> &LocalFilesystem {
        &self.fs
    }

    pub fn revision(&self, path: &Path) -> FsResult<Revision> {
        let metadata = self
            .fs
            .metadata(path)
            .map_err(|e| FsError::from_io("revision_metadata", Some(path), e))?;
        if !metadata.is_file() {
            return Err(FsError::unsupported_at(
                "revision",
                path,
                "not a regular file",
            ));
        }
        let mut file = self
            .fs
            .open(path)
            .map_err(|e| FsError::from_io("open_revision", Some(path), e))?;
        let mut hash = Sha256::new();
        let mut buf = [0; 64 * 1024];
        let mut size = 0_u64;
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| FsError::from_io("read_revision", Some(path), e))?;
            if n == 0 {
                break;
            }
            size += n as u64;
            hash.update(&buf[..n]);
        }
        Ok(Revision {
            size,
            sha256: hash.finalize().into(),
        })
    }

    fn observed_revision(&self, path: &Path) -> FsResult<ExpectedRevision> {
        match self.revision(path) {
            Ok(revision) => Ok(ExpectedRevision::Present(revision)),
            Err(error) if error.kind == "NotFound" => Ok(ExpectedRevision::Missing),
            Err(error) => Err(error),
        }
    }

    pub fn open_read(&self, path: &NativePath) -> FsResult<ReadSession> {
        let path = path
            .to_local()
            .map_err(|e| FsError::from_io("open_read", None, e))?;
        let named = self
            .fs
            .metadata(&path)
            .map_err(|e| FsError::from_io("open_read_metadata", Some(&path), e))?;
        if !named.is_file() {
            return Err(FsError::unsupported_at(
                "open_read",
                &path,
                "not a regular file",
            ));
        }
        let mut file = self
            .fs
            .open(&path)
            .map_err(|e| FsError::from_io("open_read", Some(&path), e))?;
        let metadata = file
            .metadata()
            .map_err(|e| FsError::from_io("open_read_metadata", Some(&path), e))?;
        if !metadata.is_file() {
            return Err(FsError::unsupported_at(
                "open_read",
                &path,
                "not a regular file",
            ));
        }
        let mut hash = Sha256::new();
        let mut buf = [0; MAX_READ_BYTES];
        let mut size = 0_u64;
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| FsError::from_io("open_read", Some(&path), e))?;
            if n == 0 {
                break;
            }
            size += n as u64;
            hash.update(&buf[..n]);
        }
        let after = file
            .metadata()
            .map_err(|e| FsError::from_io("open_read_metadata", Some(&path), e))?;
        let named = self
            .fs
            .metadata(&path)
            .map_err(|e| FsError::from_io("open_read_metadata", Some(&path), e))?;
        if size != metadata.len()
            || !same_snapshot(&metadata, &after)
            || !same_snapshot(&metadata, &named)
        {
            return Err(FsError::from_io(
                "open_read",
                Some(&path),
                io::Error::other("file changed while hashing"),
            ));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|e| FsError::from_io("open_read", Some(&path), e))?;
        Ok(ReadSession {
            file,
            path,
            metadata,
            revision: Revision {
                size,
                sha256: hash.finalize().into(),
            },
            streamed: Sha256::new(),
            offset: 0,
            finished: false,
        })
    }

    pub fn begin_save(
        &self,
        operation_id: String,
        path: NativePath,
        expected: ExpectedRevision,
        total_bytes: u64,
    ) -> FsResult<StagedSave> {
        if operation_id.is_empty() || operation_id.len() > 128 {
            return Err(FsError::invalid("begin_save", "invalid operation ID"));
        }
        let local = path
            .to_local()
            .map_err(|e| FsError::from_io("begin_save", None, e))?;
        let parent = local
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staged = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| FsError::from_io("stage_save", Some(&local), e))?;
        Ok(StagedSave {
            operation_id,
            path,
            expected,
            total_bytes,
            written: 0,
            staged,
            hash: Sha256::new(),
            poisoned: false,
        })
    }

    /// Commits with local in-place semantics, not an external-writer CAS. Once
    /// truncation is attempted, errors are uncertain and recovery must be retained.
    pub fn commit_save(&self, staged: StagedSave) -> FsResult<SaveOutcome> {
        if staged.poisoned || staged.written != staged.total_bytes {
            return Err(FsError::invalid(
                "commit_save",
                "staged upload is incomplete",
            ));
        }
        let revision = Revision {
            size: staged.total_bytes,
            sha256: staged.hash.clone().finalize().into(),
        };
        let fingerprint: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(
                &staged.operation_id,
                &staged.path,
                &staged.expected,
                &revision,
            ))
            .map_err(|e| FsError::invalid("save", &e.to_string()))?,
        )
        .into();
        let _mutation = self.mutations.lock().unwrap_or_else(|e| e.into_inner());
        let mut saves = self.saves.lock().unwrap_or_else(|e| e.into_inner());
        let id = staged.operation_id.clone();
        if let Some(outcome) = saves.reserve(&id, fingerprint)? {
            return Ok(outcome);
        }
        drop(saves);
        let outcome = self.perform_staged_save(staged, revision);
        self.saves
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .finish(&id, outcome.clone());
        Ok(outcome)
    }

    fn perform_staged_save(&self, mut staged: StagedSave, revision: Revision) -> SaveOutcome {
        let path = match staged.path.to_local() {
            Ok(path) => path,
            Err(error) => return SaveOutcome::Failed(FsError::from_io("save", None, error)),
        };
        let result = (|| -> FsResult<SaveOutcome> {
            staged
                .staged
                .as_file()
                .sync_all()
                .map_err(|e| FsError::from_io("stage_save", Some(&path), e))?;
            verify_staged(staged.staged.as_file_mut(), &revision)
                .map_err(|e| FsError::from_io("verify_stage", Some(&path), e))?;
            let before = self.observed_revision(&path)?;
            if before != staged.expected {
                return Ok(SaveOutcome::Conflict(before));
            }
            match staged.expected {
                ExpectedRevision::Missing => {
                    if let Err(error) = staged.staged.persist_noclobber(&path) {
                        return Ok(SaveOutcome::Uncertain(FsError::from_io(
                            "commit_save",
                            Some(&path),
                            error.error,
                        )));
                    }
                }
                ExpectedRevision::Present(_) => {
                    let mut output = OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .map_err(|e| FsError::from_io("open_save", Some(&path), e))?;
                    let opened = output
                        .metadata()
                        .map_err(|e| FsError::from_io("save_metadata", Some(&path), e))?;
                    if !opened.is_file() {
                        return Err(FsError::unsupported_at("save", &path, "not a regular file"));
                    }
                    staged
                        .staged
                        .as_file_mut()
                        .seek(SeekFrom::Start(0))
                        .map_err(|e| FsError::from_io("stage_save", Some(&path), e))?;
                    let now = self.observed_revision(&path)?;
                    if now != staged.expected {
                        return Ok(SaveOutcome::Conflict(now));
                    }
                    let named = self
                        .fs
                        .metadata(&path)
                        .map_err(|e| FsError::from_io("save_metadata", Some(&path), e))?;
                    if !same_snapshot(&opened, &named) {
                        return Err(FsError::from_io(
                            "save_metadata",
                            Some(&path),
                            io::Error::other("file changed while preparing save"),
                        ));
                    }
                    // This matches local write-through semantics. External writers can still
                    // race this check, and a failure after truncate may leave partial content.
                    if let Err(error) = output
                        .set_len(0)
                        .and_then(|()| {
                            copy_verified(staged.staged.as_file_mut(), &mut output, &revision)
                        })
                        .and_then(|()| output.flush())
                    {
                        return Ok(SaveOutcome::Uncertain(FsError::from_io(
                            "commit_save",
                            Some(&path),
                            error,
                        )));
                    }
                }
            }
            Ok(SaveOutcome::Saved(revision))
        })();
        result.unwrap_or_else(SaveOutcome::Failed)
    }
}

impl Filesystem for LocalBackend {
    fn create_symlink(
        &self,
        target: &NativePath,
        link: &NativePath,
        directory: Option<bool>,
    ) -> FsResult<()> {
        let target = target
            .to_local()
            .map_err(|error| FsError::from_io("symlink_target", None, error))?;
        let link = link
            .to_local()
            .map_err(|error| FsError::from_io("symlink", None, error))?;
        self.fs
            .create_symlink(&target, &link, directory)
            .map_err(|error| FsError::from_io("symlink", Some(&link), error))
    }

    fn canonicalize(&self, path: &NativePath) -> FsResult<NativePath> {
        let path = path
            .to_local()
            .map_err(|error| FsError::from_io("canonicalize", None, error))?;
        self.fs
            .canonicalize(&path)
            .map(|path| NativePath::from_local(&path))
            .map_err(|error| FsError::from_io("canonicalize", Some(&path), error))
    }

    fn read_link(&self, path: &NativePath) -> FsResult<NativePath> {
        let path = path
            .to_local()
            .map_err(|error| FsError::from_io("read_link", None, error))?;
        self.fs
            .read_link(&path)
            .map(|path| NativePath::from_local(&path))
            .map_err(|error| FsError::from_io("read_link", Some(&path), error))
    }

    fn metadata(&self, path: &NativePath, follow: bool) -> FsResult<Metadata> {
        let p = path
            .to_local()
            .map_err(|e| FsError::from_io("metadata", None, e))?;
        let result = if follow {
            self.fs.metadata(&p)
        } else {
            self.fs.symlink_metadata(&p)
        };
        result
            .map(Metadata::from)
            .map_err(|e| FsError::from_io("metadata", Some(&p), e))
    }

    fn metadata_batch(
        &self,
        paths: &[NativePath],
        follow: bool,
    ) -> FsResult<Vec<FsResult<Metadata>>> {
        if paths.len() > MAX_BATCH {
            return Err(FsError::invalid("metadata_batch", "batch limit exceeded"));
        }
        Ok(paths
            .iter()
            .map(|path| self.metadata(path, follow))
            .collect())
    }

    fn read_chunk(&self, path: &NativePath, offset: u64, length: usize) -> FsResult<ReadChunk> {
        if length == 0 || length > MAX_READ_BYTES {
            return Err(FsError::invalid("read", "invalid chunk length"));
        }
        let p = path
            .to_local()
            .map_err(|e| FsError::from_io("read", None, e))?;
        let before = self.revision(&p)?;
        let mut bytes = vec![0; length];
        let n = self
            .fs
            .read_range(&p, offset, &mut bytes)
            .map_err(|e| FsError::from_io("read", Some(&p), e))?;
        bytes.truncate(n);
        let after = self.revision(&p)?;
        if before != after {
            return Err(FsError::from_io(
                "read",
                Some(&p),
                io::Error::other("file changed while reading"),
            ));
        }
        Ok(ReadChunk {
            bytes,
            offset,
            eof: offset.saturating_add(n as u64) >= after.size,
            revision: after,
        })
    }

    fn read_preview(&self, path: &NativePath, length: usize) -> FsResult<ReadPreview> {
        let path = path
            .to_local()
            .map_err(|error| FsError::from_io("read_preview", None, error))?;
        let (bytes, metadata) = self
            .fs
            .read_preview(&path, length)
            .map_err(|error| FsError::from_io("read_preview", Some(&path), error))?;
        Ok(ReadPreview {
            bytes,
            metadata: metadata.into(),
        })
    }

    fn create_file(&self, path: &NativePath) -> FsResult<()> {
        let _mutation = self.mutations.lock().unwrap_or_else(|e| e.into_inner());
        let p = path
            .to_local()
            .map_err(|e| FsError::from_io("create_file", None, e))?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
            .map(|_| ())
            .map_err(|e| FsError::from_io("create_file", Some(&p), e))
    }

    fn create_dir(&self, path: &NativePath, recursive: bool) -> FsResult<()> {
        let _mutation = self.mutations.lock().unwrap_or_else(|e| e.into_inner());
        let p = path
            .to_local()
            .map_err(|e| FsError::from_io("create_dir", None, e))?;
        (if recursive {
            self.fs.create_dir_all(&p)
        } else {
            fs::create_dir(&p)
        })
        .map_err(|e| FsError::from_io("create_dir", Some(&p), e))
    }

    fn copy_file(&self, source: &NativePath, target: &NativePath) -> FsResult<u64> {
        let _mutation = self.mutations.lock().unwrap_or_else(|e| e.into_inner());
        let s = source
            .to_local()
            .map_err(|e| FsError::from_io("copy", None, e))?;
        let t = target
            .to_local()
            .map_err(|e| FsError::from_io("copy", None, e))?;
        self.fs
            .copy_file_data(&s, &t, false, &|| Ok(()))
            .map_err(|e| FsError::from_io("copy", Some(&t), e))
    }

    fn rename(&self, source: &NativePath, target: &NativePath) -> FsResult<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            let _mutation = self.mutations.lock().unwrap_or_else(|e| e.into_inner());
            let source = source
                .to_local()
                .map_err(|e| FsError::from_io("rename", None, e))?;
            let target = target
                .to_local()
                .map_err(|e| FsError::from_io("rename", None, e))?;
            let from = CString::new(source.as_os_str().as_bytes()).map_err(|e| {
                FsError::from_io(
                    "rename",
                    Some(&source),
                    io::Error::new(io::ErrorKind::InvalidInput, e),
                )
            })?;
            let to = CString::new(target.as_os_str().as_bytes()).map_err(|e| {
                FsError::from_io(
                    "rename",
                    Some(&target),
                    io::Error::new(io::ErrorKind::InvalidInput, e),
                )
            })?;
            // Both native calls atomically reject an existing destination, including symlinks.
            #[cfg(target_os = "linux")]
            let result = unsafe {
                libc::renameat2(
                    libc::AT_FDCWD,
                    from.as_ptr(),
                    libc::AT_FDCWD,
                    to.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            };
            #[cfg(target_os = "macos")]
            let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
            if result == 0 {
                Ok(())
            } else {
                Err(FsError::from_io(
                    "rename",
                    Some(&source),
                    io::Error::last_os_error(),
                ))
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (source, target);
            Err(FsError::unsupported(
                "rename",
                "race-free no-replace rename is not implemented on this platform",
            ))
        }
    }

    fn trash(&self, path: &NativePath) -> FsResult<()> {
        let path = path
            .to_local()
            .map_err(|error| FsError::from_io("trash", None, error))?;
        let _mutation = self
            .mutations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        crate::trash::move_to_trash(&path, &|| Ok(()))
            .map_err(|error| FsError::from_io("trash", Some(&path), error))
    }

    fn save(&self, request: SaveRequest) -> FsResult<SaveOutcome> {
        if request.operation_id.is_empty()
            || request.operation_id.len() > 128
            || request.bytes.len() > MAX_SAVE_BYTES
        {
            return Err(FsError::invalid(
                "save",
                "invalid operation ID or oversized save",
            ));
        }
        let mut staged = self.begin_save(
            request.operation_id,
            request.path,
            request.expected,
            request.bytes.len() as u64,
        )?;
        for (index, chunk) in request.bytes.chunks(MAX_READ_BYTES).enumerate() {
            staged.append((index * MAX_READ_BYTES) as u64, chunk)?;
        }
        self.commit_save(staged)
    }

    fn save_outcome(&self, operation_id: &str) -> SaveOutcome {
        self.saves
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outcome(operation_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_journal_expires_completed_outcomes_without_exhausting_future_saves() {
        let mut journal = SaveJournal::default();
        let saved = SaveOutcome::Saved(Revision {
            size: 0,
            sha256: [0; 32],
        });
        for index in 0..MAX_OUTCOMES {
            let id = index.to_string();
            assert!(journal.reserve(&id, [0; 32]).unwrap().is_none());
            journal.finish(&id, saved.clone());
        }
        assert_eq!(journal.reserve("0", [0; 32]).unwrap(), Some(saved.clone()));
        assert!(journal.reserve("0", [1; 32]).is_err());
        assert!(journal.reserve("next", [0; 32]).unwrap().is_none());
        assert_eq!(journal.outcome("0"), SaveOutcome::Unknown);
        assert_eq!(journal.outcome("1"), saved);
        assert_eq!(journal.outcome("next"), SaveOutcome::InProgress);
    }

    #[test]
    fn save_journal_never_expires_in_progress_writes() {
        let mut journal = SaveJournal::default();
        for index in 0..MAX_OUTCOMES {
            journal.reserve(&index.to_string(), [0; 32]).unwrap();
        }
        assert_eq!(
            journal.reserve("next", [0; 32]).unwrap_err().kind,
            "WouldBlock"
        );
        journal.finish("1", SaveOutcome::Unknown);
        journal.reserve("next", [0; 32]).unwrap();
        assert_eq!(journal.outcome("0"), SaveOutcome::InProgress);
        assert_eq!(journal.outcome("1"), SaveOutcome::Unknown);
        assert_eq!(journal.outcome("next"), SaveOutcome::InProgress);
    }

    #[test]
    fn save_conflicts_are_non_destructive_and_operation_ids_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note");
        fs::write(&path, "original").unwrap();
        let backend = LocalBackend::new();
        let expected = ExpectedRevision::Present(backend.revision(&path).unwrap());
        fs::write(&path, "external").unwrap();
        let mut request = SaveRequest {
            operation_id: "one".into(),
            path: NativePath::from_local(&path),
            expected,
            bytes: b"my edits".to_vec(),
        };
        assert!(matches!(
            backend.save(request.clone()).unwrap(),
            SaveOutcome::Conflict(_)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"external");
        assert!(matches!(
            backend.save_outcome("one"),
            SaveOutcome::Conflict(_)
        ));
        request.expected = ExpectedRevision::Present(backend.revision(&path).unwrap());
        assert!(backend.save(request.clone()).is_err());
        request.operation_id = "two".into();
        let saved = backend.save(request.clone()).unwrap();
        assert!(matches!(saved, SaveOutcome::Saved(_)));
        assert_eq!(backend.save(request).unwrap(), saved);
        assert_eq!(fs::read(&path).unwrap(), b"my edits");
        assert_eq!(
            LocalBackend::new().save_outcome("two"),
            SaveOutcome::Unknown
        );
    }

    #[test]
    fn corrupted_staging_is_rejected_before_destination_is_touched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note");
        fs::write(&path, "original").unwrap();
        let backend = LocalBackend::new();
        let expected = ExpectedRevision::Present(backend.revision(&path).unwrap());
        let mut staged = backend
            .begin_save("bad".into(), NativePath::from_local(&path), expected, 3)
            .unwrap();
        staged.append(0, b"new").unwrap();
        staged
            .staged
            .as_file_mut()
            .seek(SeekFrom::Start(0))
            .unwrap();
        staged.staged.write_all(b"bad").unwrap();
        assert!(matches!(
            backend.commit_save(staged).unwrap(),
            SaveOutcome::Failed(_)
        ));
        assert_eq!(fs::read(path).unwrap(), b"original");
    }

    #[test]
    fn preview_batches_bound_reads_and_preserve_per_path_failures() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.md");
        fs::write(&path, b"# Title\n").unwrap();
        let large_size = 512 * 1024 * 1024 + 1;
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(large_size)
            .unwrap();
        let backend = LocalBackend::new();
        let previews = backend
            .read_previews(
                &[
                    NativePath::from_local(&path),
                    NativePath::from_local(&directory.path().join("missing")),
                ],
                7,
            )
            .unwrap();
        let preview = previews[0].as_ref().unwrap();
        assert_eq!(preview.bytes, b"# Title");
        assert_eq!(preview.metadata.len, large_size);
        assert_eq!(previews[1].as_ref().unwrap_err().kind, "NotFound");
        assert!(
            backend
                .read_preview(&NativePath::from_local(&path), MAX_READ_BYTES + 1)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_symlinks_hardlinks_inode_and_permissions() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note");
        let link = dir.path().join("link");
        let hard = dir.path().join("hard");
        fs::write(&path, "keep").unwrap();
        symlink(&path, &link).unwrap();
        fs::hard_link(&path, &hard).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let backend = LocalBackend::new();
        assert_eq!(
            backend.read_link(&NativePath::from_local(&link)).unwrap(),
            NativePath::from_local(&path)
        );
        assert_eq!(
            backend
                .canonicalize(&NativePath::from_local(&link))
                .unwrap(),
            NativePath::from_local(&fs::canonicalize(&path).unwrap())
        );
        let expected = ExpectedRevision::Present(backend.revision(&path).unwrap());
        let request = SaveRequest {
            operation_id: "link".into(),
            path: NativePath::from_local(&link),
            expected,
            bytes: b"change".to_vec(),
        };
        assert!(matches!(
            backend.save(request).unwrap(),
            SaveOutcome::Saved(_)
        ));
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read(&path).unwrap(), b"change");
        assert_eq!(fs::read(&hard).unwrap(), b"change");
        let after = fs::metadata(&path).unwrap();
        assert_eq!(
            (metadata.dev(), metadata.ino(), metadata.mode()),
            (after.dev(), after.ino(), after.mode())
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn save_preserves_extended_attributes_when_supported() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note");
        fs::write(&path, "old").unwrap();
        let native = CString::new(path.as_os_str().as_bytes()).unwrap();
        let name = CString::new(if cfg!(target_os = "macos") {
            "com.moeka.test"
        } else {
            "user.moeka.test"
        })
        .unwrap();
        let value = b"attribute";
        #[cfg(target_os = "macos")]
        let status = unsafe {
            libc::setxattr(
                native.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        #[cfg(target_os = "linux")]
        let status = unsafe {
            libc::setxattr(
                native.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if status != 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
            ) {
                return;
            }
            panic!("setxattr: {error}");
        }
        let backend = LocalBackend::new();
        let expected = ExpectedRevision::Present(backend.revision(&path).unwrap());
        assert!(matches!(
            backend
                .save(SaveRequest {
                    operation_id: "xattr".into(),
                    path: NativePath::from_local(&path),
                    expected,
                    bytes: b"new".to_vec()
                })
                .unwrap(),
            SaveOutcome::Saved(_)
        ));
        let mut actual = [0; 32];
        #[cfg(target_os = "macos")]
        let count = unsafe {
            libc::getxattr(
                native.as_ptr(),
                name.as_ptr(),
                actual.as_mut_ptr().cast(),
                actual.len(),
                0,
                0,
            )
        };
        #[cfg(target_os = "linux")]
        let count = unsafe {
            libc::getxattr(
                native.as_ptr(),
                name.as_ptr(),
                actual.as_mut_ptr().cast(),
                actual.len(),
            )
        };
        assert_eq!(count, value.len() as isize);
        assert_eq!(&actual[..count as usize], value);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn rename_no_replace_preserves_existing_destination_and_moves_links() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let destination = dir.path().join("destination");
        fs::write(&source, b"source").unwrap();
        fs::write(&destination, b"destination").unwrap();
        let backend = LocalBackend::new();
        let error = backend
            .rename(
                &NativePath::from_local(&source),
                &NativePath::from_local(&destination),
            )
            .unwrap_err();
        assert_eq!(error.kind, "AlreadyExists");
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&destination).unwrap(), b"destination");
        fs::remove_file(&source).unwrap();
        symlink(&destination, &source).unwrap();
        let moved = dir.path().join("moved");
        backend
            .rename(
                &NativePath::from_local(&source),
                &NativePath::from_local(&moved),
            )
            .unwrap();
        assert!(fs::symlink_metadata(moved).unwrap().is_symlink());
        assert_eq!(fs::read(destination).unwrap(), b"destination");
    }
}
