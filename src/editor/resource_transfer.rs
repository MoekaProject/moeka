//! Blocking transfers; callers must use `fs_task::run_write`.
//!
//! Files are staged with `ExpectedRevision::Missing`, never overwritten. A failed
//! tree copy retains completed destination entries and leaves the source alone.
//! Cross-filesystem moves verify the entire copy before using native Trash.
//! This is not a transaction against external filesystem writers: the protocol
//! has no directory handles or atomic revision-conditional removal.

use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::i18n::runtime::RuntimeText;
use moeka_fs::backend::{
    ExpectedRevision, Filesystem, FsError, LocalBackend, MAX_READ_BYTES, Metadata, ReadChunk,
    ReadSession, Revision, SaveOutcome, StagedSave,
};
use moeka_fs::path::{FilesystemId, NativePath, ResourcePath};
use moeka_fs::service::{Request, Response};
use moeka_ssh::filesystem::SshFilesystem;

const MAX_DEPTH: usize = 64;
const MAX_ENTRIES: usize = 100_000;
const MAX_COLLISIONS: usize = 10_000;
static NEXT_OPERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(super) enum ResourceFilesystem {
    Local,
    Ssh(Arc<SshFilesystem>),
}

/// Returns the actual (possibly numbered) destination. Errors after mutation
/// explicitly retain the destination and report potentially uncertain effects;
/// callers must reconcile before retrying. Files are streamed in bounded chunks
/// without a per-file size ceiling; disk space and the caller's deadline apply.
pub(super) fn transfer(
    source_fs: &ResourceFilesystem,
    target_fs: &ResourceFilesystem,
    source: &ResourcePath,
    target_directory: &ResourcePath,
    move_source: bool,
) -> io::Result<ResourcePath> {
    transfer_checked(
        source_fs,
        target_fs,
        source,
        target_directory,
        move_source,
        &crate::platform::fs_task::check_deadline,
    )
}

struct Endpoint<'a> {
    filesystem: &'a ResourceFilesystem,
    local: LocalBackend,
}

impl<'a> Endpoint<'a> {
    fn new(filesystem: &'a ResourceFilesystem, resource: &ResourcePath) -> io::Result<Self> {
        let matches = match filesystem {
            ResourceFilesystem::Local => resource.filesystem == FilesystemId::Local,
            ResourceFilesystem::Ssh(ssh) => &resource.filesystem == ssh.identity(),
        };
        if !matches {
            return Err(invalid("filesystem_identity_mismatch"));
        }
        Ok(Self {
            filesystem,
            local: LocalBackend::new(),
        })
    }

    fn backend(&self) -> &dyn Filesystem {
        match self.filesystem {
            ResourceFilesystem::Local => &self.local,
            ResourceFilesystem::Ssh(ssh) => ssh.as_ref(),
        }
    }

    fn metadata(&self, path: &NativePath) -> io::Result<Metadata> {
        self.backend().metadata(path, false).map_err(fs_error)
    }

    fn validate_link(&self, entry: &Entry, destination: &NativePath) -> io::Result<()> {
        let Some(target) = &entry.link else {
            return Ok(());
        };
        if std::mem::discriminant(target) != std::mem::discriminant(destination) {
            return Err(unsupported("incompatible_symlink_path_encoding"));
        }
        if matches!(destination, NativePath::Windows(_))
            && entry.metadata.symlink_directory.is_none()
        {
            return Err(unsupported("windows_symlink_kind_required"));
        }
        if let ResourceFilesystem::Ssh(ssh) = self.filesystem
            && !ssh
                .capabilities()
                .iter()
                .any(|capability| capability == "create_symlink")
        {
            return Err(unsupported("ssh_symlink_capability_missing"));
        }
        Ok(())
    }

    fn directory(
        &self,
        path: &NativePath,
        check: &impl Fn() -> io::Result<()>,
    ) -> io::Result<Vec<NativePath>> {
        check()?;
        let mut children = Vec::new();
        match self.filesystem {
            ResourceFilesystem::Local => {
                for entry in self.local.local().read_dir(&path.to_local()?)? {
                    check()?;
                    let entry = entry?;
                    children.push(NativePath::from_local(&entry.path()));
                    if children.len() > MAX_ENTRIES {
                        return Err(invalid("transfer_entry_limit_exceeded"));
                    }
                }
            }
            ResourceFilesystem::Ssh(ssh) => {
                let Response::Session(session) = ssh
                    .request(Request::OpenDirectory { path: path.clone() })
                    .map_err(fs_error)?
                else {
                    return Err(invalid("unexpected_directory_response"));
                };
                let result = (|| {
                    loop {
                        check()?;
                        let Response::Directory { entries, done } = ssh
                            .request(Request::NextDirectory { session })
                            .map_err(fs_error)?
                        else {
                            return Err(invalid("unexpected_directory_page"));
                        };
                        for entry in entries {
                            check()?;
                            let entry = entry.map_err(fs_error)?;
                            entry.metadata.map_err(fs_error)?;
                            children.push(entry.path);
                            if children.len() > MAX_ENTRIES {
                                return Err(invalid("transfer_entry_limit_exceeded"));
                            }
                        }
                        if done {
                            break;
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    return Err(cancel_error(ssh, session, error));
                }
            }
        }
        let mut names = HashSet::new();
        for child in &children {
            let name = child
                .file_name()
                .ok_or_else(|| invalid("invalid_directory_entry"))?;
            if path.join_native_child(&name)? != *child || !names.insert(name) {
                return Err(invalid("invalid_or_duplicate_directory_child"));
            }
        }
        Ok(children)
    }

    fn open_read(&self, path: &NativePath) -> io::Result<Reader<'_>> {
        match self.filesystem {
            ResourceFilesystem::Local => self
                .local
                .open_read(path)
                .map(|reader| Reader::Local(Box::new(reader)))
                .map_err(fs_error),
            ResourceFilesystem::Ssh(ssh) => {
                match ssh
                    .request(Request::OpenRead { path: path.clone() })
                    .map_err(fs_error)?
                {
                    Response::ReadOpened { session, revision } => Ok(Reader::Remote {
                        ssh,
                        session,
                        revision,
                        finished: false,
                    }),
                    _ => Err(invalid("unexpected_read_response")),
                }
            }
        }
    }
}

enum Reader<'a> {
    Local(Box<ReadSession>),
    Remote {
        ssh: &'a SshFilesystem,
        session: u64,
        revision: Revision,
        finished: bool,
    },
}

impl Reader<'_> {
    fn revision(&self) -> &Revision {
        match self {
            Self::Local(reader) => reader.revision(),
            Self::Remote { revision, .. } => revision,
        }
    }

    fn next(&mut self) -> io::Result<ReadChunk> {
        match self {
            Self::Local(reader) => reader.next_chunk(MAX_READ_BYTES).map_err(fs_error),
            Self::Remote {
                ssh,
                session,
                finished,
                ..
            } => match ssh
                .request(Request::NextRead {
                    session: *session,
                    length: MAX_READ_BYTES,
                })
                .map_err(fs_error)?
            {
                Response::Read(chunk) => {
                    *finished = chunk.eof;
                    Ok(chunk)
                }
                _ => Err(invalid("unexpected_read_chunk")),
            },
        }
    }

    fn failure(&self, error: io::Error) -> io::Error {
        match self {
            Self::Remote {
                ssh,
                session,
                finished: false,
                ..
            } => cancel_error(ssh, *session, error),
            _ => error,
        }
    }
}

enum Writer<'a> {
    Local(Box<StagedSave>),
    Remote {
        ssh: &'a SshFilesystem,
        session: u64,
    },
}

impl Writer<'_> {
    fn append(&mut self, chunk: ReadChunk) -> io::Result<()> {
        match self {
            Self::Local(writer) => writer.append(chunk.offset, &chunk.bytes).map_err(fs_error),
            Self::Remote { ssh, session } => unit(
                ssh.request(Request::AppendSave {
                    session: *session,
                    offset: chunk.offset,
                    bytes: chunk.bytes,
                })
                .map_err(fs_error)?,
            ),
        }
    }

    fn failure(&self, error: io::Error) -> io::Error {
        match self {
            Self::Remote { ssh, session } => cancel_error(ssh, *session, error),
            _ => error,
        }
    }
}

#[derive(Clone)]
struct Entry {
    source: NativePath,
    relative: Vec<NativePath>,
    metadata: Metadata,
    link: Option<NativePath>,
    revision: Option<Revision>,
}

fn transfer_checked(
    source_fs: &ResourceFilesystem,
    target_fs: &ResourceFilesystem,
    source: &ResourcePath,
    target_directory: &ResourcePath,
    move_source: bool,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<ResourcePath> {
    check()?;
    let input = Endpoint::new(source_fs, source)?;
    let output = Endpoint::new(target_fs, target_directory)?;
    let same_filesystem = source.filesystem == target_directory.filesystem;
    if let (ResourceFilesystem::Ssh(left), ResourceFilesystem::Ssh(right)) = (source_fs, target_fs)
    {
        if !same_filesystem {
            return Err(unsupported("different_ssh_identities"));
        }
        if !Arc::ptr_eq(left, right) {
            left.verify_peer(right).map_err(fs_error)?;
        }
    }
    let name = source
        .path
        .file_name()
        .ok_or_else(|| invalid("filesystem_root_transfer"))?;
    // Resolve ancestors but not the source's final symlink.
    let parent = source
        .path
        .parent()
        .ok_or_else(|| invalid("source_parent_required"))?;
    let parent = input.backend().canonicalize(&parent).map_err(fs_error)?;
    let source = source.with_path(parent.join_native_child(&name)?);
    let target_directory = target_directory.with_path(
        output
            .backend()
            .canonicalize(&target_directory.path)
            .map_err(fs_error)?,
    );
    check()?;
    if !output.metadata(&target_directory.path)?.is_dir {
        return Err(invalid("destination_not_directory"));
    }
    let metadata = input.metadata(&source.path)?;
    if same_filesystem && metadata.is_dir && at_or_below(&target_directory.path, &source.path) {
        return Err(invalid("destination_inside_source"));
    }
    if same_filesystem && move_source && parent == target_directory.path {
        return Ok(source);
    }
    let target = destination(&output, &name, &target_directory, metadata.is_file, check)?;
    if same_filesystem && move_source {
        check()?;
        match input.backend().rename(&source.path, &target.path) {
            Ok(()) => return Ok(target),
            Err(error) if error.kind == "CrossesDevices" || error.kind == "Unsupported" => {}
            Err(error) => return Err(retained_error(&source, &target, fs_error(error), true)),
        }
    }
    let mut entries = Vec::new();
    collect(&input, source.path.clone(), Vec::new(), &mut entries, check)?;
    // Validate every destination filename before creating anything.
    for entry in &entries {
        let destination = child_target(&target.path, &entry.relative)?;
        output.validate_link(entry, &destination)?;
    }
    let result = (|| {
        for entry in &mut entries {
            check()?;
            ensure_directory_chain(&input, &source.path, &entry.relative)?;
            ensure_directory_chain(&output, &target.path, &entry.relative)?;
            ensure_unchanged(&input, entry)?;
            let path = child_target(&target.path, &entry.relative)?;
            if let Some(link) = &entry.link {
                output
                    .backend()
                    .create_symlink(link, &path, entry.metadata.symlink_directory)
                    .map_err(fs_error)?;
            } else if entry.metadata.is_dir {
                output
                    .backend()
                    .create_dir(&path, false)
                    .map_err(fs_error)?;
            } else {
                entry.revision = Some(copy_file(&input, &output, &entry.source, &path, check)?);
            }
        }
        verify_tree(&input, &output, &entries, &source.path, &target.path, check)?;
        if move_source {
            check()?;
            // Never fall back to recursive permanent deletion if native Trash fails.
            match source_fs {
                ResourceFilesystem::Local => {
                    crate::platform::trash::move_to_trash(&source.local_path()?)
                        .map_err(io::Error::other)?;
                }
                ResourceFilesystem::Ssh(_) => {
                    input.backend().trash(&source.path).map_err(fs_error)?;
                }
            }
        }
        Ok(())
    })();
    result.map_err(|error| retained_error(&source, &target, error, move_source))?;
    Ok(target)
}

fn collect(
    input: &Endpoint<'_>,
    source: NativePath,
    relative: Vec<NativePath>,
    entries: &mut Vec<Entry>,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let mut pending = vec![(source, relative)];
    while let Some((source, relative)) = pending.pop() {
        check()?;
        if relative.len() > MAX_DEPTH || entries.len() >= MAX_ENTRIES {
            return Err(invalid("transfer_tree_limit_exceeded"));
        }
        let metadata = input.metadata(&source)?;
        let link = if metadata.is_symlink {
            Some(input.backend().read_link(&source).map_err(fs_error)?)
        } else {
            if !metadata.is_dir && !metadata.is_file {
                return Err(unsupported("unsupported_file_type"));
            }
            None
        };
        let directory = metadata.is_dir && !metadata.is_symlink;
        entries.push(Entry {
            source: source.clone(),
            relative: relative.clone(),
            metadata,
            link,
            revision: None,
        });
        if directory {
            let children = input.directory(&source, check)?;
            if entries.len() + pending.len() + children.len() > MAX_ENTRIES {
                return Err(invalid("transfer_tree_limit_exceeded"));
            }
            for child in children {
                let mut relative = relative.clone();
                relative.push(
                    child
                        .file_name()
                        .ok_or_else(|| invalid("invalid_child_name"))?,
                );
                pending.push((child, relative));
            }
        }
    }
    Ok(())
}

fn copy_file(
    input: &Endpoint<'_>,
    output: &Endpoint<'_>,
    source: &NativePath,
    target: &NativePath,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<Revision> {
    check()?;
    let mut reader = input.open_read(source)?;
    let result = (|| {
        let revision = reader.revision().clone();
        check()?;
        let operation_id = format!(
            "transfer-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_OPERATION.fetch_add(1, Ordering::Relaxed),
        );
        let mut writer = match output.filesystem {
            ResourceFilesystem::Local => Writer::Local(Box::new(
                output
                    .local
                    .begin_save(
                        operation_id,
                        target.clone(),
                        ExpectedRevision::Missing,
                        revision.size,
                    )
                    .map_err(fs_error)?,
            )),
            ResourceFilesystem::Ssh(ssh) => {
                let Response::Session(session) = ssh
                    .request(Request::BeginSave {
                        operation_id,
                        path: target.clone(),
                        expected: ExpectedRevision::Missing,
                        total_bytes: revision.size,
                    })
                    .map_err(fs_error)?
                else {
                    return Err(invalid("unexpected_staged_save_response"));
                };
                Writer::Remote { ssh, session }
            }
        };
        let stream = (|| {
            let mut offset = 0;
            loop {
                check()?;
                let chunk = reader.next()?;
                if chunk.offset != offset
                    || chunk.revision != revision
                    || chunk.bytes.len() > MAX_READ_BYTES
                    || chunk.bytes.is_empty() && !chunk.eof
                {
                    return Err(invalid("invalid_transfer_read_chunk"));
                }
                offset = offset
                    .checked_add(chunk.bytes.len() as u64)
                    .ok_or_else(|| invalid("transfer_read_offset_overflow"))?;
                let done = chunk.eof;
                if offset > revision.size || done && offset != revision.size {
                    return Err(invalid("incomplete_transfer_read"));
                }
                check()?;
                if !chunk.bytes.is_empty() {
                    writer.append(chunk)?;
                }
                if done {
                    break;
                }
            }
            check()
        })();
        if let Err(error) = stream {
            return Err(writer.failure(error));
        }
        let outcome = match writer {
            Writer::Local(writer) => output.local.commit_save(*writer).map_err(fs_error)?,
            Writer::Remote { ssh, session } => {
                match ssh
                    .request(Request::CommitSave { session })
                    .map_err(fs_error)?
                {
                    Response::Save(outcome) => outcome,
                    _ => return Err(io::Error::other(RuntimeText::TransferUncertain.to_string())),
                }
            }
        };
        match outcome {
            SaveOutcome::Saved(actual) if actual == revision => Ok(revision),
            SaveOutcome::Conflict(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                RuntimeText::TransferConflict.to_string(),
            )),
            SaveOutcome::Failed(error) => Err(fs_error(error)),
            SaveOutcome::Uncertain(error) => Err(io::Error::other(format!(
                "{}: {error}",
                RuntimeText::TransferUncertain
            ))),
            _ => Err(io::Error::other(RuntimeText::TransferUncertain.to_string())),
        }
    })();
    result.map_err(|error| reader.failure(error))
}

fn verify_tree(
    input: &Endpoint<'_>,
    output: &Endpoint<'_>,
    entries: &[Entry],
    source: &NativePath,
    target: &NativePath,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let mut current = Vec::new();
    collect(input, source.clone(), Vec::new(), &mut current, check)?;
    let expected: HashSet<_> = entries.iter().map(|entry| &entry.relative).collect();
    let actual: HashSet<_> = current.iter().map(|entry| &entry.relative).collect();
    if expected != actual {
        return Err(io::Error::other(format!(
            "{} [source_tree]",
            RuntimeText::TransferChanged
        )));
    }
    for entry in entries {
        check()?;
        ensure_directory_chain(input, source, &entry.relative)?;
        ensure_directory_chain(output, target, &entry.relative)?;
        ensure_unchanged(input, entry)?;
        let destination = child_target(target, &entry.relative)?;
        let metadata = output.metadata(&destination)?;
        if metadata.is_symlink != entry.metadata.is_symlink
            || metadata.symlink_directory != entry.metadata.symlink_directory
            || metadata.is_dir != entry.metadata.is_dir
            || metadata.is_file != entry.metadata.is_file
        {
            return Err(io::Error::other(format!(
                "{} [destination_type]",
                RuntimeText::TransferChanged
            )));
        }
        if let Some(link) = &entry.link
            && output.backend().read_link(&destination).map_err(fs_error)? != *link
        {
            return Err(io::Error::other(format!(
                "{} [destination_link]",
                RuntimeText::TransferChanged
            )));
        }
        if let Some(revision) = &entry.revision {
            verify_revision(input, &entry.source, revision, check)?;
            verify_revision(output, &destination, revision, check)?;
        }
    }
    // Recheck metadata after all potentially long content verification.
    for entry in entries {
        check()?;
        ensure_unchanged(input, entry)?;
    }
    Ok(())
}

fn verify_revision(
    endpoint: &Endpoint<'_>,
    path: &NativePath,
    expected: &Revision,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let mut reader = endpoint.open_read(path)?;
    let result = (|| {
        if reader.revision() != expected {
            return Err(io::Error::other(format!(
                "{} [file_content]",
                RuntimeText::TransferChanged
            )));
        }
        let mut offset = 0;
        loop {
            check()?;
            let chunk = reader.next()?;
            if chunk.offset != offset
                || &chunk.revision != expected
                || chunk.bytes.len() > MAX_READ_BYTES
                || chunk.bytes.is_empty() && !chunk.eof
            {
                return Err(invalid("invalid_verification_chunk"));
            }
            offset = offset
                .checked_add(chunk.bytes.len() as u64)
                .ok_or_else(|| invalid("verification_offset_overflow"))?;
            if offset > expected.size {
                return Err(invalid("oversized_verification_stream"));
            }
            if chunk.eof {
                return if offset == expected.size {
                    Ok(())
                } else {
                    Err(invalid("incomplete_verification_stream"))
                };
            }
        }
    })();
    result.map_err(|error| reader.failure(error))
}

fn ensure_unchanged(input: &Endpoint<'_>, entry: &Entry) -> io::Result<()> {
    let current = input.metadata(&entry.source)?;
    let before = &entry.metadata;
    if current.is_file != before.is_file
        || current.is_dir != before.is_dir
        || current.is_symlink != before.is_symlink
        || current.symlink_directory != before.symlink_directory
        || current.len != before.len
        || current.modified_unix_nanos != before.modified_unix_nanos
        || current.created_unix_nanos != before.created_unix_nanos
    {
        return Err(io::Error::other(format!(
            "{} [source]",
            RuntimeText::TransferChanged
        )));
    }
    if let Some(link) = &entry.link
        && input.backend().read_link(&entry.source).map_err(fs_error)? != *link
    {
        return Err(io::Error::other(format!(
            "{} [source_link]",
            RuntimeText::TransferChanged
        )));
    }
    Ok(())
}

fn ensure_directory_chain(
    endpoint: &Endpoint<'_>,
    root: &NativePath,
    relative: &[NativePath],
) -> io::Result<()> {
    let mut path = root.clone();
    for name in relative {
        let metadata = endpoint.metadata(&path)?;
        if !metadata.is_dir || metadata.is_symlink {
            return Err(io::Error::other(format!(
                "{} [ancestor_type]",
                RuntimeText::TransferChanged
            )));
        }
        path = join_name(&path, name)?;
    }
    Ok(())
}

fn destination(
    output: &Endpoint<'_>,
    name: &NativePath,
    directory: &ResourcePath,
    file: bool,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<ResourcePath> {
    for suffix in 0..MAX_COLLISIONS {
        check()?;
        let name = if suffix == 0 {
            name.clone()
        } else {
            name.numbered_name(suffix, file)
        };
        let target = directory.with_path(join_name(&directory.path, &name)?);
        match output.metadata(&target.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(target),
            Err(error) => return Err(error),
            Ok(_) => {}
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} [collision_limit]", RuntimeText::TransferConflict),
    ))
}

pub(super) fn join_name(parent: &NativePath, name: &NativePath) -> io::Result<NativePath> {
    match (parent, name) {
        (NativePath::Unix(_), NativePath::Windows(units)) => {
            let name = String::from_utf16(units).map_err(|_| invalid("nonportable_filename"))?;
            parent.join_child(&name)
        }
        (NativePath::Windows(_), NativePath::Unix(bytes)) => {
            let name = std::str::from_utf8(bytes).map_err(|_| invalid("nonportable_filename"))?;
            parent.join_child(name)
        }
        _ => parent.join_native_child(name),
    }
}

fn child_target(root: &NativePath, relative: &[NativePath]) -> io::Result<NativePath> {
    relative
        .iter()
        .try_fold(root.clone(), |parent, name| join_name(&parent, name))
}

fn at_or_below(path: &NativePath, root: &NativePath) -> bool {
    let mut current = Some(path.clone());
    while let Some(path) = current {
        if &path == root {
            return true;
        }
        current = path.parent();
    }
    false
}

fn unit(response: Response) -> io::Result<()> {
    match response {
        Response::Unit => Ok(()),
        _ => Err(invalid("unexpected_mutation_response")),
    }
}

fn cancel_error(ssh: &SshFilesystem, session: u64, original: io::Error) -> io::Error {
    match ssh
        .request(Request::Cancel { session })
        .map_err(fs_error)
        .and_then(unit)
    {
        Ok(()) => original,
        Err(error) => io::Error::new(
            original.kind(),
            format!(
                "{original}; {}: {error}",
                RuntimeText::TransferCleanupFailed
            ),
        ),
    }
}

fn fs_error(error: FsError) -> io::Error {
    let kind = match error.kind.as_str() {
        "NotFound" => io::ErrorKind::NotFound,
        "AlreadyExists" => io::ErrorKind::AlreadyExists,
        "PermissionDenied" => io::ErrorKind::PermissionDenied,
        "InvalidInput" => io::ErrorKind::InvalidInput,
        "InvalidData" => io::ErrorKind::InvalidData,
        "Unsupported" => io::ErrorKind::Unsupported,
        "TimedOut" => io::ErrorKind::TimedOut,
        "Interrupted" => io::ErrorKind::Interrupted,
        "WouldBlock" => io::ErrorKind::WouldBlock,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{} [{message}]", RuntimeText::FileAction),
    )
}

fn unsupported(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{} [{message}]", RuntimeText::FeatureUnavailable),
    )
}

fn retained_error(
    source: &ResourcePath,
    target: &ResourcePath,
    error: io::Error,
    moving: bool,
) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "{} '{}' -> '{}': {error}; {}",
            RuntimeText::FileAction,
            source.path,
            target.path,
            if moving {
                RuntimeText::TransferMoveRetained
            } else {
                RuntimeText::TransferCopyRetained
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use moeka_fs::local::native as fs;
    use std::cell::Cell;
    use std::path::Path;

    fn fixture() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(".resource-transfer-test-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap()
    }

    fn copy(source: &Path, target: &Path) -> io::Result<ResourcePath> {
        transfer(
            &ResourceFilesystem::Local,
            &ResourceFilesystem::Local,
            &ResourcePath::local(source),
            &ResourcePath::local(target),
            false,
        )
    }

    #[test]
    fn preflight_accepts_large_files_without_reading_or_allocating_their_contents() {
        let root = fixture();
        let source = root.path().join("large");
        fs::File::create(&source)
            .unwrap()
            .set_len(512 * 1024 * 1024 + 1)
            .unwrap();
        let resource = ResourcePath::local(&source);
        let endpoint = Endpoint::new(&ResourceFilesystem::Local, &resource).unwrap();
        let mut entries = Vec::new();
        collect(&endpoint, resource.path, Vec::new(), &mut entries, &|| {
            Ok(())
        })
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.len, 512 * 1024 * 1024 + 1);
        assert!(entries[0].revision.is_none());
    }

    #[test]
    fn nested_copy_streams_and_preserves_collisions() {
        let root = fixture();
        let source = root.path().join("notes");
        let destination = root.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir_all(destination.join("notes")).unwrap();
        let content = vec![b'x'; MAX_READ_BYTES * 3 + 19];
        fs::write(source.join("nested/large.md"), &content).unwrap();
        fs::write(source.join("empty"), []).unwrap();
        fs::write(destination.join("notes/keep"), b"keep").unwrap();
        let target = copy(&source, &destination).unwrap().local_path().unwrap();
        assert_eq!(target.file_name().unwrap(), "notes-1");
        assert_eq!(fs::read(target.join("nested/large.md")).unwrap(), content);
        assert_eq!(fs::read(destination.join("notes/keep")).unwrap(), b"keep");
        assert!(source.join("nested/large.md").is_file());
        fs::write(destination.join("large.md"), b"existing").unwrap();
        let target = copy(&source.join("nested/large.md"), &destination).unwrap();
        assert_eq!(
            target.local_path().unwrap().file_name().unwrap(),
            "large-1.md"
        );
        assert_eq!(fs::read(destination.join("large.md")).unwrap(), b"existing");
    }

    #[test]
    fn rejects_descendants_and_identity_mismatch_before_mutation() {
        let root = fixture();
        let source = root.path().join("notes");
        fs::create_dir_all(source.join("child")).unwrap();
        assert_eq!(
            copy(&source, &source.join("child")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            copy(&source, &source).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let wrong = ResourcePath {
            filesystem: FilesystemId::Ssh([0; 32]),
            path: NativePath::from_local(&source),
        };
        assert!(
            transfer(
                &ResourceFilesystem::Local,
                &ResourceFilesystem::Local,
                &wrong,
                &ResourcePath::local(root.path()),
                false
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(source.join("child")).unwrap().count(), 0);
    }

    #[test]
    fn raced_destination_and_missing_source_never_overwrite_or_remove() {
        let root = fixture();
        let source = root.path().join("draft.md");
        let target = root.path().join("copy.md");
        fs::write(&source, b"draft").unwrap();
        fs::write(&target, b"keep").unwrap();
        let endpoint =
            Endpoint::new(&ResourceFilesystem::Local, &ResourcePath::local(&source)).unwrap();
        let error = copy_file(
            &endpoint,
            &endpoint,
            &NativePath::from_local(&source),
            &NativePath::from_local(&target),
            &|| Ok(()),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        assert_eq!(fs::read(&source).unwrap(), b"draft");
        assert!(copy(&root.path().join("missing"), root.path()).is_err());
    }

    #[test]
    fn interrupted_tree_copy_retains_completed_destination_and_source() {
        let root = fixture();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("draft"), b"draft").unwrap();
        let checks = Cell::new(0);
        let error = transfer_checked(
            &ResourceFilesystem::Local,
            &ResourceFilesystem::Local,
            &ResourcePath::local(&source),
            &ResourcePath::local(&target),
            false,
            &|| {
                checks.set(checks.get() + 1);
                if target.join("source/draft").is_file() {
                    Err(io::Error::new(io::ErrorKind::TimedOut, "injected_deadline"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert!(checks.get() > 0);
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            error
                .to_string()
                .contains(&RuntimeText::TransferCopyRetained.to_string())
        );
        assert_eq!(fs::read(source.join("draft")).unwrap(), b"draft");
        assert_eq!(fs::read(target.join("source/draft")).unwrap(), b"draft");
    }

    #[test]
    fn changed_source_retains_both_versions_instead_of_completing_transfer() {
        let root = fixture();
        let source = root.path().join("draft");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(&source, b"original").unwrap();
        let changed = Cell::new(false);
        let result = transfer_checked(
            &ResourceFilesystem::Local,
            &ResourceFilesystem::Local,
            &ResourcePath::local(&source),
            &ResourcePath::local(&target),
            false,
            &|| {
                if target.join("draft").is_file() && !changed.replace(true) {
                    fs::write(&source, b"edited after copying")?;
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&source).unwrap(), b"edited after copying");
        assert_eq!(fs::read(target.join("draft")).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn preserves_dangling_links_and_rejects_aliased_descendants() {
        let root = fixture();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink("missing", source.join("link")).unwrap();
        copy(&source, &target).unwrap();
        assert_eq!(
            fs::read_link(target.join("source/link")).unwrap(),
            Path::new("missing")
        );
        std::os::unix::fs::symlink(&source, root.path().join("alias")).unwrap();
        assert_eq!(
            copy(&source, &root.path().join("alias"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn same_filesystem_move_uses_no_replace_rename() {
        let root = fixture();
        let source = root.path().join("draft.md");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(&source, b"draft").unwrap();
        fs::write(target.join("draft.md"), b"keep").unwrap();
        let moved = transfer(
            &ResourceFilesystem::Local,
            &ResourceFilesystem::Local,
            &ResourcePath::local(&source),
            &ResourcePath::local(&target),
            true,
        )
        .unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(moved.local_path().unwrap()).unwrap(), b"draft");
        assert_eq!(fs::read(target.join("draft.md")).unwrap(), b"keep");
    }
}
