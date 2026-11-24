use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use gpui::{AnyElement, Context, MouseButton, SharedString, Window, div, prelude::*, px};
use moeka_fs::{
    backend::{ExpectedRevision, Filesystem, Metadata, Revision, SaveOutcome},
    path::{FilesystemId, NativePath, ResourcePath},
    service::{Request, Response, SearchItem, SearchOptions},
};
use moeka_ssh::{SshOptions, bundle::Bundle, filesystem::SshFilesystem};

use super::*;

static NEXT_REMOTE_WORKSPACE_SERIAL: AtomicU64 = AtomicU64::new(0);

const MAX_REMOTE_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;

fn remote_metadata_time(nanos: Option<i128>) -> Option<std::time::SystemTime> {
    let nanos = nanos?;
    let seconds = i64::try_from(nanos.div_euclid(1_000_000_000)).ok()?;
    chrono::DateTime::from_timestamp(seconds, nanos.rem_euclid(1_000_000_000) as u32)
        .map(Into::into)
}

fn remote_file_list_entry(entry: &RemoteDirectoryEntry) -> bool {
    entry.metadata.is_file
        && !moeka_fs::search::should_skip_name(&entry.name, false)
        && moeka_fs::content::is_markdown_path(Path::new(&entry.name))
}

#[derive(Default)]
struct DirectoryLoadAttempts {
    failed: HashSet<ResourcePath>,
}

impl DirectoryLoadAttempts {
    fn should_start(
        &mut self,
        path: &ResourcePath,
        cached: bool,
        loading: bool,
        refresh: bool,
    ) -> bool {
        if loading {
            return false;
        }
        if refresh {
            self.failed.remove(path);
        }
        (!cached || refresh) && !self.failed.contains(path)
    }

    fn finish(&mut self, path: &ResourcePath, success: bool) {
        if success {
            self.failed.remove(path);
        } else {
            self.failed.insert(path.clone());
        }
    }
}

#[derive(Clone)]
pub(crate) struct RemoteDocument {
    pub resource: Option<ResourcePath>,
    pub revision: Option<Revision>,
    pub save_uncertain: bool,
}

#[derive(Clone)]
pub(crate) struct RemoteDirectoryEntry {
    pub resource: ResourcePath,
    pub name: String,
    pub metadata: Metadata,
}

#[derive(Clone)]
pub(crate) struct RemoteSearchResult {
    pub(super) resource: ResourcePath,
    pub(super) snippet: String,
    /// Zero-based line of the match.
    pub(super) line: usize,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// The server-side session feeding the workspace search panel.
#[derive(Clone)]
pub(crate) struct WorkspaceSearchRemote {
    pub(super) session: u64,
    pub(super) identity: FilesystemId,
    pub(super) root: ResourcePath,
    pub(super) generation: u64,
}

/// Remote paths are shown and grouped by the search panel as ordinary paths.
pub(super) fn remote_path_buf(path: &NativePath) -> PathBuf {
    match path {
        NativePath::Unix(bytes) => {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                PathBuf::from(std::ffi::OsString::from_vec(bytes.clone()))
            }
            #[cfg(not(unix))]
            {
                PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
            }
        }
        other => PathBuf::from(other.to_string()),
    }
}

pub(super) fn remote_native_path(path: &Path) -> NativePath {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        NativePath::Unix(path.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        NativePath::Unix(path.to_string_lossy().replace('\\', "/").into_bytes())
    }
}

#[derive(Clone)]
pub(crate) struct RemoteQuickOpenResult {
    pub resource: ResourcePath,
    pub relative: String,
    pub name: String,
    pub score: i64,
    pub name_matches: Vec<usize>,
    pub path_matches: Vec<usize>,
}

#[derive(Clone)]
struct RemoteFilePreview {
    modified: Option<i128>,
    len: u64,
    details: Result<super::file_list::FileListDetails, String>,
}

fn retain_remote_preview(
    previews: &mut HashMap<ResourcePath, RemoteFilePreview>,
    entry: &RemoteDirectoryEntry,
    details: Result<super::file_list::FileListDetails, String>,
) {
    let details = match details {
        Ok(details) => Ok(details),
        Err(error) => previews
            .get(&entry.resource)
            .and_then(|preview| preview.details.as_ref().ok())
            .cloned()
            .ok_or(error),
    };
    previews.insert(
        entry.resource.clone(),
        RemoteFilePreview {
            modified: entry.metadata.modified_unix_nanos,
            len: entry.metadata.len,
            details,
        },
    );
}

enum RemoteDocumentObservation {
    Missing,
    Present {
        text: Option<String>,
        revision: Revision,
    },
}

pub(crate) struct RemoteWorkspace {
    /// A replacement connection is being established after the link dropped.
    pub reconnecting: bool,
    /// Connection order; keeps sidebar rows put while workspaces swap slots.
    serial: u64,
    pub(super) peer_scope: Option<[u8; 32]>,
    pub filesystem: Arc<SshFilesystem>,
    pub background_filesystem: Arc<SshFilesystem>,
    pub root: ResourcePath,
    pub destination: String,
    pub config_file: Option<PathBuf>,
    pub directories: HashMap<ResourcePath, Vec<RemoteDirectoryEntry>>,
    pub expanded: HashSet<ResourcePath>,
    pub loading: HashSet<ResourcePath>,
    pub refreshing: HashSet<ResourcePath>,
    directory_attempts: DirectoryLoadAttempts,
    pub selected: Option<ResourcePath>,
    pub selected_paths: HashSet<ResourcePath>,
    pub selection_anchor: Option<ResourcePath>,
    pub focused: Option<ResourcePath>,
    pub error: Option<String>,
    pub watch_warning: Option<String>,
    pub connection_warning: Option<String>,
    pub watch_session: Option<u64>,
    pub watch_sequence: u64,
    sort_mode: SidebarSortMode,
    pub(super) file_list_dir: ResourcePath,
    pub(super) tree_scroll: gpui::ScrollHandle,
    pub(super) file_list_scroll: gpui::ScrollHandle,
    previews: HashMap<ResourcePath, RemoteFilePreview>,
    previews_loading: HashSet<ResourcePath>,
    previews_stale: HashSet<ResourcePath>,
    previews_waiting_refresh: HashSet<ResourcePath>,
    preview_reconciled: HashSet<ResourcePath>,
    image_requests: HashMap<String, u64>,
    next_image_request: u64,
    resource_epoch: u64,
    directory_epoch: u64,
    documents_to_refresh: HashSet<ResourcePath>,
    document_refresh_running: bool,
}

/// A saved SSH workspace that has not connected yet; `extra` indexes
/// `ssh_restore_more`, `None` is the primary entry.
#[derive(Clone)]
struct PendingSshWorkspace {
    extra: Option<usize>,
    destination: String,
    config_file: Option<PathBuf>,
    root: ResourcePath,
}

#[derive(Clone)]
pub(crate) enum RemoteInputMode {
    Connect,
    SaveAs(String),
}

pub(crate) struct RemoteInput {
    pub editor: gpui::Entity<crate::text_input::TextInput>,
    pub ssh_picker: Option<SshHostPicker>,
    pub mode: RemoteInputMode,
    pub error: Option<String>,
}

pub(crate) enum RemoteSidebarEditMode {
    CreateFile {
        parent: ResourcePath,
        editor: gpui::Entity<crate::text_input::TextInput>,
    },
    CreateDirectory {
        parent: ResourcePath,
        editor: gpui::Entity<crate::text_input::TextInput>,
    },
    Rename {
        resource: ResourcePath,
        editor: gpui::Entity<crate::text_input::TextInput>,
    },
}

#[derive(Default)]
pub(crate) struct SshHostPicker {
    hosts: Vec<String>,
    selected_index: usize,
    scroll_handle: gpui::ScrollHandle,
    destination: Option<String>,
    config_file: Option<PathBuf>,
    loading: bool,
    connecting: bool,
    connection_generation: u64,
    browsing_filesystem: Option<Arc<SshFilesystem>>,
    directory_query_generation: u64,
    directory_query_value: String,
    directory_loading: bool,
    directories: Vec<String>,
    #[cfg(test)]
    disable_browsing_for_test: bool,
}

fn ssh_host_choices(hosts: &[String], query: &str) -> Vec<String> {
    let query = query.trim();
    let folded = query.to_lowercase();
    let mut choices = hosts
        .iter()
        .filter(|host| host.to_lowercase().contains(&folded))
        .cloned()
        .collect::<Vec<_>>();
    if !query.is_empty() && !hosts.iter().any(|host| host.eq_ignore_ascii_case(query)) {
        choices.push(query.to_string());
    }
    choices
}

fn ssh_directory_query(value: &str) -> Option<(&str, &str)> {
    if !value.starts_with('/') || value.contains('\0') {
        return None;
    }
    let separator = value.rfind('/')?;
    let parent = if separator == 0 {
        "/"
    } else {
        &value[..separator]
    };
    Some((parent, &value[separator + 1..]))
}

type RemoteConnection = (
    Arc<SshFilesystem>,
    Arc<SshFilesystem>,
    ResourcePath,
    Vec<RemoteDirectoryEntry>,
    Option<u64>,
    Option<String>,
    Option<[u8; 32]>,
);
pub(super) fn scheduler_key() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // This local-only token is used by fs_task admission, never as a remote path.
    PathBuf::from(format!(
        "moeka-ssh-task-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn next_remote_save_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    static START: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let start = START.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    });
    format!(
        "moeka-{}-{start}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn schedule_remote_filesystem_release(filesystem: Arc<SshFilesystem>, cx: &mut gpui::AsyncApp) {
    let filesystem = Arc::new(Mutex::new(Some(filesystem)));
    cx.spawn(async move |cx| {
        loop {
            let pending = filesystem.clone();
            let _ = crate::platform::fs_task::run(
                "SSH transport cleanup",
                scheduler_key(),
                move || {
                    let filesystem = pending
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .take();
                    drop(filesystem);
                    Ok(())
                },
            )
            .await;
            if filesystem
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_none()
            {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
        }
    })
    .detach();
}

pub(super) async fn cancel_remote_session(filesystem: Arc<SshFilesystem>, session: Option<u64>) {
    if let Some(session) = session {
        let _ = crate::platform::fs_task::run("SSH session cleanup", scheduler_key(), move || {
            let _ = filesystem.request(Request::Cancel { session });
            Ok(())
        })
        .await;
    }
}

fn fs_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn decode_remote_text(bytes: Vec<u8>) -> io::Result<String> {
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn remote_uses_crlf(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.contains(&b'\n')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte != b'\n' || index > 0 && bytes[index - 1] == b'\r')
}

fn serialize_remote_text(text: &str, crlf: bool) -> Vec<u8> {
    if crlf {
        text.replace("\r\n", "\n")
            .replace('\n', "\r\n")
            .into_bytes()
    } else {
        text.as_bytes().to_vec()
    }
}

pub(super) fn remote_relative_resource(
    source: &ResourcePath,
    raw: &str,
) -> io::Result<ResourcePath> {
    let raw = raw.split(['?', '#']).next().unwrap_or(raw);
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let raw = decoded.trim();
    if raw.starts_with('/') {
        return Ok(source.with_path(NativePath::unix_absolute(raw)?));
    }
    if raw.is_empty() || raw.contains("://") || raw.starts_with('~') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::runtime::RuntimeText::InvalidOutput.to_string(),
        ));
    }
    let mut path = source.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::runtime::RuntimeText::InvalidOutput.to_string(),
        )
    })?;
    for component in raw.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                path = path.parent().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        crate::i18n::runtime::RuntimeText::InvalidOutput.to_string(),
                    )
                })?;
            }
            child => path = path.join_child(child)?,
        }
    }
    Ok(path)
}

pub(super) fn remote_relative_link(source: &ResourcePath, target: &ResourcePath) -> Option<String> {
    if source.filesystem != target.filesystem {
        return None;
    }
    let mut ancestors = HashMap::new();
    let mut current = source.parent()?;
    let mut up = 0;
    loop {
        ancestors.insert(current.clone(), up);
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
        up += 1;
    }
    let mut parts = Vec::new();
    let mut current = target.clone();
    while !ancestors.contains_key(&current) {
        let NativePath::Unix(name) = current.path.file_name()? else {
            return None;
        };
        parts.push(String::from_utf8(name).ok()?);
        current = current.parent()?;
    }
    parts.extend(std::iter::repeat_n("..".to_string(), ancestors[&current]));
    parts.reverse();
    Some(parts.join("/"))
}

pub(super) fn read_remote_relative_file(
    filesystem: &SshFilesystem,
    source: &ResourcePath,
    root: &ResourcePath,
    raw: &str,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let resource = remote_relative_resource(source, raw)?;
    match filesystem.read_file(&resource.path, limit) {
        Ok((bytes, _)) => Ok(bytes),
        Err(error) if error.kind == "NotFound" && !raw.starts_with('/') => {
            let root_source = root.join_child(".moeka-root-reference")?;
            let fallback = remote_relative_resource(&root_source, raw)?;
            filesystem
                .read_file(&fallback.path, limit)
                .map(|(bytes, _)| bytes)
                .map_err(fs_error)
        }
        Err(error) => Err(fs_error(error)),
    }
}

fn remote_directory(
    filesystem: &SshFilesystem,
    path: &ResourcePath,
    language: UiLanguage,
) -> io::Result<Vec<RemoteDirectoryEntry>> {
    let response = filesystem
        .request(Request::OpenDirectory {
            path: path.path.clone(),
        })
        .map_err(fs_error)?;
    let Response::Session(session) = response else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            crate::i18n::remote_text(language, "directory_response_error"),
        ));
    };
    let result = (|| -> io::Result<Vec<RemoteDirectoryEntry>> {
        let mut entries = Vec::new();
        loop {
            let (page, done) = filesystem.next_directory_pages(session).map_err(fs_error)?;
            for entry in page {
                crate::platform::fs_task::check_deadline()?;
                let entry = entry.map_err(fs_error)?;
                let metadata = entry.metadata.map_err(fs_error)?;
                let resource = path.with_path(entry.path.clone());
                let name = entry
                    .path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| entry.path.to_string());
                entries.push(RemoteDirectoryEntry {
                    resource,
                    name,
                    metadata,
                });
            }
            if done {
                break;
            }
        }
        Ok(entries)
    })();
    if result.is_err() {
        let _ = filesystem.request(Request::Cancel { session });
    }
    let mut entries = result?;
    entries.sort_by(|left, right| {
        right
            .metadata
            .is_dir
            .cmp(&left.metadata.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(entries)
}

fn sort_remote_entries(entries: &mut Vec<RemoteDirectoryEntry>, mode: SidebarSortMode) {
    let mut keyed = std::mem::take(entries)
        .into_iter()
        .map(|entry| {
            let key = super::sidebar_tree::DirectorySortKey::for_name(
                &entry.name,
                entry.metadata.is_dir,
                entry
                    .metadata
                    .created_unix_nanos
                    .or(entry.metadata.modified_unix_nanos)
                    .unwrap_or(0),
            );
            (key, entry)
        })
        .collect::<Vec<_>>();
    keyed.sort_by(|(left, _), (right, _)| left.compare(right, mode));
    *entries = keyed.into_iter().map(|(_, entry)| entry).collect();
}

fn open_remote_filesystem(
    destination: String,
    config_file: Option<PathBuf>,
    language: UiLanguage,
) -> io::Result<Arc<SshFilesystem>> {
    let options = SshOptions {
        destination: destination.clone(),
        port: None,
        config_file,
    };
    let bytes = crate::embedded_asset("remote/servers.mrb").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            crate::i18n::remote_text(language, "bundle_missing"),
        )
    })?;
    let bundle = Bundle::parse(bytes).map_err(fs_error)?;
    bundle
        .validate_identity(super::APP_VERSION, env!("MOEKA_GIT_SOURCE_SHA"))
        .map_err(fs_error)?;
    let target = moeka_ssh::detect_target(&options)?;
    let payload = bundle.payload(target).map_err(fs_error)?;
    let expected_build = bundle.manifest().build.clone();
    Ok(Arc::new(
        SshFilesystem::connect(&options, &payload.as_payload(), Some(expected_build))
            .map_err(fs_error)?,
    ))
}

fn remote_connection(
    destination: String,
    remote_root: NativePath,
    config_file: Option<PathBuf>,
    language: UiLanguage,
) -> io::Result<RemoteConnection> {
    // The two channels bootstrap independently; their SSH round trips overlap.
    let (filesystem, background) = std::thread::scope(|scope| {
        let background = scope
            .spawn(|| open_remote_filesystem(destination.clone(), config_file.clone(), language));
        let filesystem = open_remote_filesystem(destination.clone(), config_file.clone(), language);
        let background = background
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (filesystem, background)
    });
    let filesystem = filesystem?;
    let (background_filesystem, channel_error) = match background {
        Ok(background) => match filesystem.verify_peer(&background) {
            Ok(()) => (background, None),
            Err(error) => (filesystem.clone(), Some(error.to_string())),
        },
        Err(error) => (filesystem.clone(), Some(error.to_string())),
    };
    let root = ResourcePath {
        filesystem: filesystem.identity().clone(),
        path: remote_root,
    };
    let canonical_root = filesystem.canonicalize(&root.path).map_err(fs_error)?;
    let root = root.with_path(canonical_root);
    let metadata = filesystem.metadata(&root.path, true).map_err(fs_error)?;
    if !metadata.is_dir {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::remote_text(language, "root_not_directory"),
        ));
    }
    let entries = remote_directory(&background_filesystem, &root, language)?;
    let watch = match background_filesystem.request(Request::StartWatch {
        root: root.path.clone(),
        generation: 1,
        recursive: true,
    }) {
        Ok(Response::Session(session)) => Some(session),
        Ok(_) => None,
        Err(_) => None,
    };
    let peer_scope = match filesystem.peer_identity() {
        Ok(scope) => Some(scope),
        Err(_) if channel_error.is_some() => None,
        Err(error) => return Err(fs_error(error)),
    };
    Ok((
        // Scope continuity supplements (never replaces) strict SSH host verification.
        filesystem,
        background_filesystem,
        root,
        entries,
        watch,
        channel_error,
        peer_scope,
    ))
}

pub(super) fn start_remote_search(
    filesystem: &SshFilesystem,
    root: &ResourcePath,
    options: SearchOptions,
    filters: moeka_fs::search::SearchPathFilters,
    language: UiLanguage,
) -> io::Result<(u64, Vec<RemoteSearchResult>, bool)> {
    let request = if filesystem
        .capabilities()
        .iter()
        .any(|capability| capability == "search_path_filters")
    {
        Request::StartFilteredSearch {
            root: root.path.clone(),
            options,
            filters,
        }
    } else {
        Request::StartSearch {
            root: root.path.clone(),
            options,
        }
    };
    let response = filesystem.request(request).map_err(fs_error)?;
    let Response::Session(session) = response else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            crate::i18n::remote_text(language, "search_response_error"),
        ));
    };
    match next_remote_search_page(filesystem, root, session) {
        Ok((results, done)) => Ok((session, results, done)),
        Err(error) => {
            let _ = filesystem.request(Request::Cancel { session });
            Err(error)
        }
    }
}

pub(super) fn next_remote_search_page(
    filesystem: &SshFilesystem,
    root: &ResourcePath,
    session: u64,
) -> io::Result<(Vec<RemoteSearchResult>, bool)> {
    let (items, done) = filesystem.next_search_pages(session).map_err(fs_error)?;
    let mut results = Vec::new();
    for item in items {
        crate::platform::fs_task::check_deadline()?;
        match item {
            SearchItem::Match {
                path,
                snippet,
                line,
                start,
                end,
                ..
            } => {
                results.push(RemoteSearchResult {
                    resource: root.with_path(path),
                    snippet,
                    line,
                    start,
                    end,
                });
            }
            SearchItem::Error(error) => return Err(fs_error(error)),
        }
    }
    Ok((results, done))
}

pub(super) fn filter_remote_search_results(
    root: &ResourcePath,
    filter: &super::workspace_search::SearchPathFilter,
    results: &mut Vec<RemoteSearchResult>,
) {
    results.retain(|result| {
        remote_relative_display(root, &result.resource)
            .is_some_and(|path| filter.allows_relative(&path))
    });
}

fn remote_relative_display(root: &ResourcePath, resource: &ResourcePath) -> Option<String> {
    if resource.filesystem != root.filesystem {
        return None;
    }
    let mut path = resource.clone();
    let mut parts = Vec::new();
    while path != *root {
        let NativePath::Unix(name) = path.path.file_name()? else {
            return None;
        };
        parts.push(String::from_utf8_lossy(&name).into_owned());
        path = path.parent()?;
    }
    parts.reverse();
    Some(parts.join("/"))
}

pub(super) fn resource_key(resource: &ResourcePath) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    resource.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn remote_image_cache_key(resource: &ResourcePath) -> String {
    format!("ssh-image-{}", resource_key(resource))
}

enum RemoteOpenContent {
    Text(String),
    Image(Arc<gpui::RenderImage>),
    Pdf,
}

fn remote_state_matches_workspace(
    state: &config_store::RemoteTabState,
    destination: &str,
    root: &ResourcePath,
) -> bool {
    state.destination == destination && &state.root == root
}

fn pending_remote_save(
    state: &config_store::RemoteTabState,
    resource: &ResourcePath,
) -> config_store::RemoteTabState {
    let mut pending = state.clone();
    pending.save_uncertain = true;
    pending.pending_resource = Some(resource.clone());
    pending
}

fn remote_watch_affects_directory(directory: &ResourcePath, paths: &[NativePath]) -> bool {
    paths.iter().any(|path| {
        let changed = directory.with_path(path.clone());
        changed.parent().as_ref() == Some(directory) || resource_is_at_or_below(directory, &changed)
    })
}

fn resource_is_at_or_below(resource: &ResourcePath, root: &ResourcePath) -> bool {
    if resource.filesystem != root.filesystem {
        return false;
    }
    let mut current = Some(resource.clone());
    while let Some(path) = current {
        if &path == root {
            return true;
        }
        current = path.parent();
    }
    false
}

fn rebase_resource(
    resource: &ResourcePath,
    source: &ResourcePath,
    target: &ResourcePath,
) -> Option<ResourcePath> {
    let mut current = resource.clone();
    let mut components = Vec::new();
    while current != *source {
        components.push(current.path.file_name()?);
        current = current.parent()?;
    }
    let mut rebased = target.clone();
    for component in components.into_iter().rev() {
        rebased = rebased.join_native_child(&component).ok()?;
    }
    Some(rebased)
}

fn parse_connect_input(value: &str, language: UiLanguage) -> io::Result<(String, NativePath)> {
    let value = value.trim();
    let Some(separator) = value.find(char::is_whitespace) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::remote_text(language, "connect_instruction"),
        ));
    };
    let destination = value[..separator].trim();
    let path = value[separator..].trim();
    if destination.is_empty() || path.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::remote_text(language, "connect_instruction"),
        ));
    }
    Ok((destination.to_string(), NativePath::unix_absolute(path)?))
}

fn remote_input_placeholder(mode: &RemoteInputMode, language: UiLanguage) -> String {
    match mode {
        RemoteInputMode::Connect => crate::i18n::remote_text(language, "host_placeholder"),
        RemoteInputMode::SaveAs(_) => crate::i18n::remote_text(language, "save_as_placeholder"),
    }
}

impl EditorView {
    pub(super) fn run_remote_sidebar_filter(
        &mut self,
        cancel: Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        let query = self.sidebar_filter_query().to_owned();
        self.remote_sidebar_filter_epoch = self.remote_sidebar_filter_epoch.wrapping_add(1);
        let epoch = self.remote_sidebar_filter_epoch;
        self.remote_sidebar_filter_task = None;
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        if query.is_empty() {
            self.remote_sidebar_filter_rows.clear();
            self.remote_sidebar_filter_collapsed.clear();
            cx.notify();
            return;
        }
        let filesystem = workspace.background_filesystem.clone();
        let root = workspace.root.clone();
        let generation = self.remote_generation;
        let language = self.ui_language;
        let sort_mode = self.sidebar_sort_mode;
        self.remote_sidebar_filter_task = Some(cx.spawn(async move |this, cx| {
            let work_cancel = cancel.clone();
            let work_root = root.clone();
            let result = crate::platform::fs_task::run_background(
                "SSH sidebar filter",
                scheduler_key(),
                move || -> io::Result<Option<Vec<(RemoteDirectoryEntry, usize)>>> {
                    let mut children = remote_directory(&filesystem, &work_root, language)?;
                    sort_remote_entries(&mut children, sort_mode);
                    let mut pending = children
                        .into_iter()
                        .rev()
                        .map(|entry| (entry, 0))
                        .collect::<Vec<_>>();
                    let mut rows = Vec::new();
                    while let Some((entry, depth)) = pending.pop() {
                        if work_cancel.load(Ordering::Relaxed) {
                            return Ok(None);
                        }
                        crate::platform::fs_task::check_deadline()?;
                        if entry.metadata.is_dir && !entry.metadata.is_symlink {
                            let mut children =
                                remote_directory(&filesystem, &entry.resource, language)?;
                            sort_remote_entries(&mut children, sort_mode);
                            pending
                                .extend(children.into_iter().rev().map(|child| (child, depth + 1)));
                        }
                        rows.push((entry, depth));
                    }
                    let names = rows
                        .iter()
                        .map(|(entry, _)| entry.name.to_lowercase())
                        .collect::<Vec<_>>();
                    Ok(super::sidebar_filter::filter_preorder(
                        rows.len(),
                        &query,
                        &work_cancel,
                        |index| (rows[index].1, rows[index].0.metadata.is_dir, &names[index]),
                    )
                    .map(|keep| {
                        rows.into_iter()
                            .zip(keep)
                            .filter_map(|(row, keep)| keep.then_some(row))
                            .collect()
                    }))
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                if cancel.load(Ordering::Relaxed)
                    || editor.remote_generation != generation
                    || editor.remote_sidebar_filter_epoch != epoch
                {
                    return;
                }
                let Some(workspace) = editor.remote_workspace.as_mut() else {
                    return;
                };
                if workspace.root != root {
                    return;
                }
                match result {
                    Ok(Some(rows)) => {
                        editor.remote_sidebar_filter_rows = rows;
                        workspace.error = None;
                    }
                    Ok(None) => return,
                    Err(error) => workspace.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub(super) fn run_remote_quick_open_search(&mut self, cx: &mut Context<Self>) {
        if !self.quick_open_visible {
            return;
        }
        let Some(workspace) = self.remote_workspace.as_ref() else {
            self.remote_quick_open_results.clear();
            cx.notify();
            return;
        };
        if self.quick_open_task.is_some() {
            self.remote_quick_open_results.clear();
            cx.notify();
            return;
        }
        let filesystem = workspace.background_filesystem.clone();
        let root = workspace.root.clone();
        let generation = self.remote_generation;
        let query = self.quick_open_query.clone();
        let language = self.ui_language;
        self.quick_open_epoch = self.quick_open_epoch.wrapping_add(1);
        let epoch = self.quick_open_epoch;
        let task = cx.spawn(async move |this, cx| {
            let search_query = query.clone();
            let results = crate::platform::fs_task::run_background(
                "SSH quick open",
                scheduler_key(),
                move || -> io::Result<Vec<RemoteQuickOpenResult>> {
                    let mut pending = vec![root.clone()];
                    let mut results = Vec::new();
                    while let Some(directory) = pending.pop() {
                        crate::platform::fs_task::check_deadline()?;
                        for entry in remote_directory(&filesystem, &directory, language)? {
                            if entry.metadata.is_symlink {
                                continue;
                            }
                            let Some(NativePath::Unix(name)) = entry.resource.path.file_name()
                            else {
                                continue;
                            };
                            let valid_name = std::str::from_utf8(&name).ok();
                            if valid_name.is_some_and(|name| {
                                name.starts_with('.')
                                    || entry.metadata.is_dir
                                        && matches!(
                                            name,
                                            "node_modules"
                                                | "target"
                                                | "dist"
                                                | "build"
                                                | "coverage"
                                        )
                            }) {
                                continue;
                            }
                            if entry.metadata.is_dir {
                                pending.push(entry.resource);
                            } else if entry.metadata.is_file {
                                let Some(relative) =
                                    remote_relative_display(&root, &entry.resource)
                                else {
                                    continue;
                                };
                                let (name_score, name_matches) =
                                    super::quick_open::fuzzy_match(&entry.name, &search_query)
                                        .unwrap_or_default();
                                let (path_score, path_matches) =
                                    super::quick_open::fuzzy_match(&relative, &search_query)
                                        .unwrap_or_default();
                                if !search_query.is_empty() && name_score == 0 && path_score == 0 {
                                    continue;
                                }
                                results.push(RemoteQuickOpenResult {
                                    resource: entry.resource,
                                    relative,
                                    name: entry.name,
                                    score: name_score * 100 + path_score,
                                    name_matches,
                                    path_matches,
                                });
                            }
                        }
                    }
                    results.sort_by(|left, right| {
                        right
                            .score
                            .cmp(&left.score)
                            .then_with(|| {
                                left.relative
                                    .matches('/')
                                    .count()
                                    .cmp(&right.relative.matches('/').count())
                            })
                            .then_with(|| left.relative.cmp(&right.relative))
                    });
                    Ok(results)
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                if editor.quick_open_epoch != epoch {
                    return;
                }
                editor.quick_open_task = None;
                if !editor.quick_open_visible || editor.remote_generation != generation {
                    return;
                }
                if editor.quick_open_query != query {
                    editor.run_remote_quick_open_search(cx);
                    return;
                }
                match results {
                    Ok(results) => editor.remote_quick_open_results = results,
                    Err(error) => {
                        editor.remote_quick_open_results.clear();
                        editor.notify_error(error.to_string(), cx);
                    }
                }
                editor.quick_open_selected_index = 0;
                editor.quick_open_render_limit =
                    super::list_viewport::SEARCH_LIST_POLICY.initial_count;
                editor.quick_open_scroll_handle.scroll_to_item(0);
                cx.notify();
            })
            .ok();
        });
        self.quick_open_task = Some(task);
    }

    pub(super) fn copy_remote_image(&mut self, raw: Option<String>, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let tab = &self.tabs[index];
        let Some(source) = tab.remote.as_ref().and_then(|state| state.resource.clone()) else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "remote_binding_lost"),
                cx,
            );
            return;
        };
        let Some(workspace) = self.remote_workspace.as_ref().filter(|workspace| {
            !tab.remote_binding_lost && workspace.filesystem.identity() == &source.filesystem
        }) else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "remote_binding_lost"),
                cx,
            );
            return;
        };
        let filesystem = workspace.filesystem.clone();
        let root = workspace.root.clone();
        let generation = self.remote_generation;
        let pane = self.current_pane_id();
        let tab_id = self.active_tab_id.clone();
        cx.spawn(async move |this, cx| {
            let result = crate::platform::fs_task::run_background(
                "SSH image copy",
                scheduler_key(),
                move || {
                    let (bytes, extension) = if let Some(raw) = raw {
                        let bytes = read_remote_relative_file(
                            &filesystem,
                            &source,
                            &root,
                            &raw,
                            64 * 1024 * 1024,
                        )?;
                        let extension = Path::new(&raw)
                            .extension()
                            .and_then(|value| value.to_str())
                            .map(str::to_string);
                        (bytes, extension)
                    } else {
                        let bytes = filesystem
                            .read_file(&source.path, 64 * 1024 * 1024)
                            .map_err(fs_error)?
                            .0;
                        let name = source
                            .path
                            .file_name()
                            .map(|name| name.to_string())
                            .unwrap_or_default();
                        let extension = Path::new(&name)
                            .extension()
                            .and_then(|value| value.to_str())
                            .map(str::to_string);
                        (bytes, extension)
                    };
                    let decoded = helpers::decode_image_rgba(&bytes, extension.as_deref())
                        .map_err(fs_error)?;
                    let mut png = std::io::Cursor::new(Vec::new());
                    decoded
                        .write_to(&mut png, image::ImageFormat::Png)
                        .map_err(fs_error)?;
                    Ok(gpui::Image::from_bytes(
                        gpui::ImageFormat::Png,
                        png.into_inner(),
                    ))
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation {
                    return;
                }
                editor.with_pane_if_alive(pane, |editor| {
                    if editor.active_tab_id != tab_id {
                        return;
                    }
                    match result {
                        Ok(image) => cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image)),
                        Err(error) => editor.notify_error(error.to_string(), cx),
                    }
                    cx.notify();
                });
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn import_remote_attachments(
        &mut self,
        images: Vec<gpui::Image>,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.import_remote_attachments_for_line(images, paths, None, cx);
    }

    pub(super) fn replace_remote_image_for_line(
        &mut self,
        line: usize,
        original: String,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.import_remote_attachments_for_line(Vec::new(), vec![path], Some((line, original)), cx);
    }

    fn import_remote_attachments_for_line(
        &mut self,
        images: Vec<gpui::Image>,
        paths: Vec<PathBuf>,
        replacement: Option<(usize, String)>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let tab = &self.tabs[index];
        let Some(source) = tab.remote.as_ref().and_then(|state| state.resource.clone()) else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "remote_binding_lost"),
                cx,
            );
            return;
        };
        let Some(workspace) = self.remote_workspace.as_ref().filter(|workspace| {
            !tab.remote_binding_lost && workspace.filesystem.identity() == &source.filesystem
        }) else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "remote_binding_lost"),
                cx,
            );
            return;
        };
        let policy = self.app_settings.attachment_policy();
        if matches!(
            policy.location,
            attachments::AttachmentLocation::ExternalFolder
        ) || matches!(policy.path_style, attachments::AttachmentPathStyle::FileUrl)
        {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "attachment_unavailable"),
                cx,
            );
            return;
        }
        let base = match policy.location {
            attachments::AttachmentLocation::CurrentFile
            | attachments::AttachmentLocation::CurrentFileSubfolder => source.parent(),
            attachments::AttachmentLocation::WorkspaceRoot
            | attachments::AttachmentLocation::WorkspaceFolder => Some(workspace.root.clone()),
            attachments::AttachmentLocation::ExternalFolder => None,
        };
        let Some(mut directory) = base else { return };
        let folder = if let Some(template) = policy.folder_template.as_deref() {
            let filename = source.path.file_name().and_then(|name| match name {
                NativePath::Unix(name) => String::from_utf8(name).ok(),
                _ => None,
            });
            let dirname = source
                .parent()
                .and_then(|path| path.path.file_name())
                .and_then(|name| match name {
                    NativePath::Unix(name) => String::from_utf8(name).ok(),
                    _ => None,
                });
            let expanded =
                attachments::expand_folder_template(template, |variable| match variable {
                    "fileName" => filename
                        .as_deref()
                        .map(|name| name.rsplit_once('.').map_or(name, |(stem, _)| stem))
                        .map(str::to_string)
                        .ok_or(attachments::AttachmentTemplateError::FileName),
                    _ => dirname
                        .clone()
                        .ok_or(attachments::AttachmentTemplateError::FolderName),
                });
            match expanded {
                Ok(path) => path
                    .iter()
                    .filter_map(|component| component.to_str())
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                Err(_) => {
                    self.notify_error(
                        crate::i18n::remote_text(self.ui_language, "save_path_error"),
                        cx,
                    );
                    return;
                }
            }
        } else if matches!(
            policy.location,
            attachments::AttachmentLocation::WorkspaceFolder
                | attachments::AttachmentLocation::CurrentFileSubfolder
        ) {
            if policy.folder.starts_with('/') || policy.folder.contains('\\') {
                self.notify_error(
                    crate::i18n::remote_text(self.ui_language, "save_path_error"),
                    cx,
                );
                return;
            }
            policy
                .folder
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };
        for component in folder {
            match directory.join_child(&component) {
                Ok(path) => directory = path,
                Err(error) => {
                    self.notify_error(error.to_string(), cx);
                    return;
                }
            }
        }
        let root = workspace.root.clone();
        let filesystem = workspace.filesystem.clone();
        let generation = self.remote_generation;
        let pane = self.current_pane_id();
        let tab_id = self.active_tab_id.clone();
        let cursor = self.cursor.offset;
        let is_replacement = replacement.is_some();
        let language = self.ui_language;
        cx.spawn(async move |this, cx| {
            let result = crate::platform::fs_task::run_write(
                "SSH attachment import",
                scheduler_key(),
                move || -> io::Result<String> {
                    use std::io::Read;
                    filesystem
                        .create_dir(&directory.path, true)
                        .map_err(fs_error)?;
                    let mut files = Vec::new();
                    let mut total_bytes = 0usize;
                    for image in images {
                        let image = super::normalize_clipboard_image(&image);
                        total_bytes = total_bytes.saturating_add(image.bytes.len());
                        if total_bytes > MAX_REMOTE_DOCUMENT_BYTES {
                            return Err(io::Error::other(
                                crate::i18n::runtime::RuntimeText::ImagesTooLarge,
                            ));
                        }
                        files.push((
                            format!("image.{}", super::image_extension(image.format)),
                            image.bytes,
                            true,
                        ));
                    }
                    for path in paths {
                        let name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    crate::i18n::remote_text(language, "name_required"),
                                )
                            })?
                            .to_string();
                        let mut bytes = Vec::new();
                        moeka_fs::local::native::File::open(&path)?
                            .take(MAX_REMOTE_DOCUMENT_BYTES as u64 + 1)
                            .read_to_end(&mut bytes)?;
                        if bytes.len() > MAX_REMOTE_DOCUMENT_BYTES {
                            return Err(io::Error::other(
                                crate::i18n::runtime::RuntimeText::ImageTooLarge,
                            ));
                        }
                        total_bytes = total_bytes.saturating_add(bytes.len());
                        if total_bytes > MAX_REMOTE_DOCUMENT_BYTES {
                            return Err(io::Error::other(
                                crate::i18n::runtime::RuntimeText::ImagesTooLarge,
                            ));
                        }
                        files.push((name, bytes, super::is_image_path(&path)));
                    }
                    let mut links = Vec::new();
                    for (name, bytes, is_image) in files {
                        crate::platform::fs_task::check_deadline()?;
                        let id = next_remote_save_id();
                        let unique = format!("{id}-{name}");
                        let target = directory.join_child(&unique)?;
                        match filesystem
                            .save_bytes(id, target.path.clone(), ExpectedRevision::Missing, &bytes)
                            .map_err(fs_error)?
                        {
                            SaveOutcome::Saved(_) => {}
                            SaveOutcome::Conflict(_) => {
                                return Err(io::Error::other(crate::i18n::remote_text(
                                    language,
                                    "save_conflict",
                                )));
                            }
                            SaveOutcome::Uncertain(error) => {
                                return Err(io::Error::other(format!(
                                    "{}: {error}",
                                    crate::i18n::remote_text(language, "save_uncertain")
                                )));
                            }
                            SaveOutcome::Failed(error) => return Err(fs_error(error)),
                            SaveOutcome::InProgress | SaveOutcome::Unknown => {
                                return Err(io::Error::other(crate::i18n::remote_text(
                                    language,
                                    "save_uncertain",
                                )));
                            }
                        }
                        let link_source = match policy.path_style {
                            attachments::AttachmentPathStyle::Relative => source.clone(),
                            attachments::AttachmentPathStyle::WorkspaceAbsolute
                            | attachments::AttachmentPathStyle::Shortest => {
                                root.join_child(".moeka-root-reference")?
                            }
                            attachments::AttachmentPathStyle::FileUrl => {
                                return Err(io::Error::other(crate::i18n::remote_text(
                                    language,
                                    "attachment_unavailable",
                                )));
                            }
                        };
                        let relative =
                            remote_relative_link(&link_source, &target).ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    crate::i18n::remote_text(language, "save_path_error"),
                                )
                            })?;
                        let link = if is_replacement {
                            percent_encoding::utf8_percent_encode(
                                &relative,
                                attachments::MARKDOWN_PATH_ENCODE_SET,
                            )
                            .to_string()
                        } else {
                            match policy.link_syntax {
                                attachments::AttachmentLinkSyntax::WikiLink => {
                                    format!("{}[[{}]]", if is_image { "!" } else { "" }, relative)
                                }
                                attachments::AttachmentLinkSyntax::Markdown => {
                                    let escaped = percent_encoding::utf8_percent_encode(
                                        &relative,
                                        attachments::MARKDOWN_PATH_ENCODE_SET,
                                    );
                                    if is_image {
                                        format!("![]({escaped})")
                                    } else {
                                        format!("[{unique}]({escaped})")
                                    }
                                }
                            }
                        };
                        links.push(link);
                    }
                    Ok(links.join("\n"))
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation {
                    return;
                }
                editor.with_pane_if_alive(pane, |editor| {
                    if editor.active_tab_id != tab_id {
                        return;
                    }
                    match result {
                        Ok(text) if !text.is_empty() => {
                            if let Some((line, original)) = replacement {
                                if editor.buffer.line_content(line) == original {
                                    editor.replace_image_url_with_text(line, &text, cx);
                                }
                            } else if editor.cursor.offset == cursor {
                                editor.insert_pasted_text(&text, cx);
                            }
                        }
                        Ok(_) => {}
                        Err(error) => editor.notify_error(error.to_string(), cx),
                    }
                });
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn search_remote_wiki_links(&mut self, cx: &mut Context<Self>) {
        self.remote_wiki_link_results.clear();
        let Some(workspace) = self.remote_workspace.as_ref() else {
            cx.notify();
            return;
        };
        let filesystem = workspace.background_filesystem.clone();
        let root = workspace.root.clone();
        let generation = self.remote_generation;
        let query = self.wiki_link_query.clone();
        let pane = self.current_pane_id();
        let tab_id = self.active_tab_id.clone();
        let language = self.ui_language;
        let task = cx.spawn(async move |this, cx| {
            let search_query = query.clone();
            let result = crate::platform::fs_task::run_background(
                "SSH wiki search",
                scheduler_key(),
                move || {
                    let mut pending = vec![root.clone()];
                    let mut results = Vec::<(i64, i128, String, ResourcePath)>::new();
                    while let Some(directory) = pending.pop() {
                        crate::platform::fs_task::check_deadline()?;
                        for entry in remote_directory(&filesystem, &directory, language)? {
                            if entry.metadata.is_symlink {
                                continue;
                            }
                            let Some(NativePath::Unix(name)) = entry.resource.path.file_name()
                            else {
                                continue;
                            };
                            let valid_name = std::str::from_utf8(&name).ok();
                            if valid_name.is_some_and(|name| {
                                name.starts_with('.')
                                    || entry.metadata.is_dir
                                        && matches!(
                                            name,
                                            "node_modules"
                                                | "target"
                                                | "dist"
                                                | "build"
                                                | "coverage"
                                        )
                            }) {
                                continue;
                            }
                            if entry.metadata.is_dir {
                                pending.push(entry.resource);
                            } else if entry.metadata.is_file
                                && name
                                    .rsplit(|byte| *byte == b'.')
                                    .next()
                                    .filter(|extension| extension.len() < name.len())
                                    .and_then(|extension| std::str::from_utf8(extension).ok())
                                    .is_some_and(|extension| {
                                        matches!(
                                            extension.to_ascii_lowercase().as_str(),
                                            "md" | "markdown" | "mdx"
                                        )
                                    })
                            {
                                let Some(relative) =
                                    remote_relative_display(&root, &entry.resource)
                                else {
                                    continue;
                                };
                                let score = if search_query.trim().is_empty() {
                                    0
                                } else {
                                    let file_score =
                                        super::quick_open::fuzzy_match(&entry.name, &search_query)
                                            .map_or(0, |(score, _)| score);
                                    let path_score =
                                        super::quick_open::fuzzy_match(&relative, &search_query)
                                            .map_or(0, |(score, _)| score);
                                    if file_score == 0 && path_score == 0 {
                                        continue;
                                    }
                                    file_score * 100 + path_score
                                };
                                results.push((
                                    score,
                                    entry.metadata.modified_unix_nanos.unwrap_or_default(),
                                    relative,
                                    entry.resource,
                                ));
                                results.sort_by(|left, right| {
                                    if search_query.trim().is_empty() {
                                        right.1.cmp(&left.1).then_with(|| left.2.cmp(&right.2))
                                    } else {
                                        right
                                            .0
                                            .cmp(&left.0)
                                            .then_with(|| {
                                                left.2
                                                    .matches('/')
                                                    .count()
                                                    .cmp(&right.2.matches('/').count())
                                            })
                                            .then_with(|| left.2.cmp(&right.2))
                                    }
                                });
                                results.truncate(10);
                            }
                        }
                    }
                    Ok(results
                        .into_iter()
                        .map(|(_, _, _, resource)| resource)
                        .collect())
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation {
                    return;
                }
                editor.with_pane_if_alive(pane, |editor| {
                    if editor.active_tab_id != tab_id || editor.wiki_link_query != query {
                        return;
                    }
                    editor.wiki_link_search_task = None;
                    match result {
                        Ok(results) => editor.remote_wiki_link_results = results,
                        Err(error) => editor.notify_error(error.to_string(), cx),
                    }
                    cx.notify();
                });
            })
            .ok();
        });
        self.wiki_link_search_task = Some(task);
    }

    pub(super) fn queue_remote_tab_image(
        &mut self,
        resource: ResourcePath,
        cx: &mut Context<Self>,
    ) {
        self.queue_remote_image(resource, false, cx);
    }

    fn queue_remote_image(
        &mut self,
        resource: ResourcePath,
        thumbnail: bool,
        cx: &mut Context<Self>,
    ) {
        let key = if thumbnail {
            format!("ssh-thumbnail-{}", resource_key(&resource))
        } else {
            remote_image_cache_key(&resource)
        };
        if self.image_cache.contains_key(&key) {
            return;
        }
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        if workspace.filesystem.identity() != &resource.filesystem {
            return;
        }
        let filesystem = workspace.background_filesystem.clone();
        let epoch = workspace.resource_epoch;
        workspace.next_image_request = workspace.next_image_request.wrapping_add(1);
        let request = workspace.next_image_request;
        workspace.image_requests.insert(key.clone(), request);
        let generation = self.remote_generation;
        self.image_cache.insert(key.clone(), None);
        cx.spawn(async move |this, cx| {
            let path = resource.path.clone();
            let result =
                crate::platform::fs_task::run_background("SSH image", scheduler_key(), move || {
                    let (bytes, _) = filesystem
                        .read_file(&path, 64 * 1024 * 1024)
                        .map_err(fs_error)?;
                    let name = path
                        .file_name()
                        .map(|name| name.to_string())
                        .unwrap_or_default();
                    let extension = Path::new(&name)
                        .extension()
                        .and_then(|value| value.to_str());
                    helpers::load_render_image_from_bytes(&bytes, extension)
                        .map(|image| {
                            if thumbnail {
                                super::file_list::square_crop_render_image(image)
                            } else {
                                image
                            }
                        })
                        .map(Arc::new)
                        .map_err(fs_error)
                })
                .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation
                    || editor.remote_workspace.as_ref().is_none_or(|workspace| {
                        workspace.filesystem.identity() != &resource.filesystem
                            || workspace.resource_epoch != epoch
                            || workspace.image_requests.get(&key) != Some(&request)
                    })
                    || !editor.image_cache.contains_key(&key)
                {
                    return;
                }
                if let Some(workspace) = &mut editor.remote_workspace {
                    workspace.image_requests.remove(&key);
                }
                match result {
                    Ok(image) => {
                        editor.image_cache.insert(key, Some(image));
                    }
                    Err(error) => editor.notify_error(error.to_string(), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn record_remote_tab(
        &mut self,
        tab_id: &str,
        state: config_store::RemoteTabState,
        binding_lost: bool,
    ) {
        let mut found = false;
        for tab in self.all_tabs_mut().filter(|tab| tab.id == tab_id) {
            tab.path = None;
            tab.is_remote = true;
            tab.remote_binding_lost = binding_lost;
            tab.remote = Some(state.clone());
            found = true;
        }
        if found {
            session_store::persist_session(self);
        }
    }

    /// A local folder or a connected SSH workspace gives the sidebar content.
    pub(crate) fn has_sidebar_workspace(&self) -> bool {
        !self.workspaces.is_empty()
            || self.remote_workspace.is_some()
            || matches!(
                self.ssh_restore,
                Some(config_store::SshRestore::Connected { .. })
            )
    }

    /// Makes the connected SSH workspace that serves `resource` the active one,
    /// preferring the workspace whose root contains it. False if none is connected.
    pub(super) fn focus_remote_workspace_for_resource(
        &mut self,
        resource: &ResourcePath,
        cx: &mut Context<Self>,
    ) -> bool {
        for strict in [true, false] {
            let serves = |workspace: &RemoteWorkspace| {
                workspace.filesystem.identity() == &resource.filesystem
                    && (!strict || resource_is_at_or_below(resource, &workspace.root))
            };
            if self.remote_workspace.as_ref().is_some_and(serves) {
                return true;
            }
            if let Some(index) = self.parked_remote_workspaces.iter().position(serves) {
                self.switch_remote_workspace(index, cx);
                return true;
            }
        }
        false
    }

    pub(crate) fn remote_session_active(&self) -> bool {
        self.remote_workspace_active
            || self.workspaces.is_empty()
                && self
                    .active_tab_index()
                    .is_some_and(|index| self.tabs[index].is_remote)
    }

    pub(super) fn activate_remote_workspace(&mut self, cx: &mut Context<Self>) {
        if self.remote_workspace.is_none() {
            return;
        }
        // Each workspace has its own tab view; selecting it shows that view.
        let key = self.remote_view_key_of_current();
        self.show_view(key, cx);
        self.remote_workspace_active = true;
        self.sidebar_visible = true;
        self.workspace_search_visible = false;
        self.reset_sidebar_filter(cx);
        self.sync_sidebar_width_animation(cx);
        session_store::persist_session(self);
        cx.notify();
    }

    pub(crate) fn on_connect_ssh(
        &mut self,
        _: &crate::actions::ConnectSsh,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_remote_input(RemoteInputMode::Connect, String::new(), window, cx);
    }

    pub(super) fn open_remote_input(
        &mut self,
        mode: RemoteInputMode,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(mode, RemoteInputMode::Connect) {
            if self.command_palette_visible {
                self.close_command_palette(window, cx);
            }
            if self.quick_open_visible {
                self.close_quick_open(window, cx);
            }
            self.capture_command_origin(window, cx);
        }
        self.remote_input_generation = self.remote_input_generation.wrapping_add(1);
        let is_connect = matches!(mode, RemoteInputMode::Connect);
        let mut styles = self.quick_open_text_input_styles();
        if is_connect {
            let placeholder_alpha = (styles.text_input_placeholder_color.a * 0.55).clamp(0.0, 1.0);
            styles.text_input_placeholder_color =
                crate::theme::with_alpha(styles.text_input_placeholder_color, placeholder_alpha);
            styles.text_input_focus_placeholder_color = Some(styles.text_input_placeholder_color);
        }
        let placeholder = remote_input_placeholder(&mode, self.ui_language);
        let editor = cx.new(|cx| {
            let mut input = crate::text_input::TextInput::new(styles, &placeholder, cx);
            if !initial.is_empty() {
                input.set_text(&initial, cx);
            }
            input
        });
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.submit_remote_input(cx),
                crate::text_input::TextInputEvent::Escape => {
                    this.cancel_remote_input(cx);
                }
                crate::text_input::TextInputEvent::Changed => {
                    if let Some(input) = this.remote_input.as_mut() {
                        input.error = None;
                        if let Some(picker) = input.ssh_picker.as_mut() {
                            picker.selected_index = 0;
                            picker.scroll_handle.scroll_to_item(0);
                        }
                    }
                    this.queue_ssh_directory_query(cx);
                    cx.notify();
                }
                crate::text_input::TextInputEvent::MoveUp => {
                    this.move_ssh_host_selection(-1, cx);
                }
                crate::text_input::TextInputEvent::MoveDown => {
                    this.move_ssh_host_selection(1, cx);
                }
                _ => {}
            },
        )
        .detach();
        let focus_handle = editor.read(cx).focus_handle.clone();
        window.focus(&focus_handle);
        self.remote_input = Some(RemoteInput {
            editor,
            ssh_picker: is_connect.then(|| SshHostPicker {
                loading: true,
                ..Default::default()
            }),
            mode,
            error: None,
        });
        self.close_preview_overlay();
        if is_connect {
            let generation = self.remote_input_generation;
            cx.spawn(async move |this, cx| {
                let result = crate::platform::fs_task::run(
                    "SSH configuration",
                    scheduler_key(),
                    crate::platform::ssh::configured_hosts,
                )
                .await;
                let _ = this.update(cx, |this, cx| {
                    if this.remote_input_generation != generation {
                        return;
                    }
                    let Some(input) = this.remote_input.as_mut() else {
                        return;
                    };
                    let Some(picker) = input.ssh_picker.as_mut() else {
                        return;
                    };
                    picker.loading = false;
                    match result {
                        Ok(hosts) => picker.hosts = hosts,
                        Err(error) => {
                            input.error = Some(format!(
                                "{}: {error}",
                                crate::i18n::remote_text(this.ui_language, "config_read_failed")
                            ))
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub(crate) fn cancel_remote_input(&mut self, cx: &mut Context<Self>) {
        if self
            .remote_input
            .as_ref()
            .is_some_and(|input| input.ssh_picker.is_some())
        {
            self.pending_command_focus_restore = true;
        }
        self.remote_input_generation = self.remote_input_generation.wrapping_add(1);
        if let Some(filesystem) = self
            .remote_input
            .take()
            .and_then(|mut input| input.ssh_picker.take())
            .and_then(|mut picker| picker.browsing_filesystem.take())
        {
            cx.spawn(async move |_, cx| schedule_remote_filesystem_release(filesystem, cx))
                .detach();
        }
        cx.notify();
    }

    fn move_ssh_host_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(input) = self.remote_input.as_mut() else {
            return;
        };
        let Some(picker) = input.ssh_picker.as_mut() else {
            return;
        };
        if picker.connecting {
            return;
        }
        let count = if picker.destination.is_some() {
            picker.directories.len()
        } else {
            ssh_host_choices(&picker.hosts, input.editor.read(cx).text()).len()
        };
        picker.selected_index = (picker.selected_index as isize + delta)
            .clamp(0, count.saturating_sub(1) as isize) as usize;
        picker.scroll_handle.scroll_to_item(picker.selected_index);
        self.update_ssh_completion(cx);
        cx.notify();
    }

    fn update_ssh_completion(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.remote_input.as_ref() else {
            return;
        };
        let Some(picker) = input.ssh_picker.as_ref() else {
            return;
        };
        let value = input.editor.read(cx).text().to_string();
        let suffix = picker
            .directories
            .get(picker.selected_index)
            .and_then(|path| path.strip_prefix(&value))
            .filter(|suffix| !suffix.is_empty())
            .map(str::to_string);
        input
            .editor
            .update(cx, |input, cx| input.set_completion(&value, suffix, cx));
    }

    fn queue_ssh_directory_query(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.remote_input.as_mut() else {
            return;
        };
        let Some(picker) = input.ssh_picker.as_mut() else {
            return;
        };
        if picker.destination.is_none() {
            return;
        }
        picker.directory_query_generation = picker.directory_query_generation.wrapping_add(1);
        picker.directory_query_value.clear();
        picker.directory_loading = false;
        picker.directories.clear();
        picker.selected_index = 0;
        let value = input.editor.read(cx).text().to_string();
        input
            .editor
            .update(cx, |input, cx| input.set_completion(&value, None, cx));
        let Some((parent, prefix)) = ssh_directory_query(&value) else {
            return;
        };
        let Some(filesystem) = picker.browsing_filesystem.clone() else {
            return;
        };
        picker.directory_query_value.clone_from(&value);
        picker.directory_loading = true;
        let parent = parent.to_string();
        let prefix = prefix.to_string();
        let generation = self.remote_input_generation;
        let query_generation = picker.directory_query_generation;
        let language = self.ui_language;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let current = this
                .update(cx, |editor, _| {
                    editor.remote_input_generation == generation
                        && editor
                            .remote_input
                            .as_ref()
                            .and_then(|input| input.ssh_picker.as_ref())
                            .is_some_and(|picker| {
                                picker.directory_query_generation == query_generation
                            })
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let result = crate::platform::fs_task::run_background(
                "SSH directory suggestions",
                scheduler_key(),
                move || {
                    let path = ResourcePath {
                        filesystem: filesystem.identity().clone(),
                        path: NativePath::unix_absolute(&parent)?,
                    };
                    let entries = remote_directory(&filesystem, &path, language)?;
                    let mut paths = Vec::new();
                    for entry in entries {
                        let NativePath::Unix(name) = entry
                            .resource
                            .path
                            .file_name()
                            .unwrap_or(NativePath::Unix(Vec::new()))
                        else {
                            continue;
                        };
                        let Ok(name) = String::from_utf8(name) else {
                            continue;
                        };
                        if !name.starts_with(&prefix) {
                            continue;
                        }
                        if !entry.metadata.is_dir {
                            if !entry.metadata.is_symlink {
                                continue;
                            }
                            match filesystem.metadata(&entry.resource.path, true) {
                                Ok(metadata) if metadata.is_dir => {}
                                Ok(_) => continue,
                                Err(error) if error.kind == "NotFound" => continue,
                                Err(error) => return Err(fs_error(error)),
                            }
                        }
                        let slash = if parent == "/" { "" } else { "/" };
                        paths.push(format!("{parent}{slash}{name}/"));
                    }
                    Ok(paths)
                },
            )
            .await;
            let _ = this.update(cx, |editor, cx| {
                if editor.remote_input_generation != generation {
                    return;
                }
                let Some(input) = editor.remote_input.as_mut() else {
                    return;
                };
                let Some(picker) = input.ssh_picker.as_mut() else {
                    return;
                };
                if picker.directory_query_generation != query_generation
                    || input.editor.read(cx).text() != value
                {
                    return;
                }
                picker.directory_loading = false;
                match result {
                    Ok(paths) => {
                        picker.directories = paths;
                        input.error = None;
                    }
                    Err(error) => input.error = Some(error.to_string()),
                }
                editor.update_ssh_completion(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn select_ssh_directory(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(input) = self.remote_input.as_ref() else {
            return;
        };
        input
            .editor
            .update(cx, |input, cx| input.set_text(&path, cx));
        self.queue_ssh_directory_query(cx);
        cx.notify();
    }

    fn prepare_ssh_directory_input(
        editor: &gpui::Entity<crate::text_input::TextInput>,
        cx: &mut Context<Self>,
    ) {
        editor.update(cx, |input, cx| {
            input.placeholder = "/absolute/path".into();
            input.set_text("/", cx);
        });
    }

    fn back_to_ssh_hosts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.remote_input.as_mut() else {
            return;
        };
        let Some(picker) = input.ssh_picker.as_mut() else {
            return;
        };
        picker.connection_generation = picker.connection_generation.wrapping_add(1);
        picker.destination = None;
        picker.connecting = false;
        picker.directory_query_generation = picker.directory_query_generation.wrapping_add(1);
        picker.directory_query_value.clear();
        picker.directory_loading = false;
        picker.directories.clear();
        picker.selected_index = 0;
        picker.scroll_handle.scroll_to_item(0);
        let filesystem = picker.browsing_filesystem.take();
        input.error = None;
        let placeholder = crate::i18n::remote_text(self.ui_language, "host_placeholder");
        input.editor.update(cx, |input, cx| {
            input.placeholder = placeholder.into();
            input.set_text("", cx);
            input.set_completion("", None, cx);
        });
        window.focus(&input.editor.focus_handle(cx));
        if let Some(filesystem) = filesystem {
            cx.spawn(async move |_, cx| schedule_remote_filesystem_release(filesystem, cx))
                .detach();
        }
        cx.notify();
    }

    fn select_ssh_host(&mut self, destination: String, cx: &mut Context<Self>) {
        if destination.chars().any(char::is_whitespace) {
            let config = self
                .remote_input
                .as_ref()
                .and_then(|input| input.ssh_picker.as_ref())
                .and_then(|picker| picker.config_file.clone());
            self.connect_remote(&destination, config, cx);
            return;
        }
        let options = SshOptions {
            destination: destination.clone(),
            port: None,
            config_file: None,
        };
        if let Err(error) = options.command() {
            if let Some(input) = self.remote_input.as_mut() {
                input.error = Some(error.to_string());
            }
            cx.notify();
            return;
        }
        let Some(input) = self.remote_input.as_mut() else {
            return;
        };
        let Some(picker) = input.ssh_picker.as_mut() else {
            return;
        };
        picker.connection_generation = picker.connection_generation.wrapping_add(1);
        let connection_generation = picker.connection_generation;
        picker.destination = Some(destination.clone());
        picker.connecting = true;
        picker.selected_index = 0;
        picker.scroll_handle.scroll_to_item(0);
        input.error = None;
        let installing = crate::i18n::remote_text(self.ui_language, "installing_placeholder");
        input.editor.update(cx, |input, cx| {
            input.placeholder = installing.into();
            input.set_text("", cx);
        });
        #[cfg(test)]
        if picker.disable_browsing_for_test {
            picker.connecting = false;
            Self::prepare_ssh_directory_input(&input.editor, cx);
            cx.notify();
            return;
        }
        let config_file = picker.config_file.clone();
        let generation = self.remote_input_generation;
        let language = self.ui_language;
        cx.spawn(async move |this, cx| {
            let result = crate::platform::fs_task::run_write(
                "SSH directory connection",
                scheduler_key(),
                move || open_remote_filesystem(destination, config_file, language),
            )
            .await;
            let (mut filesystem, error) = match result {
                Ok(remote) => (Some(remote), None),
                Err(error) => (None, Some(error.to_string())),
            };
            let _ = this.update(cx, |editor, cx| {
                if editor.remote_input_generation != generation {
                    return;
                }
                let Some(input) = editor.remote_input.as_mut() else {
                    return;
                };
                let Some(picker) = input.ssh_picker.as_mut() else {
                    return;
                };
                if picker.connection_generation != connection_generation {
                    return;
                }
                picker.connecting = false;
                input.error = error;
                if let Some(remote) = filesystem.take() {
                    picker.browsing_filesystem = Some(remote);
                    Self::prepare_ssh_directory_input(&input.editor, cx);
                    editor.queue_ssh_directory_query(cx);
                } else {
                    Self::prepare_ssh_directory_input(&input.editor, cx);
                }
                cx.notify();
            });
            if let Some(remote) = filesystem {
                schedule_remote_filesystem_release(remote, cx);
            }
        })
        .detach();
        cx.notify();
    }

    fn submit_remote_input(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.remote_input.as_ref() else {
            return;
        };
        let value = input.editor.read(cx).text().trim().to_string();
        let mode = input.mode.clone();
        match mode {
            RemoteInputMode::Connect => {
                let Some(picker) = input.ssh_picker.as_ref() else {
                    return;
                };
                if picker.connecting {
                    return;
                }
                if let Some(destination) = picker.destination.as_ref() {
                    self.connect_remote(
                        &format!("{destination} {value}"),
                        picker.config_file.clone(),
                        cx,
                    );
                } else if let Some(host) = ssh_host_choices(&picker.hosts, &value)
                    .get(picker.selected_index)
                    .cloned()
                {
                    self.select_ssh_host(host, cx);
                }
            }
            RemoteInputMode::SaveAs(tab_id) => self.save_remote_as(&tab_id, value, cx),
        }
    }

    fn connect_remote(
        &mut self,
        input: &str,
        config_file: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if config_file.as_ref().is_some_and(|path| !path.is_absolute()) {
            if let Some(input) = self.remote_input.as_mut() {
                input.error = Some(crate::i18n::remote_text(
                    self.ui_language,
                    "config_absolute",
                ));
            }
            cx.notify();
            return;
        }
        let language = self.ui_language;
        let (destination, root) = match parse_connect_input(input, language) {
            Ok(value) => value,
            Err(error) => {
                if let Some(input) = self.remote_input.as_mut() {
                    input.error = Some(error.to_string());
                }
                cx.notify();
                return;
            }
        };
        self.start_remote_connection(destination, root, config_file, None, true, cx);
    }

    pub(super) fn resume_ssh_workspace(&mut self, cx: &mut Context<Self>) {
        use config_store::SshRestore;
        if !crate::platform::capabilities::CHILD_PROCESSES || self.remote_workspace.is_some() {
            return;
        }
        let active = self
            .active_tab_index()
            .and_then(|i| self.tabs[i].remote.as_ref());
        let target = self.ssh_restore.clone().or_else(|| {
            active
                .or_else(|| self.all_tabs().find_map(|t| t.remote.as_ref()))
                .map(|t| SshRestore::Connected {
                    destination: t.destination.clone(),
                    config_file: t.config_file.clone(),
                    root: t.root.clone(),
                    active: active.is_some(),
                })
        });
        let Some(SshRestore::Connected {
            destination,
            config_file,
            root,
            active,
        }) = target.clone()
        else {
            return;
        };
        self.remote_workspace_active = active;
        self.ssh_restore = target;
        self.ssh_restore_error = None;
        self.start_remote_connection(
            destination,
            root.path,
            config_file,
            Some(root.filesystem),
            true,
            cx,
        );
        for extra in self.ssh_restore_more.clone() {
            if let SshRestore::Connected {
                destination,
                config_file,
                root,
                ..
            } = extra
            {
                self.start_remote_connection(
                    destination,
                    root.path,
                    config_file,
                    Some(root.filesystem),
                    false,
                    cx,
                );
            }
        }
    }

    fn start_remote_connection(
        &mut self,
        destination: String,
        root: NativePath,
        config_file: Option<PathBuf>,
        restoring: Option<FilesystemId>,
        foreground: bool,
        cx: &mut Context<Self>,
    ) {
        let language = self.ui_language;
        let expected_root = root.clone();
        // Only a connection that will take over the sidebar supersedes others.
        if foreground {
            self.remote_connect_epoch = self.remote_connect_epoch.wrapping_add(1);
        }
        let connect_epoch = self.remote_connect_epoch;
        let input_generation = self.remote_input_generation;
        let picker_connection_generation = self
            .remote_input
            .as_ref()
            .and_then(|input| input.ssh_picker.as_ref())
            .map(|picker| picker.connection_generation);
        if let Some(picker) = self
            .remote_input
            .as_mut()
            .and_then(|input| input.ssh_picker.as_mut())
        {
            picker.connecting = true;
        }
        let scheduler = scheduler_key();
        let requested_destination = destination.clone();
        let requested_config_file = config_file.clone();
        let result = cx.spawn(async move |this, cx| {
            let result = crate::platform::fs_task::run_write("SSH connect", scheduler, move || {
                remote_connection(destination, root, config_file, language)
            })
            .await;
            let (mut connection, connect_error) = match result {
                Ok(connection) => (Some(connection), None),
                Err(error) => (None, Some(error.to_string())),
            };
            let _ = this.update(cx, |editor, cx| {
                if (foreground && editor.remote_connect_epoch != connect_epoch)
                    || editor.remote_input_generation != input_generation
                    || picker_connection_generation.is_some_and(|generation| {
                        editor
                            .remote_input
                            .as_ref()
                            .and_then(|input| input.ssh_picker.as_ref())
                            .is_none_or(|picker| picker.connection_generation != generation)
                    })
                    || (restoring.is_none() && editor.remote_input.is_none())
                {
                    return;
                }
                if let (Some(expected), Some((fs, _, root, ..))) = (&restoring, &connection)
                    && (fs.identity() != expected || root.path != expected_root)
                {
                    editor.record_ssh_restore_error(
                        &requested_destination,
                        &expected_root,
                        foreground,
                        crate::i18n::remote_text(language, "wrong_connection"),
                    );
                    cx.notify();
                    return;
                }
                if let Some((_, _, root, ..)) = &connection
                    && editor.remote_workspace_connected(&requested_destination, root)
                {
                    // Already open: reuse that workspace instead of a second connection.
                    let root = root.clone();
                    editor.forget_pending_ssh_restore(&requested_destination, &root);
                    if restoring.is_none() {
                        editor.cancel_remote_input(cx);
                        editor.show_connected_remote_workspace(&requested_destination, &root, cx);
                    }
                    return;
                }
                if let Some((
                    filesystem,
                    background_filesystem,
                    root,
                    entries,
                    watch_session,
                    channel_error,
                    peer_scope,
                )) = connection.take()
                {
                    if restoring.is_none() {
                        editor.cancel_remote_input(cx);
                    }
                    let identity = filesystem.identity().clone();
                    let reconnect_root = root.clone();
                    let reconnect_destination = requested_destination.clone();
                    let take_over = foreground || editor.remote_workspace.is_none();
                    let mut workspace = RemoteWorkspace {
                        reconnecting: false,
                        serial: NEXT_REMOTE_WORKSPACE_SERIAL.fetch_add(1, Ordering::Relaxed),
                        peer_scope,
                        filesystem: filesystem.clone(),
                        background_filesystem: background_filesystem.clone(),
                        root: root.clone(),
                        destination: reconnect_destination.clone(),
                        config_file: requested_config_file.clone(),
                        directories: HashMap::from([(root.clone(), entries)]),
                        expanded: HashSet::from([root]),
                        loading: HashSet::new(),
                        refreshing: HashSet::new(),
                        directory_attempts: DirectoryLoadAttempts::default(),
                        selected: None,
                        selected_paths: HashSet::new(),
                        selection_anchor: None,
                        focused: None,
                        error: watch_session.is_none().then(|| {
                            crate::i18n::remote_text(editor.ui_language, "watch_unavailable")
                        }),
                        watch_warning: None,
                        connection_warning: channel_error.map(|error| {
                            format!(
                                "{}: {error}",
                                crate::i18n::remote_text(editor.ui_language, "single_channel")
                            )
                        }),
                        watch_session,
                        watch_sequence: 0,
                        sort_mode: SidebarSortMode::NameAsc,
                        file_list_dir: reconnect_root.clone(),
                        tree_scroll: gpui::ScrollHandle::new(),
                        file_list_scroll: gpui::ScrollHandle::new(),
                        previews: HashMap::new(),
                        previews_loading: HashSet::new(),
                        previews_stale: HashSet::new(),
                        previews_waiting_refresh: HashSet::new(),
                        preview_reconciled: HashSet::new(),
                        image_requests: HashMap::new(),
                        next_image_request: 0,
                        resource_epoch: 0,
                        directory_epoch: 0,
                        documents_to_refresh: HashSet::new(),
                        document_refresh_running: false,
                    };
                    editor.forget_pending_ssh_restore(&reconnect_destination, &reconnect_root);
                    if !take_over {
                        // Connected for the session restore; it waits unwatched until
                        // the user switches to it.
                        let session = workspace.watch_session.take();
                        editor.parked_remote_workspaces.push(workspace);
                        cx.spawn(async move |_, _| {
                            cancel_remote_session(background_filesystem, session).await;
                        })
                        .detach();
                        session_store::persist_session(editor);
                        cx.notify();
                        return;
                    }
                    editor.park_active_remote_workspace(cx);
                    editor.remote_generation = editor.remote_generation.wrapping_add(1);
                    let generation = editor.remote_generation;
                    editor.remote_sidebar_filter_rows.clear();
                    editor.remote_sidebar_filter_collapsed.clear();
                    editor.invalidate_sidebar_filter_index();
                    editor.remote_workspace = Some(workspace);
                    editor.ssh_restore = None;
                    editor.ssh_restore_error = None;
                    if restoring.is_none() || editor.remote_workspace_active {
                        editor.activate_remote_workspace(cx);
                    } else if editor.workspaces.is_empty() && editor.remote_workspace_active {
                        // Nothing else restores a sidebar for a remote-only session.
                        editor.sidebar_visible = true;
                        editor.sync_sidebar_width_animation(cx);
                    }
                    // Tabs of other connected workspaces keep their binding.
                    let connected = editor
                        .parked_remote_workspaces
                        .iter()
                        .map(|workspace| (workspace.destination.clone(), workspace.root.clone()))
                        .collect::<Vec<_>>();
                    for tab in editor.all_tabs_mut() {
                        if tab.is_remote
                            && !tab.remote.as_ref().is_some_and(|state| {
                                connected.iter().any(|(destination, root)| {
                                    remote_state_matches_workspace(state, destination, root)
                                })
                            })
                        {
                            tab.remote_binding_lost = true;
                        }
                    }
                    editor.start_remote_watch(identity, generation, cx);
                    editor.begin_remote_rebind(cx);
                    session_store::persist_session(editor);
                    if restoring.is_none() {
                        editor.notify_info(
                            crate::i18n::remote_text(editor.ui_language, "connected"),
                            cx,
                        );
                    }
                } else if let Some(error) = connect_error {
                    if restoring.is_some() {
                        editor.record_ssh_restore_error(
                            &requested_destination,
                            &expected_root,
                            foreground,
                            error,
                        );
                        cx.notify();
                        return;
                    }
                    let Some(input) = editor.remote_input.as_mut() else {
                        return;
                    };
                    if let Some(picker) = input.ssh_picker.as_mut() {
                        picker.connecting = false;
                    }
                    input.error = Some(error);
                }
                cx.notify();
            });
            if let Some((filesystem, background_filesystem, _, _, watch_session, _, _)) =
                connection.take()
            {
                cancel_remote_session(background_filesystem.clone(), watch_session).await;
                schedule_remote_filesystem_release(filesystem, cx);
                schedule_remote_filesystem_release(background_filesystem, cx);
            }
        });
        result.detach();
    }

    pub(super) fn remote_workspace_connected(
        &self,
        destination: &str,
        root: &ResourcePath,
    ) -> bool {
        self.remote_workspace
            .iter()
            .chain(self.parked_remote_workspaces.iter())
            .any(|workspace| workspace.destination == destination && &workspace.root == root)
    }

    fn ssh_restore_key(destination: &str, root: &NativePath) -> String {
        format!("{destination}\0{root}")
    }

    fn record_ssh_restore_error(
        &mut self,
        destination: &str,
        root: &NativePath,
        primary: bool,
        error: String,
    ) {
        if primary {
            self.ssh_restore_error = Some(error);
        } else {
            self.ssh_restore_errors
                .insert(Self::ssh_restore_key(destination, root), error);
        }
    }

    /// Restored workspaces still waiting for (or failing) their connection.
    fn pending_ssh_workspaces(&self) -> Vec<PendingSshWorkspace> {
        use config_store::SshRestore::Connected;
        let primary = self.ssh_restore.iter().map(|entry| (None, entry));
        let extras = self
            .ssh_restore_more
            .iter()
            .enumerate()
            .map(|(index, entry)| (Some(index), entry));
        primary
            .chain(extras)
            .filter_map(|(extra, entry)| match entry {
                Connected {
                    destination,
                    config_file,
                    root,
                    ..
                } => Some(PendingSshWorkspace {
                    extra,
                    destination: destination.clone(),
                    config_file: config_file.clone(),
                    root: root.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn retry_pending_ssh_workspace(
        &mut self,
        pending: PendingSshWorkspace,
        cx: &mut Context<Self>,
    ) {
        if !crate::platform::capabilities::CHILD_PROCESSES {
            return;
        }
        match pending.extra {
            None => self.ssh_restore_error = None,
            Some(_) => {
                self.ssh_restore_errors.remove(&Self::ssh_restore_key(
                    &pending.destination,
                    &pending.root.path,
                ));
            }
        }
        self.start_remote_connection(
            pending.destination,
            pending.root.path,
            pending.config_file,
            Some(pending.root.filesystem),
            pending.extra.is_none(),
            cx,
        );
        cx.notify();
    }

    fn forget_pending_ssh_restore(&mut self, destination: &str, root: &ResourcePath) {
        self.ssh_restore_errors
            .remove(&Self::ssh_restore_key(destination, &root.path));
        if matches!(
            &self.ssh_restore,
            Some(config_store::SshRestore::Connected { destination: pending, root: pending_root, .. })
                if pending == destination && pending_root.path == root.path
        ) {
            self.ssh_restore = None;
            self.ssh_restore_error = None;
        }
        self.ssh_restore_more.retain(|entry| {
            !matches!(entry, config_store::SshRestore::Connected {
                destination: pending,
                root: pending_root,
                ..
            } if pending == destination && pending_root.path == root.path)
        });
    }

    /// Brings an already connected workspace to the front, whichever slot holds it.
    fn show_connected_remote_workspace(
        &mut self,
        destination: &str,
        root: &ResourcePath,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .parked_remote_workspaces
            .iter()
            .position(|workspace| workspace.destination == destination && &workspace.root == root)
        {
            self.switch_remote_workspace(index, cx);
        }
        self.activate_remote_workspace(cx);
    }

    /// Moves the active SSH workspace aside. Its connection stays open, but it
    /// stops watching and loses in-flight work, which is re-requested when it
    /// becomes active again.
    pub(super) fn park_active_remote_workspace(&mut self, cx: &mut Context<Self>) {
        if self.remote_workspace.is_none() {
            return;
        }
        self.cancel_workspace_search_remote_session(cx);
        let Some(mut workspace) = self.remote_workspace.take() else {
            return;
        };
        self.remote_generation = self.remote_generation.wrapping_add(1);
        self.remote_watch_generation = self.remote_watch_generation.wrapping_add(1);
        self.remote_search_generation = self.remote_search_generation.wrapping_add(1);
        self.remote_opens_inflight.clear();
        self.remote_sidebar_filter_epoch = self.remote_sidebar_filter_epoch.wrapping_add(1);
        self.remote_sidebar_filter_task = None;
        self.remote_sidebar_filter_rows.clear();
        self.remote_sidebar_filter_collapsed.clear();
        self.invalidate_sidebar_filter_index();
        workspace.loading.clear();
        workspace.refreshing.clear();
        workspace.previews_loading.clear();
        workspace.previews_waiting_refresh.clear();
        workspace.image_requests.clear();
        workspace.documents_to_refresh.clear();
        workspace.document_refresh_running = false;
        workspace.resource_epoch = workspace.resource_epoch.wrapping_add(1);
        workspace.directory_epoch = workspace.directory_epoch.wrapping_add(1);
        workspace.watch_sequence = 0;
        let session = workspace.watch_session.take();
        let filesystem = workspace.background_filesystem.clone();
        cx.spawn(async move |_, _| {
            cancel_remote_session(filesystem, session).await;
        })
        .detach();
        self.parked_remote_workspaces.push(workspace);
    }

    /// Makes the parked workspace at `index` the active one.
    pub(super) fn switch_remote_workspace(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.parked_remote_workspaces.len() {
            return;
        }
        // Parking appends, so `index` still names the same workspace afterwards.
        self.park_active_remote_workspace(cx);
        let workspace = self.parked_remote_workspaces.remove(index);
        self.remote_workspace = Some(workspace);
        self.remote_generation = self.remote_generation.wrapping_add(1);
        if self.remote_workspace.as_ref().is_some_and(|workspace| {
            workspace.filesystem.is_closed() || workspace.background_filesystem.is_closed()
        }) {
            // It was parked while the link died; reconnect instead of watching.
            self.reconnect_remote_workspace(cx);
            self.follow_live_remote_view(cx);
            session_store::persist_session(self);
            cx.notify();
            return;
        }
        self.restart_remote_watch(cx);
        self.refresh_remote_tree(cx);
        self.begin_remote_rebind(cx);
        if self.workspace_search_visible {
            self.run_workspace_search(cx);
        }
        self.follow_live_remote_view(cx);
        session_store::persist_session(self);
        cx.notify();
    }

    /// While an SSH view is on screen it always belongs to the current connection.
    fn follow_live_remote_view(&mut self, cx: &mut Context<Self>) {
        if self.live_view.is_some()
            && let Some(key) = self.remote_view_key_of_current()
        {
            self.show_view(Some(key), cx);
        }
    }

    fn remote_workspace_slot_mut(
        &mut self,
        destination: &str,
        root: &ResourcePath,
    ) -> Option<&mut RemoteWorkspace> {
        self.remote_workspace
            .iter_mut()
            .chain(self.parked_remote_workspaces.iter_mut())
            .find(|workspace| workspace.destination == destination && &workspace.root == root)
    }

    /// Replaces a dead connection with a fresh one, retrying with growing delays
    /// until it works or the workspace is closed. Open tabs keep their binding;
    /// their documents are reconciled once the link is back.
    pub(super) fn reconnect_remote_workspace(&mut self, cx: &mut Context<Self>) {
        const DELAYS_SECS: [u64; 6] = [0, 2, 5, 10, 30, 60];
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        if workspace.reconnecting {
            return;
        }
        workspace.reconnecting = true;
        let destination = workspace.destination.clone();
        let root = workspace.root.clone();
        let config_file = workspace.config_file.clone();
        let language = self.ui_language;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut attempt = 0_usize;
            loop {
                let delay = DELAYS_SECS[attempt.min(DELAYS_SECS.len() - 1)];
                attempt += 1;
                if delay > 0 {
                    cx.background_executor()
                        .timer(Duration::from_secs(delay))
                        .await;
                }
                let still_wanted = this
                    .update(cx, |editor, _| {
                        editor
                            .remote_workspace_slot_mut(&destination, &root)
                            .is_some_and(|workspace| workspace.reconnecting)
                    })
                    .unwrap_or(false);
                if !still_wanted {
                    return;
                }
                let (connect_destination, connect_root, connect_config) =
                    (destination.clone(), root.path.clone(), config_file.clone());
                let result = crate::platform::fs_task::run_write(
                    "SSH reconnect",
                    scheduler_key(),
                    move || {
                        remote_connection(
                            connect_destination,
                            connect_root,
                            connect_config,
                            language,
                        )
                    },
                )
                .await;
                let mut connection = match result {
                    Ok(connection) => Some(connection),
                    Err(error) => {
                        let message = error.to_string();
                        let _ = this.update(cx, |editor, cx| {
                            if let Some(workspace) =
                                editor.remote_workspace_slot_mut(&destination, &root)
                            {
                                workspace.error = Some(message);
                            }
                            cx.notify();
                        });
                        continue;
                    }
                };
                let installed = this
                    .update(cx, |editor, cx| {
                        let Some((filesystem, background, _, entries, watch, warning, scope)) =
                            connection.take()
                        else {
                            return true;
                        };
                        let identity = filesystem.identity().clone();
                        let is_active = editor.remote_workspace.as_ref().is_some_and(|workspace| {
                            workspace.destination == destination && workspace.root == root
                        });
                        let Some(workspace) = editor.remote_workspace_slot_mut(&destination, &root)
                        else {
                            // Closed meanwhile: hand the connection back for cleanup.
                            connection = Some((
                                filesystem,
                                background,
                                root.clone(),
                                entries,
                                watch,
                                warning,
                                scope,
                            ));
                            return true;
                        };
                        if !workspace.reconnecting {
                            connection = Some((
                                filesystem,
                                background,
                                root.clone(),
                                entries,
                                watch,
                                warning,
                                scope,
                            ));
                            return true;
                        }
                        workspace.filesystem = filesystem;
                        workspace.background_filesystem = background.clone();
                        workspace.peer_scope = scope;
                        workspace.directories.insert(root.clone(), entries);
                        workspace.loading.clear();
                        workspace.refreshing.clear();
                        workspace.error = None;
                        workspace.watch_warning = None;
                        workspace.connection_warning = warning.map(|error| {
                            format!(
                                "{}: {error}",
                                crate::i18n::remote_text(language, "single_channel")
                            )
                        });
                        workspace.watch_session = if is_active { watch } else { None };
                        workspace.watch_sequence = 0;
                        workspace.reconnecting = false;
                        workspace.resource_epoch = workspace.resource_epoch.wrapping_add(1);
                        workspace.directory_epoch = workspace.directory_epoch.wrapping_add(1);
                        if is_active {
                            editor.remote_generation = editor.remote_generation.wrapping_add(1);
                            let generation = editor.remote_generation;
                            editor.start_remote_watch(identity, generation, cx);
                            editor.refresh_remote_tree(cx);
                            editor.refresh_remote_documents(&[], true, cx);
                        } else if watch.is_some() {
                            cx.spawn(async move |_, _| {
                                cancel_remote_session(background, watch).await;
                            })
                            .detach();
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(true);
                if let Some((filesystem, background, _, _, watch, _, _)) = connection.take() {
                    cancel_remote_session(background.clone(), watch).await;
                    schedule_remote_filesystem_release(filesystem, cx);
                    schedule_remote_filesystem_release(background, cx);
                }
                if installed {
                    return;
                }
            }
        })
        .detach();
    }

    /// Opens a fresh watch session for the active workspace and polls it.
    fn restart_remote_watch(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        let filesystem = workspace.background_filesystem.clone();
        let identity = workspace.filesystem.identity().clone();
        let root = workspace.root.path.clone();
        let generation = self.remote_generation;
        cx.spawn(async move |this, cx| {
            let session =
                crate::platform::fs_task::run_background("SSH watch", scheduler_key(), move || {
                    Ok(
                        match filesystem.request(Request::StartWatch {
                            root,
                            generation: 1,
                            recursive: true,
                        }) {
                            Ok(Response::Session(session)) => Some(session),
                            _ => None,
                        },
                    )
                })
                .await
                .ok()
                .flatten();
            let _ = this.update(cx, |editor, cx| {
                let Some(workspace) = editor.remote_workspace.as_mut() else {
                    return;
                };
                if editor.remote_generation != generation
                    || workspace.filesystem.identity() != &identity
                {
                    return;
                }
                workspace.watch_session = session;
                workspace.watch_sequence = 0;
                if session.is_none() {
                    workspace.error = Some(crate::i18n::remote_text(
                        editor.ui_language,
                        "watch_unavailable",
                    ));
                }
                editor.start_remote_watch(identity, generation, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Rebinds disconnected tabs of the active workspace, asking first unless
    /// every one of them was last seen on this same peer.
    fn begin_remote_rebind(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        let destination = workspace.destination.clone();
        let root = workspace.root.clone();
        let peer_scope = workspace.peer_scope;
        let generation = self.remote_generation;
        let matching = |tab: &&OpenTab| {
            tab.remote_binding_lost
                && tab
                    .remote
                    .as_ref()
                    .is_some_and(|state| remote_state_matches_workspace(state, &destination, &root))
        };
        let has_matching_tabs = self.all_tabs().any(|tab| matching(&tab));
        let verified = peer_scope.is_some()
            && self
                .all_tabs()
                .filter(matching)
                .filter_map(|tab| tab.remote.as_ref())
                .all(|state| state.peer_scope == peer_scope);
        if has_matching_tabs && verified {
            self.confirm_remote_rebind(generation, cx);
        } else if has_matching_tabs {
            self.show_dialog(
                Dialog {
                    title: crate::i18n::remote_text(self.ui_language, "rebind_title"),
                    message: crate::i18n::remote_text(self.ui_language, "rebind_message"),
                    save_label: None,
                    confirm_label: crate::i18n::remote_text(self.ui_language, "continue"),
                    cancel_label: crate::i18n::remote_text(self.ui_language, "cancel"),
                    is_danger: true,
                    action: DialogAction::RebindRemote { generation },
                },
                cx,
            );
        }
    }

    pub(super) fn confirm_remote_rebind(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.remote_generation != generation {
            return;
        }
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        let root = workspace.root.clone();
        let destination = workspace.destination.clone();
        let peer_scope = workspace.peer_scope;
        let matching = self
            .all_tabs()
            .filter_map(|tab| {
                let state = tab.remote.as_ref()?;
                remote_state_matches_workspace(state, &destination, &root).then_some((
                    tab.id.clone(),
                    state.clone(),
                    tab.remote_text_unavailable(),
                ))
            })
            .collect::<Vec<_>>();
        for (id, state, missing_snapshot) in matching {
            if let Some(resource) = state.resource.clone() {
                self.remote_documents.insert(
                    id.clone(),
                    RemoteDocument {
                        resource: Some(resource),
                        revision: if missing_snapshot {
                            None
                        } else {
                            state.revision.clone()
                        },
                        save_uncertain: state.save_uncertain,
                    },
                );
            }
            for tab in self.all_tabs_mut().filter(|tab| tab.id == id) {
                tab.remote_binding_lost = false;
                if let Some(state) = &mut tab.remote {
                    state.peer_scope = peer_scope;
                }
            }
        }
        if self.active_tab_is_image()
            && let Some(resource) = self
                .active_tab_index()
                .and_then(|index| self.tabs[index].remote.as_ref())
                .and_then(|state| state.resource.clone())
        {
            self.queue_remote_tab_image(resource, cx);
        }
        self.refresh_remote_documents(&[], true, cx);
        session_store::persist_session(self);
        cx.notify();
    }

    fn start_remote_watch(
        &mut self,
        identity: FilesystemId,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        let session = workspace.watch_session;
        let foreground_filesystem = workspace.filesystem.clone();
        let filesystem = workspace.background_filesystem.clone();
        let mut sequence = workspace.watch_sequence;
        self.remote_watch_generation = self.remote_watch_generation.wrapping_add(1);
        let watch_generation = self.remote_watch_generation;
        let language = self.ui_language;
        cx.spawn(async move |this, cx| {
            let mut last_document_reconcile = std::time::Instant::now();
            let mut ticks = 0_u32;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                ticks = ticks.wrapping_add(1);
                let result = if let Some(session) = session {
                    let fs = filesystem.clone();
                    crate::platform::fs_task::run_background(
                        "SSH watch",
                        scheduler_key(),
                        // A short deadline: a hung link is declared dead (and replaced)
                        // within seconds instead of the 30 s default.
                        move || match fs
                            .request_with_timeout(
                                Request::PollWatch {
                                    session,
                                    after_sequence: sequence,
                                },
                                Duration::from_secs(10),
                            )
                            .map_err(fs_error)?
                        {
                            Response::Watch(batch) => Ok(Some(batch)),
                            _ => Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                crate::i18n::remote_text(language, "watch_response_error"),
                            )),
                        },
                    )
                    .await
                } else if ticks % 10 == 0 {
                    // Without a watch session, a periodic ping still notices a dead link.
                    let fs = filesystem.clone();
                    crate::platform::fs_task::run_background(
                        "SSH ping",
                        scheduler_key(),
                        move || {
                            fs.request(Request::PeerIdentity).map_err(fs_error)?;
                            Ok(None)
                        },
                    )
                    .await
                } else {
                    Ok(None)
                };
                let should_continue = this
                    .update(cx, |editor, cx| {
                        let Some(workspace) = editor.remote_workspace.as_mut() else {
                            return false;
                        };
                        if editor.remote_generation != generation
                            || editor.remote_watch_generation != watch_generation
                            || workspace.filesystem.identity() != &identity
                        {
                            return false;
                        }
                        if workspace.filesystem.is_closed()
                            || workspace.background_filesystem.is_closed()
                        {
                            editor.reconnect_remote_workspace(cx);
                            return false;
                        }
                        let mut reload = Vec::new();
                        let mut document_paths = Vec::new();
                        let mut invalidate_resources = false;
                        let periodic_reconcile =
                            last_document_reconcile.elapsed() >= Duration::from_secs(30);
                        if periodic_reconcile {
                            last_document_reconcile = std::time::Instant::now();
                        }
                        let mut reconcile_documents = periodic_reconcile;
                        match result {
                            Ok(Some(batch)) => {
                                sequence = batch.sequence;
                                let warning = batch.error.as_ref().map(ToString::to_string);
                                let warning_changed = workspace.watch_warning != warning;
                                let changed = batch.invalidated
                                    || warning_changed
                                    || !batch.events.is_empty();
                                reconcile_documents |= batch.invalidated || warning_changed;
                                let reconcile_directories = batch.invalidated || warning_changed;
                                workspace.watch_warning = warning;
                                if changed {
                                    workspace.error = None;
                                }
                                if changed {
                                    invalidate_resources = true;
                                    document_paths.extend(
                                        batch
                                            .events
                                            .iter()
                                            .flat_map(|event| event.paths.iter().cloned()),
                                    );
                                    let mut directories = workspace.expanded.clone();
                                    directories.insert(workspace.file_list_dir.clone());
                                    reload.extend(directories.into_iter().filter(|directory| {
                                        reconcile_directories
                                            || remote_watch_affects_directory(
                                                directory,
                                                &document_paths,
                                            )
                                    }));
                                    workspace.refreshing.extend(reload.iter().cloned());
                                }
                            }
                            Ok(None) => {
                                if periodic_reconcile {
                                    invalidate_resources = true;
                                    reload.extend(workspace.expanded.iter().cloned());
                                    if !reload.contains(&workspace.file_list_dir) {
                                        reload.push(workspace.file_list_dir.clone());
                                    }
                                    workspace.refreshing.extend(reload.iter().cloned());
                                }
                            }
                            Err(error) => {
                                let message = error.to_string();
                                if workspace.error.as_ref() != Some(&message) {
                                    workspace.error = Some(message);
                                    cx.notify();
                                }
                            }
                        }
                        if invalidate_resources {
                            editor.invalidate_sidebar_filter_index();
                            editor.invalidate_remote_watch_images(&document_paths);
                        }
                        for directory in reload {
                            editor.load_remote_directory(directory, cx);
                        }
                        if reconcile_documents || !document_paths.is_empty() {
                            editor.refresh_remote_documents(
                                &document_paths,
                                reconcile_documents,
                                cx,
                            );
                        }
                        if invalidate_resources {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            }
            schedule_remote_filesystem_release(filesystem, cx);
            schedule_remote_filesystem_release(foreground_filesystem, cx);
        })
        .detach();
    }

    pub(crate) fn disconnect_remote(&mut self, cx: &mut Context<Self>) {
        if self.remote_workspace.is_none() {
            return;
        }
        self.cancel_workspace_search_remote_session(cx);
        // Leave this workspace's view (its tabs stay, unbound) before the connection goes.
        let was_active = self.live_view.is_some();
        if was_active {
            self.show_view(None, cx);
        }
        let Some(workspace) = self.remote_workspace.take() else {
            return;
        };
        self.remote_workspace_active = false;
        self.resource_clipboard = None;
        self.remote_generation = self.remote_generation.wrapping_add(1);
        self.remote_watch_generation = self.remote_watch_generation.wrapping_add(1);
        self.remote_search_generation = self.remote_search_generation.wrapping_add(1);
        self.invalidate_sidebar_filter_index();
        self.remote_sidebar_filter_epoch = self.remote_sidebar_filter_epoch.wrapping_add(1);
        self.remote_sidebar_filter_task = None;
        self.remote_sidebar_filter_rows.clear();
        self.remote_sidebar_filter_collapsed.clear();
        self.remote_opens_inflight.clear();
        for tab in self.all_tabs_mut().filter(|tab| {
            tab.remote.as_ref().is_some_and(|state| {
                remote_state_matches_workspace(state, &workspace.destination, &workspace.root)
            })
        }) {
            tab.remote_binding_lost = true;
        }
        if self.parked_remote_workspaces.is_empty() {
            self.ssh_restore = Some(config_store::SshRestore::Disconnected);
        } else {
            self.remote_workspace = Some(self.parked_remote_workspaces.remove(0));
            if was_active && self.workspaces.is_empty() {
                // Without a local workspace the next SSH workspace takes over the view.
                let key = self.remote_view_key_of_current();
                self.show_view(key, cx);
            }
            self.restart_remote_watch(cx);
            self.refresh_remote_tree(cx);
            self.begin_remote_rebind(cx);
        }
        if self.workspaces.is_empty() && self.remote_workspace.is_none() {
            self.sidebar_visible = false;
            self.sidebar_resizing = false;
            self.workspace_search_visible = false;
            self.reset_sidebar_filter(cx);
            self.sync_sidebar_width_animation(cx);
        }
        if self.workspace_search_visible {
            self.run_workspace_search(cx);
        }
        session_store::persist_session(self);
        let filesystem = workspace.filesystem;
        let background_filesystem = workspace.background_filesystem;
        let session = workspace.watch_session;
        cx.spawn(async move |_this, cx| {
            if let Some(session) = session {
                let cancel_filesystem = background_filesystem.clone();
                let _ = crate::platform::fs_task::run(
                    "SSH disconnect watch",
                    scheduler_key(),
                    move || {
                        cancel_filesystem
                            .request(Request::Cancel { session })
                            .map_err(fs_error)
                            .map(|_| ())
                    },
                )
                .await;
            }
            schedule_remote_filesystem_release(filesystem, cx);
            schedule_remote_filesystem_release(background_filesystem, cx);
        })
        .detach();
        self.notify_info(
            crate::i18n::remote_text(self.ui_language, "disconnected"),
            cx,
        );
        cx.notify();
    }

    pub(super) fn load_remote_directory(&mut self, path: ResourcePath, cx: &mut Context<Self>) {
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        if !workspace.directory_attempts.should_start(
            &path,
            workspace.directories.contains_key(&path),
            workspace.loading.contains(&path),
            workspace.refreshing.contains(&path),
        ) {
            return;
        }
        workspace.refreshing.remove(&path);
        workspace.loading.insert(path.clone());
        let filesystem = workspace.background_filesystem.clone();
        let identity = filesystem.identity().clone();
        let generation = self.remote_generation;
        let epoch = workspace.directory_epoch;
        let language = self.ui_language;
        cx.spawn(async move |this, cx| {
            let fs = filesystem;
            let work_path = path.clone();
            let result = crate::platform::fs_task::run_background(
                "SSH directory",
                scheduler_key(),
                move || remote_directory(&fs, &work_path, language),
            )
            .await;
            this.update(cx, |editor, cx| {
                let Some(workspace) = editor.remote_workspace.as_mut() else {
                    return;
                };
                if editor.remote_generation != generation
                    || workspace.filesystem.identity() != &identity
                    || workspace.directory_epoch != epoch
                {
                    return;
                }
                workspace.loading.remove(&path);
                workspace.directory_attempts.finish(&path, result.is_ok());
                match result {
                    Ok(mut entries) => {
                        sort_remote_entries(&mut entries, editor.sidebar_sort_mode);
                        workspace
                            .previews_waiting_refresh
                            .retain(|resource| resource.parent().as_ref() != Some(&path));
                        workspace.previews.retain(|resource, preview| {
                            if resource.parent().as_ref() != Some(&path) {
                                return true;
                            }
                            let Some(entry) =
                                entries.iter().find(|entry| &entry.resource == resource)
                            else {
                                return false;
                            };
                            if entry.metadata.modified_unix_nanos != preview.modified
                                || entry.metadata.len != preview.len
                            {
                                workspace.previews_stale.insert(resource.clone());
                            }
                            true
                        });
                        workspace.directories.insert(path.clone(), entries);
                        workspace.error = None;
                    }
                    Err(error) => {
                        workspace.error = Some(error.to_string());
                        workspace.refreshing.remove(&path);
                    }
                }
                let retry = workspace.refreshing.contains(&path)
                    && !workspace.directory_attempts.failed.contains(&path);
                if retry {
                    editor.load_remote_directory(path, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn invalidate_remote_resources(&mut self) {
        if let Some(workspace) = self.remote_workspace.as_mut() {
            workspace.previews.clear();
            workspace.previews_waiting_refresh.clear();
            workspace.previews_loading.clear();
            workspace.previews_stale.clear();
            workspace.preview_reconciled.clear();
            workspace.image_requests.clear();
            workspace.resource_epoch = workspace.resource_epoch.wrapping_add(1);
        }
        self.retired_images.extend(
            self.image_cache
                .extract_if(|key, _| {
                    key.starts_with("ssh-image-") || key.starts_with("ssh-thumbnail-")
                })
                .filter_map(|(_, image)| image),
        );
    }

    pub(super) fn refresh_remote_tree(&mut self, cx: &mut Context<Self>) {
        self.invalidate_sidebar_filter_index();
        self.invalidate_remote_resources();
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        workspace.directory_epoch = workspace.directory_epoch.wrapping_add(1);
        workspace.loading.clear();
        let mut directories = workspace.expanded.clone();
        directories.insert(workspace.file_list_dir.clone());
        for directory in directories {
            if let Some(workspace) = self.remote_workspace.as_mut() {
                workspace.refreshing.insert(directory.clone());
            }
            self.load_remote_directory(directory, cx);
        }
    }

    fn invalidate_remote_watch_images(&mut self, paths: &[NativePath]) {
        let Some(workspace) = &mut self.remote_workspace else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        let changed = paths
            .iter()
            .map(|path| workspace.root.with_path(path.clone()))
            .collect::<Vec<_>>();
        let affected = |resource: &ResourcePath| {
            changed
                .iter()
                .any(|root| resource_is_at_or_below(resource, root))
        };
        workspace
            .preview_reconciled
            .retain(|resource| !affected(resource));
        // Revalidate changed text without blanking the accepted preview while I/O runs.
        workspace.previews_stale.extend(
            workspace
                .previews
                .keys()
                .filter(|resource| affected(resource))
                .cloned(),
        );
        let mut images = changed.clone();
        for (source, preview) in &workspace.previews {
            if let Ok(details) = &preview.details {
                images.extend(
                    details
                        .image_paths
                        .iter()
                        .filter_map(|target| remote_relative_resource(source, target).ok())
                        .filter(&affected),
                );
            }
        }
        let keys = images
            .iter()
            .flat_map(|resource| {
                [
                    remote_image_cache_key(resource),
                    format!("ssh-thumbnail-{}", resource_key(resource)),
                ]
            })
            .collect::<HashSet<_>>();
        workspace
            .image_requests
            .retain(|key, _| !keys.contains(key));
        self.retired_images.extend(
            self.image_cache
                .extract_if(|key, _| keys.contains(key))
                .filter_map(|(_, image)| image),
        );
    }

    pub(super) fn collapse_remote_directories(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        workspace.expanded.clear();
        workspace.expanded.insert(workspace.root.clone());
        cx.notify();
    }

    pub(super) fn expand_remote_directories(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        let directories = workspace
            .directories
            .values()
            .flatten()
            .filter(|entry| entry.metadata.is_dir)
            .map(|entry| entry.resource.clone())
            .collect::<Vec<_>>();
        if let Some(workspace) = self.remote_workspace.as_mut() {
            workspace.expanded.extend(directories.iter().cloned());
        }
        for directory in directories {
            self.load_remote_directory(directory, cx);
        }
        cx.notify();
    }

    fn ensure_remote_sort_current(&mut self) {
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        if workspace.sort_mode != self.sidebar_sort_mode {
            workspace.sort_mode = self.sidebar_sort_mode;
            for entries in workspace.directories.values_mut() {
                sort_remote_entries(entries, self.sidebar_sort_mode);
            }
            self.invalidate_sidebar_filter_index();
        }
    }

    fn refresh_remote_documents(
        &mut self,
        paths: &[NativePath],
        reconcile: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        let identity = workspace.filesystem.identity().clone();
        workspace.documents_to_refresh.extend(
            self.remote_documents
                .values()
                .filter_map(|document| document.resource.as_ref())
                .filter(|resource| {
                    resource.filesystem == identity
                        && (reconcile
                            || paths.iter().any(|path| {
                                resource_is_at_or_below(resource, &resource.with_path(path.clone()))
                            }))
                })
                .cloned(),
        );
        if workspace.document_refresh_running || workspace.documents_to_refresh.is_empty() {
            return;
        }
        workspace.document_refresh_running = true;
        let generation = self.remote_generation;
        let language = self.ui_language;
        cx.spawn(async move |this, cx| {
            loop {
                let request = this
                    .update(cx, |editor, _| {
                        if editor.remote_generation != generation {
                            return None;
                        }
                        let workspace = editor.remote_workspace.as_mut()?;
                        let Some(resource) = workspace.documents_to_refresh.iter().next().cloned()
                        else {
                            workspace.document_refresh_running = false;
                            return None;
                        };
                        workspace.documents_to_refresh.remove(&resource);
                        let filesystem = workspace.background_filesystem.clone();
                        let expected = editor
                            .remote_documents
                            .iter()
                            .filter(|(id, document)| {
                                document.resource.as_ref() == Some(&resource)
                                    && !document.save_uncertain
                                    && !editor.remote_saves_inflight.contains(*id)
                                    && editor
                                        .all_tabs()
                                        .any(|tab| &tab.id == *id && !tab.remote_binding_lost)
                            })
                            .map(|(id, document)| (id.clone(), document.revision.clone()))
                            .collect::<HashMap<_, _>>();
                        let text = editor.all_tabs().any(|tab| {
                            expected.contains_key(&tab.id)
                                && tab
                                    .file_kind
                                    .as_ref()
                                    .is_some_and(FileKind::supports_text_buffer)
                        });
                        Some((filesystem, resource, expected, text))
                    })
                    .ok()
                    .flatten();
                let Some((filesystem, resource, expected, text)) = request else {
                    break;
                };
                if expected.is_empty() {
                    continue;
                }
                let path = resource.path.clone();
                let expected_revisions = expected.values().cloned().collect::<Vec<_>>();
                let result = crate::platform::fs_task::run_background(
                    "SSH document refresh",
                    scheduler_key(),
                    move || {
                        let observed = filesystem.read_chunk(&path, 0, 1).and_then(|chunk| {
                            if expected_revisions
                                .iter()
                                .all(|revision| revision.as_ref() == Some(&chunk.revision))
                            {
                                Ok((None, chunk.revision))
                            } else {
                                filesystem
                                    .read_file(&path, MAX_REMOTE_DOCUMENT_BYTES)
                                    .map(|(bytes, revision)| (Some(bytes), revision))
                            }
                        });
                        match observed {
                            Ok((None, revision)) => Ok(RemoteDocumentObservation::Present {
                                text: None,
                                revision,
                            }),
                            Ok((Some(bytes), revision)) => {
                                let name = path
                                    .file_name()
                                    .map(|name| name.to_string())
                                    .unwrap_or_default();
                                if text
                                    && !moeka_fs::content::classify_content(
                                        Path::new(&name),
                                        &bytes,
                                    )
                                    .supports_text_search()
                                {
                                    return Err(io::Error::new(
                                        io::ErrorKind::Unsupported,
                                        crate::i18n::remote_text(
                                            language,
                                            "unsupported_remote_binary",
                                        ),
                                    ));
                                }
                                Ok(RemoteDocumentObservation::Present {
                                    text: if text {
                                        Some(decode_remote_text(bytes)?)
                                    } else {
                                        None
                                    },
                                    revision,
                                })
                            }
                            Err(error) if error.kind == "NotFound" => {
                                Ok(RemoteDocumentObservation::Missing)
                            }
                            Err(error) => Err(fs_error(error)),
                        }
                    },
                )
                .await;
                let live = this
                    .update(cx, |editor, cx| {
                        if editor.remote_generation != generation {
                            return false;
                        }
                        if !expected.iter().any(|(id, revision)| {
                            !editor.remote_saves_inflight.contains(id)
                                && editor.remote_documents.get(id).is_some_and(|document| {
                                    document.resource.as_ref() == Some(&resource)
                                        && &document.revision == revision
                                        && !document.save_uncertain
                                })
                                && editor
                                    .all_tabs()
                                    .any(|tab| &tab.id == id && !tab.remote_binding_lost)
                        }) {
                            return true;
                        }
                        match result {
                            Ok(observation) => editor.apply_remote_document_observation(
                                &resource,
                                &expected,
                                observation,
                                cx,
                            ),
                            Err(error) => {
                                if let Some(workspace) = editor.remote_workspace.as_mut() {
                                    workspace.error = Some(format!("{}: {error}", resource.path));
                                }
                                cx.notify();
                            }
                        }
                        true
                    })
                    .unwrap_or(false);
                if !live {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply_remote_document_observation(
        &mut self,
        resource: &ResourcePath,
        expected: &HashMap<String, Option<Revision>>,
        observation: RemoteDocumentObservation,
        cx: &mut Context<Self>,
    ) {
        let mut conflict = false;
        let mut changed = false;
        let mut invalidate_images = false;
        for pane in self.pane_ids() {
            self.with_pane(pane, |editor| {
                let mut active_snapshot = None;
                for index in 0..editor.tabs.len() {
                    let id = editor.tabs[index].id.clone();
                    let Some(expected_revision) = expected.get(&id) else {
                        continue;
                    };
                    if editor.tabs[index].remote_binding_lost
                        || editor.remote_saves_inflight.contains(&id)
                        || editor.remote_documents.get(&id).is_none_or(|document| {
                            document.resource.as_ref() != Some(resource)
                                || &document.revision != expected_revision
                                || document.save_uncertain
                        })
                    {
                        continue;
                    }
                    let RemoteDocumentObservation::Present { text, revision } = &observation else {
                        changed |= !editor.tabs[index].is_deleted;
                        editor.tabs[index].is_deleted = true;
                        continue;
                    };
                    changed |= editor.tabs[index].is_deleted;
                    editor.tabs[index].is_deleted = false;
                    if expected_revision.as_ref() == Some(revision) {
                        continue;
                    }
                    if editor.tabs[index].is_dirty {
                        conflict = true;
                        continue;
                    }
                    changed = true;
                    let active = editor.active_tab_id.as_ref() == Some(&id);
                    if active && text.is_some() {
                        let snapshot = editor.current_snapshot();
                        editor.tabs[index].set_snapshot(snapshot);
                    }
                    if let Some(text) = text {
                        let changed = super::file_watch::refresh_clean_snapshot(
                            &mut editor.tabs[index].snapshot,
                            false,
                            text.clone(),
                        );
                        if changed {
                            editor.tabs[index].heading_title_cache.get_mut().take();
                            editor.tabs[index].history.saved_text = Some(text.clone());
                            editor.tabs[index].history.clear_entries();
                            if active {
                                active_snapshot = Some(editor.tabs[index].snapshot.clone());
                            }
                        }
                    } else if matches!(editor.tabs[index].file_kind, Some(FileKind::Image)) {
                        invalidate_images = true;
                    } else if matches!(editor.tabs[index].file_kind, Some(FileKind::Pdf))
                        && let Some(view) = editor.pdf_views.get(&id).cloned()
                        && let Some(workspace) = editor.remote_workspace.as_ref()
                    {
                        view.update(cx, |view, cx| {
                            view.reload_remote(
                                workspace.filesystem.clone(),
                                resource.path.clone(),
                                &editor.theme,
                                editor.ui_language,
                                cx,
                            )
                        });
                    }
                    if let Some(state) = editor.tabs[index].remote.as_mut() {
                        state.revision = Some(revision.clone());
                        if let Some(text) = text {
                            state.line_ending_crlf = remote_uses_crlf(text);
                        }
                    }
                    if let Some(document) = editor.remote_documents.get_mut(&id) {
                        document.revision = Some(revision.clone());
                    }
                }
                if let Some(snapshot) = active_snapshot {
                    editor.discard_history_transaction();
                    editor.restore_tab_snapshot_state(&snapshot);
                    editor.invalidate_active_file_panel_caches();
                }
            });
        }
        if conflict {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "save_conflict"),
                cx,
            );
        }
        if invalidate_images {
            self.invalidate_remote_resources();
        }
        if changed {
            session_store::persist_session(self);
            cx.notify();
        }
    }
    fn load_remote_file_previews(
        &mut self,
        entries: &[RemoteDirectoryEntry],
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        if workspace.previews.len() > 256 {
            workspace
                .previews
                .retain(|resource, _| entries.iter().any(|entry| &entry.resource == resource));
        }
        let pending = entries
            .iter()
            .filter(|entry| {
                (!workspace.previews.contains_key(&entry.resource)
                    || workspace.previews_stale.contains(&entry.resource))
                    && !workspace.previews_loading.contains(&entry.resource)
                    && !workspace.previews_waiting_refresh.contains(&entry.resource)
            })
            .cloned()
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return;
        }
        workspace
            .previews_loading
            .extend(pending.iter().map(|entry| entry.resource.clone()));
        for entry in &pending {
            workspace.previews_stale.remove(&entry.resource);
        }
        let filesystem = workspace.background_filesystem.clone();
        let epoch = workspace.resource_epoch;
        let generation = self.remote_generation;
        cx.spawn(async move |this, cx| {
            let paths = pending
                .iter()
                .map(|entry| entry.resource.path.clone())
                .collect::<Vec<_>>();
            let result = crate::platform::fs_task::run_background(
                "SSH file list previews",
                scheduler_key(),
                move || {
                    filesystem
                        .read_previews(&paths, super::file_list::FILE_LIST_READ_LIMIT)
                        .map_err(fs_error)
                        .and_then(|results| {
                            if results.len() == paths.len() {
                                Ok(results)
                            } else {
                                Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    crate::i18n::runtime::RuntimeText::FileAction,
                                ))
                            }
                        })
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation {
                    return;
                }
                let Some(workspace) = editor.remote_workspace.as_mut() else {
                    return;
                };
                if workspace.resource_epoch != epoch {
                    return;
                }
                for entry in &pending {
                    workspace.previews_loading.remove(&entry.resource);
                }
                let mut reload = HashSet::new();
                match result {
                    Ok(results) => {
                        for (entry, result) in pending.into_iter().zip(results) {
                            let current = entry
                                .resource
                                .parent()
                                .and_then(|parent| workspace.directories.get(&parent))
                                .is_some_and(|entries| {
                                    entries.iter().any(|current| {
                                        current.resource == entry.resource
                                            && current.metadata.len == entry.metadata.len
                                            && current.metadata.modified_unix_nanos
                                                == entry.metadata.modified_unix_nanos
                                    })
                                });
                            if !current {
                                continue;
                            }
                            if result.as_ref().is_ok_and(|preview| {
                                preview.metadata.len != entry.metadata.len
                                    || preview.metadata.modified_unix_nanos
                                        != entry.metadata.modified_unix_nanos
                            }) {
                                if !workspace.preview_reconciled.insert(entry.resource.clone()) {
                                    let message = crate::i18n::remote_text(
                                        editor.ui_language,
                                        "preview_changed",
                                    );
                                    workspace.error = Some(message.clone());
                                    retain_remote_preview(
                                        &mut workspace.previews,
                                        &entry,
                                        Err(message),
                                    );
                                    continue;
                                }
                                workspace
                                    .previews_waiting_refresh
                                    .insert(entry.resource.clone());
                                if let Some(parent) = entry.resource.parent() {
                                    workspace.refreshing.insert(parent.clone());
                                    reload.insert(parent);
                                }
                                continue;
                            }
                            let details = result
                                .map(|preview| {
                                    let mut text =
                                        String::from_utf8_lossy(&preview.bytes).into_owned();
                                    if text.ends_with('\u{fffd}') {
                                        text.pop();
                                    }
                                    let (title, preview) =
                                        super::file_list::extract_title_and_preview(&text);
                                    super::file_list::FileListDetails {
                                        title,
                                        preview,
                                        image_paths: super::file_list::extract_image_paths(&text),
                                    }
                                })
                                .map_err(|error| error.to_string());
                            if let Err(error) = &details {
                                workspace.error = Some(error.clone());
                            }
                            retain_remote_preview(&mut workspace.previews, &entry, details);
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        workspace.error = Some(message.clone());
                        for entry in pending {
                            retain_remote_preview(
                                &mut workspace.previews,
                                &entry,
                                Err(message.clone()),
                            );
                        }
                    }
                }
                for directory in reload {
                    editor.load_remote_directory(directory, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens a workspace-search hit; once the file is showing, the match is
    /// revealed with the same highlighting and scrolling as a local one.
    pub(super) fn open_remote_file_at_match(
        &mut self,
        resource: ResourcePath,
        search_match: WorkspaceSearchMatch,
        cx: &mut Context<Self>,
    ) {
        let line = search_match.line_number.saturating_sub(1);
        self.remote_pending_search_match = Some((resource.clone(), search_match));
        self.open_remote_file_with_location(resource, None, Some(line), cx);
    }

    /// Reveals `line`, upgrading to the pending search match for `resource`.
    fn reveal_remote_target(
        &mut self,
        resource: &ResourcePath,
        line: usize,
        cx: &mut Context<Self>,
    ) {
        match self.remote_pending_search_match.take() {
            Some((pending, search_match)) if &pending == resource => {
                let path = remote_path_buf(&resource.path);
                self.reveal_workspace_search_match(path, search_match, cx);
            }
            _ => self.reveal_line_in_current_buffer(line, cx),
        }
    }

    pub(super) fn open_remote_file(&mut self, resource: ResourcePath, cx: &mut Context<Self>) {
        self.open_remote_file_with_location(resource, None, None, cx);
    }

    fn open_remote_file_with_location(
        &mut self,
        resource: ResourcePath,
        anchor: Option<String>,
        target_line: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if let Some((tab_id, pane)) = self.remote_documents.iter().find_map(|(id, document)| {
            (document.resource.as_ref() == Some(&resource)
                && self
                    .all_tabs()
                    .any(|tab| &tab.id == id && !tab.remote_binding_lost))
            .then(|| self.pane_id_for_tab(id).map(|pane| (id.clone(), pane)))
            .flatten()
        }) {
            self.focus_pane(pane, cx);
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) {
                self.activate_tab_by_index(index, cx);
            }
            if let Some(workspace) = self.remote_workspace.as_mut()
                && resource_is_at_or_below(&resource, &workspace.root)
            {
                workspace.selected = Some(resource.clone());
                workspace.selected_paths.clear();
                workspace.selected_paths.insert(resource.clone());
                workspace.focused = Some(resource.clone());
                if let Some(parent) = resource.parent() {
                    workspace.file_list_dir = parent;
                }
            }
            if let Some(anchor) = anchor
                && let Some(line) = self.find_anchor_line_in_current_buffer(&anchor)
            {
                self.reveal_line_in_current_buffer(line, cx);
            } else if let Some(line) = target_line {
                self.reveal_remote_target(&resource, line, cx);
            }
            return;
        }
        let Some((filesystem, root, destination, config_file)) =
            self.remote_workspace.as_ref().map(|workspace| {
                (
                    workspace.filesystem.clone(),
                    workspace.root.clone(),
                    workspace.destination.clone(),
                    workspace.config_file.clone(),
                )
            })
        else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "not_connected"),
                cx,
            );
            return;
        };
        if resource.filesystem != *filesystem.identity() {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "wrong_connection"),
                cx,
            );
            return;
        }
        if !self.remote_opens_inflight.insert(resource.clone()) {
            return;
        }
        let pane = self.current_pane_id();
        let connection_generation = self.remote_generation;
        let identity = filesystem.identity().clone();
        let open_resource = resource.clone();
        let scheduler = scheduler_key();
        let language = self.ui_language;
        cx.spawn(async move |this, cx| {
            let fs = filesystem;
            let path = open_resource.path.clone();
            let result =
                crate::platform::fs_task::run_background("SSH open file", scheduler, move || {
                    let (bytes, revision) = fs
                        .read_file(&path, MAX_REMOTE_DOCUMENT_BYTES)
                        .map_err(fs_error)?;
                    let name = path
                        .file_name()
                        .map(|name| name.to_string())
                        .unwrap_or_default();
                    let file_kind = super::classify_file_kind_for_bytes(Path::new(&name), &bytes);
                    let content = match file_kind {
                        FileKind::Image => {
                            let extension = Path::new(&name)
                                .extension()
                                .and_then(|value| value.to_str());
                            let image = helpers::load_render_image_from_bytes(&bytes, extension)
                                .map_err(fs_error)?;
                            RemoteOpenContent::Image(Arc::new(image))
                        }
                        FileKind::Pdf => RemoteOpenContent::Pdf,
                        ref kind if kind.supports_text_buffer() => {
                            RemoteOpenContent::Text(decode_remote_text(bytes)?)
                        }
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::Unsupported,
                                crate::i18n::remote_text(language, "unsupported_remote_binary"),
                            ));
                        }
                    };
                    Ok((content, revision, file_kind, name))
                })
                .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation == connection_generation {
                    editor.remote_opens_inflight.remove(&open_resource);
                }
                if editor.remote_generation != connection_generation
                    || editor
                        .remote_workspace
                        .as_ref()
                        .is_none_or(|workspace| workspace.filesystem.identity() != &identity)
                {
                    return;
                }
                if editor.with_pane_if_alive(pane, |_| ()).is_none() {
                    return;
                }
                match result {
                    Ok((content, revision, file_kind, name)) => {
                        if let Some(workspace) = editor.remote_workspace.as_mut()
                            && resource_is_at_or_below(&open_resource, &workspace.root)
                        {
                            workspace.selected = Some(open_resource.clone());
                            workspace.selected_paths.clear();
                            workspace.selected_paths.insert(open_resource.clone());
                            workspace.focused = Some(open_resource.clone());
                            if let Some(parent) = open_resource.parent() {
                                workspace.file_list_dir = parent;
                            }
                        }
                        editor.with_pane_if_alive(pane, |editor| {
                            editor.create_remote_scratch_tab(cx);
                            let Some(index) = editor.active_tab_index() else {
                                return;
                            };
                            let tab_id = editor.tabs[index].id.clone();
                            let text = match content {
                                RemoteOpenContent::Text(text) => text,
                                RemoteOpenContent::Image(image) => {
                                    editor.image_cache.insert(
                                        remote_image_cache_key(&open_resource),
                                        Some(image),
                                    );
                                    String::new()
                                }
                                RemoteOpenContent::Pdf => String::new(),
                            };
                            let line_ending_crlf = remote_uses_crlf(&text);
                            let snapshot = Self::snapshot_for_loaded_text(text.clone(), &file_kind);
                            editor.tabs[index].title = name;
                            editor.tabs[index].path = None;
                            editor.tabs[index].is_remote = true;
                            editor.tabs[index].remote_binding_lost = false;
                            editor.tabs[index].file_kind = Some(file_kind);
                            editor.tabs[index].is_dirty = false;
                            editor.tabs[index].is_deleted = false;
                            editor.tabs[index].history.saved_text = Some(text);
                            editor.tabs[index].remote = Some(config_store::RemoteTabState {
                                peer_scope: editor
                                    .remote_workspace
                                    .as_ref()
                                    .and_then(|w| w.peer_scope),
                                destination: destination.clone(),
                                config_file: config_file.clone(),
                                root: root.clone(),
                                resource: Some(open_resource.clone()),
                                revision: Some(revision.clone()),
                                save_uncertain: false,
                                pending_resource: None,
                                line_ending_crlf,
                            });
                            editor.tabs[index].set_snapshot(snapshot.clone());
                            editor.restore_tab_snapshot_state(&snapshot);
                            editor.active_file_kind_cache = editor.tabs[index].file_kind.clone();
                            editor.current_file_path = None;
                            editor.remote_documents.insert(
                                tab_id,
                                RemoteDocument {
                                    resource: Some(open_resource.clone()),
                                    revision: Some(revision),
                                    save_uncertain: false,
                                },
                            );
                            editor.invalidate_active_file_panel_caches();
                            if let Some(anchor) = anchor.as_ref()
                                && let Some(line) =
                                    editor.find_anchor_line_in_current_buffer(anchor)
                            {
                                editor.reveal_line_in_current_buffer(line, cx);
                            } else if let Some(line) = target_line {
                                editor.reveal_remote_target(&open_resource, line, cx);
                            }
                            session_store::persist_session(editor);
                            cx.notify();
                        });
                    }
                    Err(error) => editor.notify_error(error.to_string(), cx),
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn navigate_remote_markdown(&mut self, target: String, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let tab = &self.tabs[index];
        let Some(source) = tab.remote.as_ref().and_then(|state| state.resource.clone()) else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "remote_binding_lost"),
                cx,
            );
            return;
        };
        let Some(workspace) = self.remote_workspace.as_ref().filter(|workspace| {
            !tab.remote_binding_lost && workspace.filesystem.identity() == &source.filesystem
        }) else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "remote_binding_lost"),
                cx,
            );
            return;
        };
        let (raw, anchor) = split_markdown_target(&target);
        let anchor = anchor
            .filter(|anchor| !anchor.is_empty())
            .map(str::to_string);
        let mut candidates = Vec::new();
        match remote_relative_resource(&source, raw) {
            Ok(resource) => candidates.push(resource),
            Err(error) => {
                self.notify_error(error.to_string(), cx);
                return;
            }
        }
        if !raw.rsplit('/').next().unwrap_or_default().contains('.')
            && let Ok(resource) = remote_relative_resource(&source, &format!("{raw}.md"))
        {
            candidates.push(resource);
        }
        if !raw.starts_with('/')
            && let Ok(root_source) = workspace.root.join_child(".moeka-root-reference")
        {
            if let Ok(resource) = remote_relative_resource(&root_source, raw)
                && !candidates.contains(&resource)
            {
                candidates.push(resource);
            }
            if !raw.rsplit('/').next().unwrap_or_default().contains('.')
                && let Ok(resource) = remote_relative_resource(&root_source, &format!("{raw}.md"))
                && !candidates.contains(&resource)
            {
                candidates.push(resource);
            }
        }
        let filesystem = workspace.filesystem.clone();
        let generation = self.remote_generation;
        let pane = self.current_pane_id();
        let tab_id = self.active_tab_id.clone();
        let epoch = self.preview_open_epoch.wrapping_add(1);
        self.preview_open_epoch = epoch;
        cx.spawn(async move |this, cx| {
            let result =
                crate::platform::fs_task::run_background("SSH link", scheduler_key(), move || {
                    for candidate in candidates {
                        match filesystem.metadata(&candidate.path, true) {
                            Ok(metadata) if !metadata.is_dir => return Ok(candidate),
                            Ok(_) => {}
                            Err(error) => {
                                if error.kind != "NotFound" {
                                    return Err(fs_error(error));
                                }
                            }
                        }
                    }
                    Err(io::Error::new(io::ErrorKind::NotFound, target))
                })
                .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation || editor.preview_open_epoch != epoch {
                    return;
                }
                editor.with_pane_if_alive(pane, |editor| {
                    if editor.active_tab_id != tab_id {
                        return;
                    }
                    match result {
                        Ok(resource) => {
                            editor.open_remote_file_with_location(resource, anchor, None, cx)
                        }
                        Err(error) => editor.notify_error(error.to_string(), cx),
                    }
                });
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn start_remote_sidebar_create(
        &mut self,
        parent: ResourcePath,
        directory: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_edit_in_file_list =
            self.file_list_visible && self.file_list_menu_origin && !directory;
        if let Some(workspace) = self.remote_workspace.as_mut() {
            workspace.expanded.insert(parent.clone());
        }
        self.load_remote_directory(parent.clone(), cx);
        let styles = self.sidebar_edit_text_input_styles();
        let placeholder = if directory {
            self.t(crate::i18n::TextKey::FolderNamePlaceholder)
        } else {
            self.t(crate::i18n::TextKey::FileNamePlaceholder)
        };
        let editor = cx.new(move |cx| crate::text_input::TextInput::new(styles, placeholder, cx));
        cx.subscribe(
            &editor,
            |this, _, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_remote_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_remote_sidebar_edit(cx),
                _ => {}
            },
        )
        .detach();
        let focus = editor.focus_handle(cx);
        self.sidebar_edit_blur_sub = Some(cx.on_blur(&focus, window, |this, _, cx| {
            this.commit_remote_sidebar_edit(cx);
        }));
        window.focus(&focus);
        self.remote_sidebar_edit = Some(if directory {
            RemoteSidebarEditMode::CreateDirectory { parent, editor }
        } else {
            RemoteSidebarEditMode::CreateFile { parent, editor }
        });
        cx.notify();
    }

    pub(super) fn start_remote_sidebar_rename(
        &mut self,
        resource: ResourcePath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_edit_in_file_list = self.file_list_visible && self.file_list_menu_origin;
        let name = resource
            .path
            .file_name()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let styles = self.sidebar_edit_text_input_styles();
        let placeholder = self.t(crate::i18n::TextKey::FileNamePlaceholder);
        let editor = cx.new(move |cx| crate::text_input::TextInput::new(styles, placeholder, cx));
        editor.update(cx, |input, cx| {
            input.set_text(&name, cx);
            input.select_all(cx);
        });
        cx.subscribe(
            &editor,
            |this, _, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_remote_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_remote_sidebar_edit(cx),
                _ => {}
            },
        )
        .detach();
        let focus = editor.focus_handle(cx);
        self.sidebar_edit_blur_sub = Some(cx.on_blur(&focus, window, |this, _, cx| {
            this.commit_remote_sidebar_edit(cx);
        }));
        window.focus(&focus);
        self.remote_sidebar_edit = Some(RemoteSidebarEditMode::Rename { resource, editor });
        cx.notify();
    }

    fn commit_remote_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        let value = match self.remote_sidebar_edit.as_ref() {
            Some(RemoteSidebarEditMode::CreateFile { editor, .. })
            | Some(RemoteSidebarEditMode::CreateDirectory { editor, .. })
            | Some(RemoteSidebarEditMode::Rename { editor, .. }) => {
                editor.read(cx).text().to_string()
            }
            None => return,
        };
        if value.trim().is_empty() {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "name_required"),
                cx,
            );
            return;
        }
        let Some(edit) = self.remote_sidebar_edit.take() else {
            return;
        };
        self.sidebar_edit_blur_sub = None;
        match edit {
            RemoteSidebarEditMode::CreateFile { parent, editor } => {
                let _ = editor;
                let name = if Path::new(value.trim())
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| !extension.is_empty())
                {
                    value
                } else {
                    format!("{}.md", value.trim())
                };
                self.create_remote_entry(parent, name, false, cx);
            }
            RemoteSidebarEditMode::CreateDirectory { parent, editor } => {
                let _ = editor;
                self.create_remote_entry(parent, value, true, cx);
            }
            RemoteSidebarEditMode::Rename { resource, editor } => {
                let _ = editor;
                self.rename_remote_entry(resource, value, cx);
            }
        }
        cx.notify();
    }

    fn cancel_remote_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        self.remote_sidebar_edit = None;
        self.sidebar_edit_blur_sub = None;
        cx.notify();
    }

    fn create_remote_entry(
        &mut self,
        parent: ResourcePath,
        name: String,
        directory: bool,
        cx: &mut Context<Self>,
    ) {
        if name.trim().is_empty() {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "name_required"),
                cx,
            );
            return;
        }
        let path = match parent.join_child(name.trim()) {
            Ok(path) => path,
            Err(error) => {
                if let Some(input) = self.remote_input.as_mut() {
                    input.error = Some(error.to_string());
                }
                cx.notify();
                return;
            }
        };
        let Some(filesystem) = self
            .remote_workspace
            .as_ref()
            .map(|workspace| workspace.filesystem.clone())
        else {
            return;
        };
        let identity = filesystem.identity().clone();
        let generation = self.remote_generation;
        self.remote_input = None;
        cx.spawn(async move |this, cx| {
            let new_path = path.path.clone();
            let result = crate::platform::fs_task::run_write(
                if directory {
                    "SSH create directory"
                } else {
                    "SSH create file"
                },
                scheduler_key(),
                move || {
                    if directory {
                        filesystem.create_dir(&new_path, false).map_err(fs_error)
                    } else {
                        filesystem.create_file(&new_path).map_err(fs_error)
                    }
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation
                    || editor
                        .remote_workspace
                        .as_ref()
                        .is_none_or(|workspace| workspace.filesystem.identity() != &identity)
                {
                    return;
                }
                match result {
                    Ok(()) => {
                        if let Some(workspace) = editor.remote_workspace.as_mut() {
                            workspace.refreshing.insert(parent.clone());
                        }
                        editor.load_remote_directory(parent, cx);
                        if !directory {
                            editor.open_remote_file(path, cx);
                        }
                    }
                    Err(error) => editor.notify_error(error.to_string(), cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn rename_remote_entry(&mut self, source: ResourcePath, name: String, cx: &mut Context<Self>) {
        if self.all_tabs().any(|tab| {
            tab.remote.as_ref().is_some_and(|state| {
                state
                    .resource
                    .as_ref()
                    .is_some_and(|path| resource_is_at_or_below(path, &source))
                    && (state.save_uncertain || self.remote_saves_inflight.contains(&tab.id))
            })
        }) {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "save_uncertain"),
                cx,
            );
            return;
        }
        if name.trim().is_empty() {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "name_required"),
                cx,
            );
            return;
        }
        let Some(parent) = source.parent() else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "cannot_rename_root"),
                cx,
            );
            return;
        };
        let target = match parent.join_child(name.trim()) {
            Ok(target) => target,
            Err(error) => {
                if let Some(input) = self.remote_input.as_mut() {
                    input.error = Some(error.to_string());
                }
                cx.notify();
                return;
            }
        };
        if target == source {
            self.remote_input = None;
            cx.notify();
            return;
        }
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        let filesystem = workspace.filesystem.clone();
        let identity = filesystem.identity().clone();
        let generation = self.remote_generation;
        self.remote_input = None;
        cx.spawn(async move |this, cx| {
            let from = source.path.clone();
            let to = target.path.clone();
            let result =
                crate::platform::fs_task::run_write("SSH rename", scheduler_key(), move || {
                    filesystem.rename(&from, &to).map_err(fs_error)
                })
                .await;
            this.update(cx, |editor, cx| {
                if editor.remote_generation != generation
                    || editor
                        .remote_workspace
                        .as_ref()
                        .is_none_or(|workspace| workspace.filesystem.identity() != &identity)
                {
                    return;
                }
                match result {
                    Ok(()) => {
                        for document in editor.remote_documents.values_mut() {
                            if let Some(resource) = document
                                .resource
                                .as_ref()
                                .and_then(|resource| rebase_resource(resource, &source, &target))
                            {
                                document.resource = Some(resource);
                            }
                        }
                        for tab in editor.all_tabs_mut() {
                            let Some(state) = tab.remote.as_mut() else {
                                continue;
                            };
                            let Some(old_resource) = state.resource.clone() else {
                                continue;
                            };
                            let Some(resource) = rebase_resource(&old_resource, &source, &target)
                            else {
                                continue;
                            };
                            state.resource = Some(resource.clone());
                            if old_resource == source
                                && let Some(name) = resource.path.file_name()
                            {
                                tab.title = name.to_string();
                            }
                        }
                        for resource in &mut editor.app_settings.starred_resources {
                            if let Some(rebased) = rebase_resource(resource, &source, &target) {
                                *resource = rebased;
                            }
                        }
                        editor.save_settings_in_background(cx);
                        if let Some(workspace) = editor.remote_workspace.as_mut() {
                            workspace.file_list_dir =
                                rebase_resource(&workspace.file_list_dir, &source, &target)
                                    .unwrap_or_else(|| workspace.file_list_dir.clone());
                            workspace.selected = workspace.selected.as_ref().map(|selected| {
                                rebase_resource(selected, &source, &target)
                                    .unwrap_or_else(|| selected.clone())
                            });
                            workspace.selected_paths = workspace
                                .selected_paths
                                .iter()
                                .map(|selected| {
                                    rebase_resource(selected, &source, &target)
                                        .unwrap_or_else(|| selected.clone())
                                })
                                .collect();
                            workspace.selection_anchor =
                                workspace.selection_anchor.as_ref().map(|selected| {
                                    rebase_resource(selected, &source, &target)
                                        .unwrap_or_else(|| selected.clone())
                                });
                            workspace.focused = workspace.focused.as_ref().map(|selected| {
                                rebase_resource(selected, &source, &target)
                                    .unwrap_or_else(|| selected.clone())
                            });
                            workspace.expanded = workspace
                                .expanded
                                .iter()
                                .map(|path| {
                                    rebase_resource(path, &source, &target)
                                        .unwrap_or_else(|| path.clone())
                                })
                                .collect();
                            workspace.directories = std::mem::take(&mut workspace.directories)
                                .into_iter()
                                .map(|(path, mut entries)| {
                                    for entry in &mut entries {
                                        if let Some(resource) =
                                            rebase_resource(&entry.resource, &source, &target)
                                        {
                                            entry.name = resource
                                                .path
                                                .file_name()
                                                .map(|name| name.to_string())
                                                .unwrap_or_default();
                                            entry.resource = resource;
                                        }
                                    }
                                    (
                                        rebase_resource(&path, &source, &target).unwrap_or(path),
                                        entries,
                                    )
                                })
                                .collect();
                            workspace.previews.clear();
                            workspace.refreshing.insert(parent.clone());
                        }
                        editor.invalidate_sidebar_filter_index();
                        editor.load_remote_directory(parent, cx);
                        session_store::persist_session(editor);
                    }
                    Err(error) => editor.notify_error(error.to_string(), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn save_remote_active(
        &mut self,
        save_as: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_id) = self.active_tab_id.clone() else {
            return;
        };
        let Some(index) = self.active_tab_index() else {
            return;
        };
        if self.tabs[index].remote_text_unavailable() {
            self.notify_error(
                crate::i18n::runtime::RuntimeText::DocumentUnavailable.to_string(),
                cx,
            );
            return;
        }
        if self.remote_saves_inflight.contains(&tab_id) {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "save_uncertain"),
                cx,
            );
            return;
        }
        let Some((filesystem, root, destination, config_file)) =
            self.remote_workspace.as_ref().map(|workspace| {
                (
                    workspace.filesystem.clone(),
                    workspace.root.clone(),
                    workspace.destination.clone(),
                    workspace.config_file.clone(),
                )
            })
        else {
            let uncertain = self
                .remote_documents
                .get(&tab_id)
                .is_some_and(|document| document.save_uncertain)
                || self.tabs[index]
                    .remote
                    .as_ref()
                    .is_some_and(|remote| remote.save_uncertain);
            self.notify_error(
                crate::i18n::remote_text(
                    self.ui_language,
                    if uncertain {
                        "save_uncertain"
                    } else {
                        "not_connected"
                    },
                ),
                cx,
            );
            return;
        };
        if self.tabs[index].remote_binding_lost {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "remote_binding_lost"),
                cx,
            );
            return;
        }
        let existing = self.remote_documents.get(&tab_id).cloned();
        if existing
            .as_ref()
            .is_some_and(|document| document.save_uncertain)
            || self.tabs[index]
                .remote
                .as_ref()
                .is_some_and(|remote| remote.save_uncertain)
        {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "save_uncertain"),
                cx,
            );
            return;
        }
        if save_as
            || (self.tabs[index]
                .remote
                .as_ref()
                .is_some_and(|remote| remote.resource.is_none())
                && existing
                    .as_ref()
                    .and_then(|document| document.resource.as_ref())
                    .is_none())
        {
            let initial = self.tabs[index].title.trim().to_string();
            self.open_remote_input(RemoteInputMode::SaveAs(tab_id), initial, window, cx);
            return;
        }
        let resource = existing
            .as_ref()
            .and_then(|document| document.resource.clone())
            .or_else(|| {
                self.tabs[index]
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.resource.clone())
            })
            .or_else(|| {
                root.join_child(&format!("{}.md", self.tabs[index].title))
                    .ok()
            });
        let Some(resource) = resource else {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "save_path_error"),
                cx,
            );
            return;
        };
        if resource.filesystem != *filesystem.identity() {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "wrong_connection"),
                cx,
            );
            return;
        }
        let identity = filesystem.identity().clone();
        let expected = existing
            .as_ref()
            .and_then(|document| document.revision.clone())
            .or_else(|| {
                self.tabs[index]
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.revision.clone())
            })
            .map(ExpectedRevision::Present)
            .unwrap_or(ExpectedRevision::Missing);
        let text = self.normalize_text_for_save();
        let snapshot = self.current_snapshot();
        let generation = self.remote_generation;
        let pane = self.current_pane_id();
        let operation_id = next_remote_save_id();
        self.remote_saves_inflight.insert(tab_id.clone());
        let prior_state =
            self.tabs[index]
                .remote
                .clone()
                .unwrap_or_else(|| config_store::RemoteTabState {
                    peer_scope: self.remote_workspace.as_ref().and_then(|w| w.peer_scope),
                    destination: destination.clone(),
                    config_file: config_file.clone(),
                    root: root.clone(),
                    resource: None,
                    revision: None,
                    save_uncertain: false,
                    pending_resource: None,
                    line_ending_crlf: false,
                });
        let pending = pending_remote_save(&prior_state, &resource);
        self.record_remote_tab(&tab_id, pending, false);
        cx.spawn(async move |this, cx| {
            let save_bytes = serialize_remote_text(&text, prior_state.line_ending_crlf);
            let save_path = resource.path.clone();
            let result =
                crate::platform::fs_task::run_write("SSH save", scheduler_key(), move || {
                    filesystem
                        .save_bytes(operation_id, save_path, expected, &save_bytes)
                        .map_err(fs_error)
                })
                .await;
            this.update(cx, |editor, cx| {
                editor.remote_saves_inflight.remove(&tab_id);
                if matches!(
                    &result,
                    Ok(SaveOutcome::Conflict(_) | SaveOutcome::Failed(_))
                ) {
                    editor.record_remote_tab(
                        &tab_id,
                        prior_state.clone(),
                        editor.remote_generation != generation
                            || editor.remote_workspace.as_ref().is_none_or(|workspace| {
                                workspace.filesystem.identity() != &identity
                            }),
                    );
                }
                if editor.remote_generation != generation
                    || editor
                        .remote_workspace
                        .as_ref()
                        .is_none_or(|workspace| workspace.filesystem.identity() != &identity)
                    || editor.pane_id_for_tab(&tab_id) != Some(pane)
                {
                    return;
                }
                let outcome = match result {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        if let Some(document) = editor.remote_documents.get_mut(&tab_id) {
                            document.save_uncertain = true;
                        } else {
                            editor.remote_documents.insert(
                                tab_id.clone(),
                                RemoteDocument {
                                    resource: Some(resource.clone()),
                                    revision: None,
                                    save_uncertain: true,
                                },
                            );
                        }
                        editor.notify_error(
                            format!(
                                "{}: {error}",
                                crate::i18n::remote_text(editor.ui_language, "save_uncertain")
                            ),
                            cx,
                        );
                        return;
                    }
                };
                match outcome {
                    SaveOutcome::Saved(revision) => {
                        let Some(owner) = editor.pane_id_for_tab(&tab_id) else {
                            return;
                        };
                        editor.with_pane_if_alive(owner, |editor| {
                            let Some(index) = editor.tabs.iter().position(|tab| tab.id == tab_id)
                            else {
                                return;
                            };
                            let unchanged = if editor.active_tab_id.as_deref() == Some(&tab_id) {
                                editor.buffer.text() == text
                            } else {
                                editor.tabs[index].snapshot.text == text
                            };
                            editor.tabs[index].is_remote = true;
                            editor.tabs[index].path = None;
                            editor.tabs[index].is_deleted = false;
                            editor.tabs[index].history.saved_text = Some(text.clone());
                            editor.tabs[index].remote_binding_lost = false;
                            editor.tabs[index].remote = Some(config_store::RemoteTabState {
                                peer_scope: prior_state.peer_scope,
                                destination: destination.clone(),
                                config_file: config_file.clone(),
                                root: root.clone(),
                                resource: Some(resource.clone()),
                                revision: Some(revision.clone()),
                                save_uncertain: false,
                                pending_resource: None,
                                line_ending_crlf: prior_state.line_ending_crlf,
                            });
                            if unchanged {
                                editor.tabs[index].is_dirty = false;
                                editor.tabs[index].set_snapshot(snapshot.clone());
                                editor.clear_temp_file_for_tab(&tab_id);
                            }
                            editor.remote_documents.insert(
                                tab_id.clone(),
                                RemoteDocument {
                                    resource: Some(resource.clone()),
                                    revision: Some(revision),
                                    save_uncertain: false,
                                },
                            );
                            session_store::persist_session(editor);
                            cx.notify();
                        });
                    }
                    SaveOutcome::Conflict(_) => {
                        editor.notify_error(
                            crate::i18n::remote_text(editor.ui_language, "save_conflict"),
                            cx,
                        );
                    }
                    SaveOutcome::Uncertain(error) => {
                        editor.remote_documents.insert(
                            tab_id.clone(),
                            RemoteDocument {
                                resource: Some(resource.clone()),
                                revision: None,
                                save_uncertain: true,
                            },
                        );
                        editor.notify_error(
                            format!(
                                "{}: {error}",
                                crate::i18n::remote_text(editor.ui_language, "save_uncertain")
                            ),
                            cx,
                        );
                    }
                    SaveOutcome::Failed(error) => editor.notify_error(error.to_string(), cx),
                    SaveOutcome::InProgress | SaveOutcome::Unknown => {
                        if let Some(document) = editor.remote_documents.get_mut(&tab_id) {
                            document.save_uncertain = true;
                        }
                        editor.notify_error(
                            crate::i18n::remote_text(editor.ui_language, "save_uncertain"),
                            cx,
                        );
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn save_remote_as(&mut self, tab_id: &str, name: String, cx: &mut Context<Self>) {
        let Some(tab) = self.all_tabs().find(|tab| tab.id == tab_id) else {
            return;
        };
        if tab.remote_binding_lost
            || tab
                .remote
                .as_ref()
                .is_some_and(|state| state.save_uncertain)
            || self
                .remote_documents
                .get(tab_id)
                .is_some_and(|document| document.save_uncertain)
        {
            self.notify_error(
                crate::i18n::remote_text(
                    self.ui_language,
                    if tab.remote_binding_lost {
                        "remote_binding_lost"
                    } else {
                        "save_uncertain"
                    },
                ),
                cx,
            );
            return;
        }
        if name.trim().is_empty() {
            if let Some(input) = self.remote_input.as_mut() {
                input.error = Some(crate::i18n::remote_text(self.ui_language, "name_required"));
            }
            cx.notify();
            return;
        }
        let Some((filesystem, root, destination, config_file)) =
            self.remote_workspace.as_ref().map(|workspace| {
                (
                    workspace.filesystem.clone(),
                    workspace.root.clone(),
                    workspace.destination.clone(),
                    workspace.config_file.clone(),
                )
            })
        else {
            return;
        };
        let resource = match root.join_child(name.trim()) {
            Ok(resource) => resource,
            Err(error) => {
                if let Some(input) = self.remote_input.as_mut() {
                    input.error = Some(error.to_string());
                }
                cx.notify();
                return;
            }
        };
        let Some(owner) = self.pane_id_for_tab(tab_id) else {
            self.remote_input = None;
            return;
        };
        let content = self.with_pane(owner, |editor| {
            if editor.active_tab_id.as_deref() == Some(tab_id) {
                Some(editor.normalize_text_for_save())
            } else {
                editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == tab_id)
                    .map(|tab| tab.snapshot.text.clone())
            }
        });
        let Some(content) = content else {
            return;
        };
        let generation = self.remote_generation;
        let identity = filesystem.identity().clone();
        let operation_id = next_remote_save_id();
        let tab_id = tab_id.to_string();
        if !self.remote_saves_inflight.insert(tab_id.clone()) {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "save_uncertain"),
                cx,
            );
            return;
        }
        let prior_state = self
            .all_tabs()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| tab.remote.clone())
            .unwrap_or_else(|| config_store::RemoteTabState {
                peer_scope: self.remote_workspace.as_ref().and_then(|w| w.peer_scope),
                destination: destination.clone(),
                config_file: config_file.clone(),
                root: root.clone(),
                resource: None,
                revision: None,
                save_uncertain: false,
                pending_resource: None,
                line_ending_crlf: false,
            });
        let prior_binding_lost = self
            .all_tabs()
            .find(|tab| tab.id == tab_id)
            .is_some_and(|tab| tab.remote_binding_lost);
        let pending = pending_remote_save(&prior_state, &resource);
        self.record_remote_tab(&tab_id, pending, prior_binding_lost);
        self.remote_input = None;
        cx.spawn(async move |this, cx| {
            let bytes = serialize_remote_text(&content, prior_state.line_ending_crlf);
            let path = resource.path.clone();
            let result =
                crate::platform::fs_task::run_write("SSH save as", scheduler_key(), move || {
                    filesystem
                        .save_bytes(operation_id, path, ExpectedRevision::Missing, &bytes)
                        .map_err(fs_error)
                })
                .await;
            this.update(cx, |editor, cx| {
                editor.remote_saves_inflight.remove(&tab_id);
                let definitely_failed = matches!(
                    &result,
                    Ok(SaveOutcome::Conflict(_) | SaveOutcome::Failed(_))
                );
                if definitely_failed {
                    editor.record_remote_tab(
                        &tab_id,
                        prior_state.clone(),
                        prior_binding_lost
                            || editor.remote_generation != generation
                            || editor.remote_workspace.as_ref().is_none_or(|workspace| {
                                workspace.filesystem.identity() != &identity
                            }),
                    );
                }
                if editor.remote_generation != generation
                    || editor
                        .remote_workspace
                        .as_ref()
                        .is_none_or(|workspace| workspace.filesystem.identity() != &identity)
                    || editor.pane_id_for_tab(&tab_id) != Some(owner)
                {
                    return;
                }
                match result {
                    Ok(SaveOutcome::Saved(revision)) => {
                        let Some(owner) = editor.pane_id_for_tab(&tab_id) else {
                            return;
                        };
                        editor.with_pane_if_alive(owner, |editor| {
                            let live_text = (editor.active_tab_id.as_deref() == Some(&tab_id))
                                .then(|| editor.buffer.text() == content);
                            let mut unchanged = false;
                            if let Some(tab) = editor.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                tab.title = resource
                                    .path
                                    .file_name()
                                    .map(|name| name.to_string())
                                    .unwrap_or_default();
                                tab.path = None;
                                tab.is_remote = true;
                                tab.remote_binding_lost = false;
                                tab.remote = Some(config_store::RemoteTabState {
                                    peer_scope: editor
                                        .remote_workspace
                                        .as_ref()
                                        .and_then(|w| w.peer_scope),
                                    destination: destination.clone(),
                                    config_file: config_file.clone(),
                                    root: root.clone(),
                                    resource: Some(resource.clone()),
                                    revision: Some(revision.clone()),
                                    save_uncertain: false,
                                    pending_resource: None,
                                    line_ending_crlf: prior_state.line_ending_crlf,
                                });
                                tab.history.saved_text = Some(content.clone());
                                if live_text.unwrap_or_else(|| tab.snapshot.text == content) {
                                    tab.is_dirty = false;
                                    unchanged = true;
                                }
                            }
                            if unchanged {
                                editor.clear_temp_file_for_tab(&tab_id);
                            }
                            editor.remote_documents.insert(
                                tab_id.clone(),
                                RemoteDocument {
                                    resource: Some(resource.clone()),
                                    revision: Some(revision),
                                    save_uncertain: false,
                                },
                            );
                            session_store::persist_session(editor);
                            cx.notify();
                        });
                    }
                    Ok(SaveOutcome::Conflict(_)) => editor.notify_error(
                        crate::i18n::remote_text(editor.ui_language, "save_conflict"),
                        cx,
                    ),
                    Ok(SaveOutcome::Uncertain(error)) => {
                        editor.notify_error(
                            format!(
                                "{}: {error}",
                                crate::i18n::remote_text(editor.ui_language, "save_uncertain")
                            ),
                            cx,
                        );
                    }
                    Ok(SaveOutcome::Failed(error)) => editor.notify_error(error.to_string(), cx),
                    Ok(SaveOutcome::InProgress | SaveOutcome::Unknown) | Err(_) => {
                        editor.notify_error(
                            crate::i18n::remote_text(editor.ui_language, "save_uncertain"),
                            cx,
                        );
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn render_ssh_host_picker(
        &self,
        input: &RemoteInput,
        picker: &SshHostPicker,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.quick_open;
        let width = self.scaled(styles.quick_open_panel_width_px);
        let sidebar = self.sidebar_width();
        let available = (self.last_window_width - sidebar - self.right_panel_width()).max(width);
        let left = (sidebar + (available - width) / 2.0).max(0.0);
        let top =
            (self.last_window_height / 5.0).max(self.scaled(styles.quick_open_panel_top_offset_px));
        let query = input.editor.read(cx).text();
        let choices = if picker.destination.is_some() {
            picker.directories.clone()
        } else {
            ssh_host_choices(&picker.hosts, query)
        };
        let rows = choices.iter().enumerate().map(|(index, host)| {
            let host = host.clone();
            crate::ui::list::list_item(
                self.styles.command_item(),
                crate::style::ControlState {
                    selected: index == picker.selected_index,
                    disabled: picker.connecting,
                    ..Default::default()
                },
                self.ui_scale,
            )
            .id(("ssh-host", index))
            .debug_selector({
                let host = host.clone();
                move || format!("ssh-picker-item-{host}")
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(self.scaled(styles.quick_open_result_name_font_size_px)))
                    .text_color(styles.quick_open_result_text_color)
                    .child(host.clone()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    let Some(picker) = this
                        .remote_input
                        .as_ref()
                        .and_then(|input| input.ssh_picker.as_ref())
                    else {
                        return;
                    };
                    if picker.connecting {
                        return;
                    }
                    if picker.destination.is_some() {
                        this.select_ssh_directory(host.clone(), cx);
                    } else {
                        this.select_ssh_host(host.clone(), cx);
                    }
                }),
            )
        });
        div()
            .occlude()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(styles.quick_open_backdrop_bg)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.cancel_remote_input(cx);
                        }),
                    ),
            )
            .child(
                crate::ui::surface::surface(&self.styles.command_surface(), self.ui_scale)
                    .id("ssh-host-picker")
                    .debug_selector(|| "ssh-host-picker".into())
                    .absolute()
                    .top(px(top))
                    .left(px(left))
                    .w(px(width))
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                        }),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(
                                self.scaled(self.styles.ui.space(crate::style::SpaceStep::One))
                            ))
                            .px(px(self.scaled(styles.quick_open_input_padding_x_px)))
                            .py(px(self.scaled(styles.quick_open_input_padding_y_px)))
                            .border_b(px(self.ui_border_width(1.0)))
                            .border_color(styles.quick_open_panel_border_color)
                            .when(picker.destination.is_some(), |this| {
                                this.child(super::tooltip::with_tooltip(
                                    crate::ui::button::icon_button(
                                        "ssh-host-back",
                                        self.styles.workspace_toggle(),
                                        Default::default(),
                                        self.ui_scale,
                                    )
                                    .debug_selector(|| "ssh-host-back".into())
                                    .child(
                                        gpui::svg()
                                            .path("assets/icons/navigation/arrow-left.svg")
                                            .size(px(self.scaled(
                                                self.styles.workspace_toggle().icon_size_px,
                                            )))
                                            .text_color(
                                                self.styles.workspace_toggle().normal.foreground,
                                            )
                                            .debug_selector(|| "ssh-host-back-icon".into()),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, window, cx| {
                                            this.back_to_ssh_hosts(window, cx);
                                        }),
                                    ),
                                    cx.entity(),
                                    crate::i18n::remote_text(self.ui_language, "back_to_hosts"),
                                    None::<SharedString>,
                                ))
                            })
                            .child(div().flex_1().min_w(px(0.0)).child(input.editor.clone())),
                    )
                    .child(
                        div()
                            .id("ssh-host-results")
                            .w_full()
                            .max_h(px(self.scaled(styles.quick_open_results_max_height_px)))
                            .overflow_y_scroll()
                            .track_scroll(&picker.scroll_handle)
                            .children(rows)
                            .when(choices.is_empty() || picker.connecting, |this| {
                                this.child(
                                    div()
                                        .px(px(self.scaled(styles.quick_open_input_padding_x_px)))
                                        .py(px(self.scaled(styles.quick_open_input_padding_y_px)))
                                        .text_size(px(
                                            self.scaled(styles.quick_open_result_path_font_size_px)
                                        ))
                                        .text_color(styles.quick_open_result_secondary_text_color)
                                        .child(if picker.connecting {
                                            format!(
                                                "{}...",
                                                crate::i18n::remote_text(
                                                    self.ui_language,
                                                    "connect"
                                                )
                                            )
                                        } else if picker.loading {
                                            "~/.ssh/config ...".to_string()
                                        } else if picker.directory_loading {
                                            crate::i18n::remote_text(self.ui_language, "searching")
                                        } else if picker.destination.is_some() {
                                            self.t(crate::i18n::TextKey::SearchNoResults)
                                                .to_string()
                                        } else {
                                            crate::i18n::remote_text(
                                                self.ui_language,
                                                "host_placeholder",
                                            )
                                        }),
                                )
                            }),
                    )
                    .when_some(input.error.clone(), |this, error| {
                        this.child(
                            div()
                                .px(px(self.scaled(styles.quick_open_input_padding_x_px)))
                                .py(px(self.scaled(styles.quick_open_input_padding_y_px)))
                                .text_size(px(
                                    self.scaled(styles.quick_open_result_path_font_size_px)
                                ))
                                .text_color(self.styles.sidebar.sidebar_error_text_color)
                                .child(error),
                        )
                    })
                    .child(self.render_search_shortcut_footer(
                        crate::i18n::search_menu_footer_label(self.ui_language, "execute"),
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn render_remote_input(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let directory_query_stale = self.remote_input.as_ref().is_some_and(|input| {
            input.ssh_picker.as_ref().is_some_and(|picker| {
                picker.destination.is_some()
                    && picker.browsing_filesystem.is_some()
                    && !picker.connecting
                    && picker.directory_query_value != input.editor.read(cx).text()
            })
        });
        if directory_query_stale {
            self.queue_ssh_directory_query(cx);
        }
        let Some(input) = self.remote_input.as_ref() else {
            return div().into_any_element();
        };
        if let Some(picker) = input.ssh_picker.as_ref() {
            return self.render_ssh_host_picker(input, picker, cx);
        }
        let mode_name = match &input.mode {
            RemoteInputMode::Connect => "add_ssh_directory",
            RemoteInputMode::SaveAs(_) => "save_as_title",
        };
        let title = crate::i18n::remote_text(self.ui_language, mode_name);
        let instruction = match &input.mode {
            RemoteInputMode::Connect => {
                crate::i18n::remote_text(self.ui_language, "connect_instruction")
            }
            _ => crate::i18n::remote_text(self.ui_language, "name_instruction"),
        };
        let ds = &self.styles.dialog;
        let editor = input.editor.clone();
        let error = input.error.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(ds.dialog_backdrop_color)
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .child(
                crate::ui::surface::surface(&self.styles.dialog_surface(), 1.0)
                    .w(px(self.scaled(self.styles.dialog_width())))
                    .px(px(self.scaled(ds.dialog_padding_x_px)))
                    .py(px(self.scaled(ds.dialog_padding_y_px)))
                    .flex()
                    .flex_col()
                    .gap(px(self.scaled(ds.dialog_gap_px)))
                    .child(
                        div()
                            .text_size(px(self.scaled(ds.dialog_title_font_size_px)))
                            .text_color(ds.dialog_title_text_color)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(self.scaled(ds.dialog_message_font_size_px)))
                            .text_color(ds.dialog_message_text_color)
                            .child(instruction),
                    )
                    .child(editor)
                    .when_some(error, |this, error| {
                        this.child(
                            div()
                                .text_size(px(self.scaled(ds.dialog_message_font_size_px)))
                                .text_color(self.styles.sidebar.sidebar_error_text_color)
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(self.scaled(ds.dialog_btn_gap_px)))
                            .child(
                                crate::ui::button::button(
                                    "remote-cancel",
                                    self.styles.dialog_button(
                                        ds.dialog_btn_secondary_bg,
                                        ds.dialog_btn_secondary_text_color,
                                        ds.dialog_btn_secondary_hover_bg,
                                    ),
                                    Default::default(),
                                    self.ui_scale,
                                )
                                .child(crate::i18n::remote_text(self.ui_language, "cancel"))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.cancel_remote_input(cx)),
                                ),
                            )
                            .child(
                                crate::ui::button::button(
                                    "remote-submit",
                                    self.styles.dialog_button(
                                        ds.dialog_btn_primary_bg,
                                        ds.dialog_btn_primary_text_color,
                                        ds.dialog_btn_primary_hover_bg,
                                    ),
                                    Default::default(),
                                    self.ui_scale,
                                )
                                .child(crate::i18n::remote_text(self.ui_language, "continue"))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _window, cx| {
                                        this.submit_remote_input(cx);
                                    }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn visible_remote_tree_rows(&self) -> Vec<(RemoteDirectoryEntry, usize)> {
        let mut rows = Vec::new();
        if self.sidebar_filter_active() {
            let mut hidden_depth = None;
            for (entry, depth) in &self.remote_sidebar_filter_rows {
                if hidden_depth.is_some_and(|hidden| *depth > hidden) {
                    continue;
                }
                hidden_depth = (entry.metadata.is_dir
                    && self
                        .remote_sidebar_filter_collapsed
                        .contains(&entry.resource))
                .then_some(*depth);
                rows.push((entry.clone(), *depth));
            }
        } else if let Some(workspace) = &self.remote_workspace {
            self.collect_remote_rows(&workspace.root, 1, &mut rows);
        }
        rows
    }

    fn remote_visible_resources(&self) -> Vec<(ResourcePath, bool)> {
        self.visible_remote_tree_rows()
            .into_iter()
            .map(|(entry, _)| (entry.resource, entry.metadata.is_dir))
            .collect()
    }

    pub(super) fn select_remote_sidebar_item(
        &mut self,
        path: &ResourcePath,
        shift: bool,
        cmd: bool,
    ) {
        self.focused_workspace_root = None;
        let visible: Vec<_> = if shift {
            self.remote_visible_resources()
                .into_iter()
                .map(|(path, _)| path)
                .collect()
        } else {
            Vec::new()
        };
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        super::file_tree::select_tree_item(
            &mut workspace.selected_paths,
            &mut workspace.selection_anchor,
            path,
            &visible,
            shift,
            cmd,
        );
        workspace.selected = Some(path.clone());
        workspace.focused = Some(path.clone());
    }

    pub(super) fn remote_file_tree_context_resources(
        &self,
        resource: &ResourcePath,
    ) -> Vec<ResourcePath> {
        let mut resources = self
            .remote_workspace
            .as_ref()
            .filter(|workspace| workspace.selected_paths.contains(resource))
            .map(|workspace| workspace.selected_paths.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_else(|| vec![resource.clone()]);
        resources.sort_by_key(|resource| resource.path.to_string().len());
        let mut filtered = Vec::<ResourcePath>::new();
        for resource in resources {
            if !filtered
                .iter()
                .any(|parent| resource_is_at_or_below(&resource, parent))
            {
                filtered.push(resource);
            }
        }
        filtered
    }

    pub(super) fn toggle_remote_sidebar_directory(
        &mut self,
        path: ResourcePath,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_filter_active() {
            if !self.remote_sidebar_filter_collapsed.remove(&path) {
                self.remote_sidebar_filter_collapsed.insert(path);
            }
            cx.notify();
            return;
        }
        let Some(workspace) = self.remote_workspace.as_mut() else {
            return;
        };
        workspace.file_list_dir = path.clone();
        if !workspace.expanded.remove(&path) {
            workspace.expanded.insert(path.clone());
            self.load_remote_directory(path, cx);
        }
        cx.notify();
    }

    pub(super) fn remote_sidebar_has_focus(&self) -> bool {
        self.remote_session_active()
            && self
                .remote_workspace
                .as_ref()
                .is_some_and(|workspace| workspace.focused.is_some())
    }

    pub(super) fn navigate_remote_sidebar(&mut self, delta: isize, cx: &mut Context<Self>) {
        let visible = self.remote_visible_resources();
        if visible.is_empty() {
            return;
        }
        let current = self
            .remote_workspace
            .as_ref()
            .and_then(|workspace| workspace.focused.as_ref())
            .and_then(|focused| visible.iter().position(|(path, _)| path == focused));
        let next = match current {
            Some(index) => (index as isize + delta).clamp(0, visible.len() as isize - 1) as usize,
            None if delta < 0 => visible.len() - 1,
            None => 0,
        };
        let path = visible[next].0.clone();
        self.select_remote_sidebar_item(&path, false, false);
        if let Some(workspace) = &self.remote_workspace {
            self.ensure_file_tree_item_minimally_visible(
                &workspace.tree_scroll,
                next,
                visible.len(),
            );
        }
        cx.notify();
    }

    fn remote_directory_is_expanded(&self, path: &ResourcePath) -> bool {
        if self.sidebar_filter_active() {
            !self.remote_sidebar_filter_collapsed.contains(path)
        } else {
            self.remote_workspace
                .as_ref()
                .is_some_and(|w| w.expanded.contains(path))
        }
    }

    pub(super) fn remote_sidebar_nav_left(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .remote_workspace
            .as_ref()
            .and_then(|workspace| workspace.focused.clone())
        else {
            return;
        };
        if self
            .remote_workspace
            .as_ref()
            .is_some_and(|w| w.root == path)
        {
            return;
        }
        if self.remote_directory_is_expanded(&path)
            && self
                .remote_visible_resources()
                .iter()
                .any(|(item, dir)| *dir && item == &path)
        {
            self.toggle_remote_sidebar_directory(path, cx);
        } else if let Some(parent) = path.parent()
            && self
                .remote_workspace
                .as_ref()
                .is_some_and(|workspace| resource_is_at_or_below(&parent, &workspace.root))
        {
            self.select_remote_sidebar_item(&parent, false, false);
            cx.notify();
        }
    }

    pub(super) fn remote_sidebar_nav_right(&mut self, cx: &mut Context<Self>) {
        let visible = self.remote_visible_resources();
        let Some(path) = self
            .remote_workspace
            .as_ref()
            .and_then(|workspace| workspace.focused.clone())
        else {
            return;
        };
        let is_root = self
            .remote_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.root == path);
        if is_root {
            if let Some((first, _)) = visible.first() {
                let first = first.clone();
                self.select_remote_sidebar_item(&first, false, false);
                if let Some(workspace) = &self.remote_workspace {
                    self.ensure_file_tree_item_minimally_visible(
                        &workspace.tree_scroll,
                        0,
                        visible.len(),
                    );
                }
                cx.notify();
            }
            return;
        }
        let is_dir = visible
            .iter()
            .find(|(item, _)| item == &path)
            .is_some_and(|(_, is_dir)| *is_dir);
        if !is_dir {
            return;
        }
        if !self.remote_directory_is_expanded(&path) {
            self.toggle_remote_sidebar_directory(path, cx);
        } else if let Some((child, _)) = visible
            .iter()
            .find(|(item, _)| item.parent().as_ref() == Some(&path))
        {
            let child = child.clone();
            self.select_remote_sidebar_item(&child, false, false);
            cx.notify();
        }
    }

    pub(super) fn remote_sidebar_confirm_focused(&mut self, cx: &mut Context<Self>) {
        let visible = self.remote_visible_resources();
        let Some(path) = self
            .remote_workspace
            .as_ref()
            .and_then(|workspace| workspace.focused.clone())
        else {
            return;
        };
        if self
            .remote_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.root == path)
        {
            self.remote_sidebar_nav_right(cx);
            return;
        }
        let is_dir = visible
            .iter()
            .find(|(item, _)| item == &path)
            .is_some_and(|(_, is_dir)| *is_dir);
        if is_dir {
            self.toggle_remote_sidebar_directory(path, cx);
        } else {
            self.open_remote_file(path, cx);
        }
    }

    pub(super) fn remote_sidebar_delete_focused(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .remote_workspace
            .as_ref()
            .and_then(|workspace| workspace.focused.clone())
        else {
            return;
        };
        if self
            .remote_workspace
            .as_ref()
            .is_some_and(|workspace| path != workspace.root)
        {
            self.confirm_resource_trash(self.remote_file_tree_context_resources(&path), cx);
        }
    }

    fn render_remote_entry(
        &self,
        entry: &RemoteDirectoryEntry,
        depth: usize,
        active_guide_levels: &HashSet<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(RemoteSidebarEditMode::Rename { resource, editor }) = &self.remote_sidebar_edit
            && resource == &entry.resource
            && !self.sidebar_edit_in_file_list
        {
            return self.render_sidebar_edit_row(editor, depth, entry.metadata.is_dir, cx);
        }
        let selected = self.file_tree_resource_selected(&entry.resource);
        let focused = self.file_tree_resource_focused(&entry.resource);
        let cut = self.resource_clipboard.as_ref().is_some_and(|clipboard| {
            clipboard.is_cut
                && clipboard
                    .resources
                    .iter()
                    .any(|resource| resource_is_at_or_below(&entry.resource, resource))
        });
        let expanded = self.remote_directory_is_expanded(&entry.resource);
        let path = entry.resource.clone();
        let context_path = path.clone();
        let is_dir = entry.metadata.is_dir;
        let id = resource_key(&path);
        let drag_resources = self.dragged_remote_resources(&entry.resource);
        let drag_label = entry.name.clone();
        let drop_target = if is_dir {
            entry.resource.clone()
        } else {
            entry
                .resource
                .parent()
                .unwrap_or_else(|| entry.resource.clone())
        };
        let drag_over_bg = self.styles.sidebar.sidebar_item_selected_bg;
        let ghost_style = self.styles.drag_preview(false, self.ui_scale);
        let visual =
            self.render_file_tree_row_visual(super::sidebar_render::FileTreeRowVisualSpec {
                name: &entry.name,
                depth,
                is_dir,
                is_expanded: expanded,
                selected,
                focused,
                active_guide_levels,
                dir_file_count: None,
            });
        crate::ui::list::tree_row(
            self.styles.sidebar_tree_row(),
            crate::style::TreeRowState {
                selected,
                focused,
                drag_over: false,
                cut,
                resizing: self.sidebar_resizing,
            },
            self.ui_scale,
        )
        .id(SharedString::from(format!("remote-{id}")))
        .debug_selector(|| format!("remote-{id}"))
        .w_full()
        .h(px(self.sidebar_item_height_px()))
        .flex_none()
        .children(visual.guides)
        .child(visual.content)
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                this.open_file_tree_resource_context_menu(
                    context_path.clone(),
                    is_dir,
                    event,
                    window,
                    cx,
                );
            }),
        )
        .on_click(
            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                this.activate_file_tree_resource(path.clone(), is_dir, event, window, cx);
            }),
        )
        .when(!self.sidebar_resizing, |row| {
            let count = drag_resources.len();
            row.on_drag(
                super::DraggedResources {
                    resources: drag_resources,
                },
                move |_dragged, offset, _window, cx| {
                    cx.new(|_cx| super::DraggedSidebarItemsView {
                        label: drag_label.clone(),
                        count,
                        style: ghost_style.clone(),
                        cursor_offset: offset,
                    })
                },
            )
        })
        .drag_over::<super::DraggedResources>(move |style, _, _, _| style.bg(drag_over_bg))
        .drag_over::<super::DraggedSidebarItems>(move |style, _, _, _| style.bg(drag_over_bg))
        .on_drop({
            let target = drop_target.clone();
            cx.listener(move |this, dragged: &super::DraggedResources, window, cx| {
                let copy = window.modifiers().alt;
                this.drop_resources_into(dragged.resources.clone(), target.clone(), copy, cx);
            })
        })
        .on_drop({
            let target = drop_target;
            cx.listener(
                move |this, dragged: &super::DraggedSidebarItems, window, cx| {
                    let copy = window.modifiers().alt;
                    let resources = dragged.paths.iter().map(|path| ResourcePath::local(path));
                    this.drop_resources_into(resources.collect(), target.clone(), copy, cx);
                },
            )
        })
        .into_any_element()
    }

    /// The rows a drag starting on `resource` carries: the whole selection when
    /// the row is part of it.
    fn dragged_remote_resources(&self, resource: &ResourcePath) -> Vec<ResourcePath> {
        match self.remote_workspace.as_ref() {
            Some(workspace) if workspace.selected_paths.contains(resource) => {
                super::file_tree::filter_resource_roots(
                    workspace.selected_paths.iter().cloned().collect(),
                )
            }
            _ => vec![resource.clone()],
        }
    }

    fn collect_remote_rows(
        &self,
        path: &ResourcePath,
        depth: usize,
        rows: &mut Vec<(RemoteDirectoryEntry, usize)>,
    ) {
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        if depth > 64 {
            return;
        }
        let Some(entries) = workspace.directories.get(path) else {
            return;
        };
        for entry in entries {
            if moeka_fs::search::should_skip_name(&entry.name, entry.metadata.is_dir)
                || self.file_list_visible && !entry.metadata.is_dir
            {
                continue;
            }
            rows.push((entry.clone(), depth));
            if entry.metadata.is_dir && workspace.expanded.contains(&entry.resource) {
                self.collect_remote_rows(&entry.resource, depth + 1, rows);
            }
        }
    }

    pub(super) fn select_remote_file_list_item(
        &mut self,
        resource: &ResourcePath,
        shift: bool,
        secondary: bool,
    ) {
        let Some(workspace) = &mut self.remote_workspace else {
            return;
        };
        let visible = workspace
            .directories
            .get(&workspace.file_list_dir)
            .into_iter()
            .flatten()
            .filter(|entry| remote_file_list_entry(entry))
            .map(|entry| entry.resource.clone())
            .collect::<Vec<_>>();
        super::file_tree::select_tree_item(
            &mut workspace.selected_paths,
            &mut workspace.selection_anchor,
            resource,
            &visible,
            shift,
            secondary,
        );
        workspace.selected = Some(resource.clone());
        workspace.focused = Some(resource.clone());
        self.focused_workspace_root = None;
        self.focused_starred_path = None;
    }

    pub(super) fn render_remote_file_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_remote_sort_current();
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return div()
                .w(px(self.file_list_layout_width()))
                .h_full()
                .bg(self.styles.sidebar.file_list_bg)
                .into_any_element();
        };
        let directory = workspace.file_list_dir.clone();
        let scroll = workspace.file_list_scroll.clone();
        self.load_remote_directory(directory.clone(), cx);
        let entries = self
            .remote_workspace
            .as_ref()
            .and_then(|workspace| workspace.directories.get(&directory))
            .into_iter()
            .flatten()
            .filter(|entry| remote_file_list_entry(entry))
            .cloned()
            .collect::<Vec<_>>();
        let row_height = self.file_list_item_height_px();
        let create_editor = match &self.remote_sidebar_edit {
            Some(RemoteSidebarEditMode::CreateFile { parent, editor })
                if self.sidebar_edit_in_file_list && parent == &directory =>
            {
                Some(editor.clone())
            }
            _ => None,
        };
        let prefix_height = if create_editor.is_some() {
            row_height
        } else {
            0.0
        };
        let is_empty = entries.is_empty() && create_editor.is_none();
        let visible = super::list_viewport::fixed_list_window(
            &scroll,
            self.last_window_height,
            prefix_height,
            row_height,
            entries.len(),
            8,
            16,
        );
        self.load_remote_file_previews(&entries[visible.preload.clone()], cx);
        let retained_thumbnails = self
            .remote_workspace
            .as_ref()
            .into_iter()
            .flat_map(|workspace| {
                entries[visible.preload.clone()].iter().flat_map(|entry| {
                    workspace
                        .previews
                        .get(&entry.resource)
                        .and_then(|preview| preview.details.as_ref().ok())
                        .into_iter()
                        .flat_map(|details| &details.image_paths)
                        .filter_map(|target| remote_relative_resource(&entry.resource, target).ok())
                        .map(|resource| format!("ssh-thumbnail-{}", resource_key(&resource)))
                })
            })
            .collect::<HashSet<_>>();
        self.retired_images.extend(
            self.image_cache
                .extract_if(|key, _| {
                    key.starts_with("ssh-thumbnail-") && !retained_thumbnails.contains(key)
                })
                .filter_map(|(_, image)| image),
        );
        let ss = self.styles.sidebar.clone();
        let generation = self.remote_generation;
        let mut rows = Vec::new();
        for entry in entries
            .iter()
            .skip(visible.render.start)
            .take(visible.render.len())
        {
            if let Some(RemoteSidebarEditMode::Rename { resource, editor }) =
                &self.remote_sidebar_edit
                && self.sidebar_edit_in_file_list
                && resource == &entry.resource
            {
                rows.push(self.render_file_list_edit_row(editor));
                continue;
            }
            let details = self
                .remote_workspace
                .as_ref()
                .and_then(|workspace| workspace.previews.get(&entry.resource))
                .and_then(|preview| preview.details.as_ref().ok())
                .cloned();
            let mut thumbnail = None;
            for target in details
                .as_ref()
                .into_iter()
                .flat_map(|details| &details.image_paths)
            {
                let Ok(resource) = remote_relative_resource(&entry.resource, target) else {
                    continue;
                };
                let key = format!("ssh-thumbnail-{}", resource_key(&resource));
                match self.image_cache.get(&key) {
                    Some(Some(image)) => {
                        thumbnail = Some(image.clone());
                        break;
                    }
                    Some(None) => continue,
                    None => {
                        self.queue_remote_image(resource, true, cx);
                        break;
                    }
                }
            }
            let path = entry.resource.clone();
            let context_path = path.clone();
            let selected = self.file_tree_resource_selected(&path)
                || self
                    .active_tab_index()
                    .and_then(|index| self.tabs[index].remote.as_ref())
                    .and_then(|remote| remote.resource.as_ref())
                    == Some(&path);
            let thumbnail = thumbnail.map(|image| {
                crate::ui::decoration::thumbnail(
                    self.styles.file_thumbnail(),
                    self.scaled(self.file_list_item_content_height_px()),
                    self.ui_scale,
                )
                .child(super::file_list::FileListImageElement {
                    image,
                    radius: px(self.scaled(self.styles.file_thumbnail().radius_px)),
                })
                .into_any_element()
            });
            rows.push(
                self.render_file_list_row_content(
                    super::file_list::FileListRowContent {
                        name: &entry.name,
                        details: details.as_ref(),
                        modified: remote_metadata_time(entry.metadata.modified_unix_nanos),
                        created: remote_metadata_time(
                            entry
                                .metadata
                                .created_unix_nanos
                                .or(entry.metadata.modified_unix_nanos),
                        ),
                        selected,
                        thumbnail,
                    },
                    cx,
                )
                .id(SharedString::from(format!(
                    "remote-file-list-{}",
                    resource_key(&path)
                )))
                .on_drag(
                    super::DraggedResources {
                        resources: self.dragged_remote_resources(&path),
                    },
                    {
                        let label = entry.name.clone();
                        let count = self.dragged_remote_resources(&path).len();
                        let ghost_style = self.styles.drag_preview(false, self.ui_scale);
                        move |_dragged, offset, _window, cx| {
                            cx.new(|_cx| super::DraggedSidebarItemsView {
                                label: label.clone(),
                                count,
                                style: ghost_style.clone(),
                                cursor_offset: offset,
                            })
                        }
                    },
                )
                .on_click(
                    cx.listener(move |editor, event: &gpui::ClickEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        if editor.validate_resource_menu(&path, generation, cx) {
                            let modifiers = event.modifiers();
                            editor.select_file_list_resource(
                                &path,
                                modifiers.shift,
                                modifiers.secondary(),
                            );
                            if !modifiers.shift && !modifiers.secondary() {
                                editor.reveal_resource_in_tree(path.clone(), cx);
                                editor.open_remote_file(path.clone(), cx);
                            }
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |editor, event: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if editor.validate_resource_menu(&context_path, generation, cx) {
                            if !editor.file_tree_resource_selected(&context_path) {
                                editor.select_file_list_resource(&context_path, false, false);
                            }
                            editor.open_file_tree_resource_context_menu(
                                context_path.clone(),
                                false,
                                event,
                                window,
                                cx,
                            );
                            editor.file_list_menu_origin = true;
                        }
                    }),
                )
                .into_any_element(),
            );
        }
        let menu_directory = directory.clone();
        div()
            .id("remote-file-list")
            .w(px(self.file_list_layout_width()))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .relative()
            .bg(ss.file_list_bg)
            .border_r(px(ss.sidebar_resize_indicator_width_px))
            .border_color(ss.sidebar_resize_indicator_color)
            .child(
                self.render_resource_file_list_header(
                    directory.clone(),
                    self.remote_workspace
                        .as_ref()
                        .is_some_and(|workspace| workspace.root == directory),
                    entries.len(),
                    cx,
                ),
            )
            .child(
                div()
                    .id("remote-file-list-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .on_scroll_wheel(cx.listener(|editor, _, _, cx| {
                        editor.reveal_panel_scrollbar(PanelScrollbarKind::FileList, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |editor, event: &gpui::MouseDownEvent, window, cx| {
                            editor.open_file_tree_resource_context_menu(
                                menu_directory.clone(),
                                true,
                                event,
                                window,
                                cx,
                            );
                            editor.file_list_menu_origin = true;
                        }),
                    )
                    .when_some(create_editor, |this, editor| {
                        this.child(self.render_file_list_edit_row(&editor))
                    })
                    .child(div().h(px(visible.top_spacer_px)).flex_none())
                    .children(rows)
                    .when(is_empty, |this| {
                        this.child(
                            crate::ui::typography::label(
                                crate::style::TextStyle::new(
                                    ss.file_list_empty_font_size_px,
                                    ss.file_list_empty_text_color,
                                ),
                                self.ui_scale,
                            )
                            .w_full()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(self.t(crate::i18n::TextKey::FileListEmpty)),
                        )
                    })
                    .child(div().h(px(visible.bottom_spacer_px)).flex_none())
                    .child(
                        div()
                            .h(px(self.scaled(ss.sidebar_bottom_spacer_height_px)))
                            .flex_none(),
                    ),
            )
            .when_some(
                self.render_scrollbar_for_handle(
                    PanelScrollbarKind::FileList,
                    &scroll,
                    self.last_window_height,
                    0.0,
                    self.scaled(self.styles.self_drawn.scrollbar.right_margin_px),
                    cx,
                ),
                |this, scrollbar| this.child(scrollbar),
            )
            .into_any_element()
    }

    pub(crate) fn render_remote_sidebar(
        &mut self,
        refresh: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stars = self.visible_starred_resources();
        self.prepare_starred_motion(&stars);
        let starred_section = self.render_starred_sidebar_section(&stars, cx);
        if self.remote_workspace.is_none() {
            return div()
                .id("remote-sidebar")
                .w(px(self.sidebar_layout_width()))
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .bg(self.styles.sidebar.sidebar_bg)
                .child(self.render_sidebar_top_spacer(cx))
                .child(starred_section)
                .children(
                    self.workspaces
                        .iter()
                        .filter(|root| self.sidebar_lists_local_workspace(root))
                        .cloned()
                        .map(|root| self.render_workspace_root(root, cx)),
                )
                .children(self.render_remote_workspace_rows(cx))
                .child(div().flex_1())
                .child(self.render_sidebar_settings_item(cx))
                .into_any_element();
        }
        let _ = refresh;
        self.ensure_remote_sort_current();
        self.ensure_sidebar_filter_input(cx);
        self.ensure_sidebar_filter_current(cx);
        let workspace = self.remote_workspace.as_ref().unwrap();
        let scroll = workspace.tree_scroll.clone();
        let root = workspace.root.clone();
        let error = workspace
            .error
            .clone()
            .or_else(|| workspace.watch_warning.clone())
            .or_else(|| workspace.connection_warning.clone());
        let rows = self.visible_remote_tree_rows();
        let ss = self.styles.sidebar.clone();
        let mut list = Vec::<AnyElement>::new();
        {
            let guide_levels = super::sidebar_motion::file_tree_guide_levels(
                &rows.iter().map(|(_, depth)| *depth).collect::<Vec<_>>(),
            );
            let create_edit = match &self.remote_sidebar_edit {
                Some(RemoteSidebarEditMode::CreateFile { parent, editor }) => (!self
                    .sidebar_edit_in_file_list)
                    .then(|| (parent.clone(), editor.clone(), false)),
                Some(RemoteSidebarEditMode::CreateDirectory { parent, editor }) => {
                    Some((parent.clone(), editor.clone(), true))
                }
                _ => None,
            };
            let visible = super::list_viewport::fixed_list_window(
                &scroll,
                self.last_window_height,
                0.0,
                self.scaled(ss.sidebar_item_height_px),
                rows.len(),
                8,
                8,
            );
            let render_range = if create_edit.is_some() {
                0..rows.len()
            } else {
                visible.render.clone()
            };
            list.push(
                div()
                    .h(px(if create_edit.is_some() {
                        0.0
                    } else {
                        visible.top_spacer_px
                    }))
                    .flex_none()
                    .into_any_element(),
            );
            if let Some((parent, editor, directory)) = &create_edit
                && parent == &root
            {
                list.push(self.render_sidebar_edit_row(editor, 1, *directory, cx));
            }
            for (index, (entry, depth)) in rows
                .into_iter()
                .enumerate()
                .skip(render_range.start)
                .take(render_range.len())
            {
                list.push(self.render_remote_entry(&entry, depth, &guide_levels[index], cx));
                if let Some((parent, editor, directory)) = &create_edit
                    && parent == &entry.resource
                {
                    list.push(self.render_sidebar_edit_row(editor, depth + 1, *directory, cx));
                }
            }
            list.push(
                div()
                    .h(px(if create_edit.is_some() {
                        0.0
                    } else {
                        visible.bottom_spacer_px
                    }))
                    .flex_none()
                    .into_any_element(),
            );
        }
        let top_spacer = self.render_sidebar_top_spacer(cx);
        div()
            .id("remote-sidebar")
            .w(px(self.sidebar_layout_width()))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .relative()
            .bg(ss.sidebar_bg)
            .when(self.sidebar_resizing, |this| {
                this.border_r(px(self.scaled(ss.sidebar_resize_active_indicator_width_px)))
                    .border_color(ss.sidebar_resize_active_indicator_color)
            })
            .when(!self.sidebar_resizing, |this| {
                this.border_r(px(ss.sidebar_resize_indicator_width_px))
                    .border_color(ss.sidebar_resize_indicator_color)
            })
            .child(top_spacer)
            .child(self.render_sidebar_filter(cx))
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .w_full()
                        .px(px(self.scaled(ss.sidebar_error_padding_x_px)))
                        .py(px(self.scaled(ss.sidebar_error_padding_y_px)))
                        .text_color(ss.sidebar_error_text_color)
                        .text_size(px(self.scaled(ss.sidebar_error_font_size_px)))
                        .child(error),
                )
            })
            .child(starred_section)
            .child(self.render_workspace_roots(cx))
            .child(
                div()
                    .id("remote-tree")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .children(list),
            )
            .child(self.render_sidebar_settings_item(cx))
            .into_any_element()
    }

    /// Rows for every connected SSH workspace in connection order, so switching
    /// the active one never reorders the list.
    pub(super) fn render_remote_workspace_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows: Vec<(u64, Option<usize>)> = self
            .remote_workspace
            .iter()
            .map(|workspace| (workspace.serial, None))
            .chain(
                self.parked_remote_workspaces
                    .iter()
                    .enumerate()
                    .map(|(index, workspace)| (workspace.serial, Some(index))),
            )
            .collect();
        rows.sort_by_key(|(serial, _)| *serial);
        let strip = self.workspace_strip_visible();
        let mut elements: Vec<AnyElement> = rows
            .into_iter()
            // The strip lists every workspace; the sidebar keeps the active one.
            .filter(|(_, parked)| !strip || (parked.is_none() && self.remote_workspace_active))
            .map(|(_, parked)| self.render_remote_workspace_row(parked, cx))
            .collect();
        if !strip {
            for (index, pending) in self.pending_ssh_workspaces().into_iter().enumerate() {
                elements.push(self.render_pending_remote_workspace_row(index, pending, cx));
            }
        }
        elements
    }

    /// `(serial, parked index, workspace)` in connection order.
    pub(super) fn remote_workspaces_in_order(&self) -> Vec<(u64, Option<usize>, &RemoteWorkspace)> {
        let mut all: Vec<_> = self
            .remote_workspace
            .iter()
            .map(|workspace| (workspace.serial, None, workspace))
            .chain(
                self.parked_remote_workspaces
                    .iter()
                    .enumerate()
                    .map(|(index, workspace)| (workspace.serial, Some(index), workspace)),
            )
            .collect();
        all.sort_by_key(|(serial, ..)| *serial);
        all
    }

    pub(super) fn pending_ssh_workspace_count(&self) -> usize {
        self.pending_ssh_workspaces().len()
    }

    /// Display name and whether it failed, for each saved workspace still connecting.
    pub(super) fn pending_ssh_workspace_labels(&self) -> Vec<(String, bool)> {
        self.pending_ssh_workspaces()
            .into_iter()
            .map(|pending| {
                let failed = match pending.extra {
                    None => self.ssh_restore_error.is_some(),
                    Some(_) => self.ssh_restore_errors.contains_key(&Self::ssh_restore_key(
                        &pending.destination,
                        &pending.root.path,
                    )),
                };
                let name = pending
                    .root
                    .path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| pending.root.path.to_string())
                    .to_uppercase();
                (name, failed)
            })
            .collect()
    }

    pub(super) fn retry_pending_ssh_workspace_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_ssh_workspaces().into_iter().nth(index) else {
            return;
        };
        let failed = match pending.extra {
            None => self.ssh_restore_error.is_some(),
            Some(_) => self.ssh_restore_errors.contains_key(&Self::ssh_restore_key(
                &pending.destination,
                &pending.root.path,
            )),
        };
        if failed {
            self.retry_pending_ssh_workspace(pending, cx);
        }
    }

    /// Makes the SSH workspace with this connection serial the active one.
    pub(super) fn activate_remote_workspace_serial(&mut self, serial: u64, cx: &mut Context<Self>) {
        if let Some(index) = self
            .parked_remote_workspaces
            .iter()
            .position(|workspace| workspace.serial == serial)
        {
            self.switch_remote_workspace(index, cx);
        }
        self.activate_remote_workspace(cx);
        if let Some(root) = self.remote_workspace.as_ref().map(|w| w.root.clone()) {
            self.select_remote_sidebar_item(&root, false, false);
            if self.file_list_visible {
                if let Some(workspace) = self.remote_workspace.as_mut() {
                    workspace.file_list_dir = root.clone();
                }
                self.load_remote_directory(root, cx);
            }
        }
    }

    /// Moves one SSH workspace next to another in the displayed order.
    pub(super) fn reorder_remote_workspaces(&mut self, source: u64, target: u64, after: bool) {
        if source == target {
            return;
        }
        let mut order: Vec<u64> = self
            .remote_workspaces_in_order()
            .into_iter()
            .map(|(serial, ..)| serial)
            .collect();
        let (Some(from), true) = (
            order.iter().position(|serial| *serial == source),
            order.contains(&target),
        ) else {
            return;
        };
        let moved = order.remove(from);
        let to = order.iter().position(|serial| *serial == target).unwrap() + usize::from(after);
        order.insert(to, moved);
        for workspace in self
            .remote_workspace
            .iter_mut()
            .chain(self.parked_remote_workspaces.iter_mut())
        {
            if let Some(position) = order.iter().position(|serial| *serial == workspace.serial) {
                workspace.serial = position as u64;
            }
        }
    }

    /// A saved workspace that is still connecting (or offline): its name carries
    /// the state as a prefix instead of a separate status label.
    fn render_pending_remote_workspace_row(
        &self,
        index: usize,
        pending: PendingSshWorkspace,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let error = match pending.extra {
            None => self.ssh_restore_error.clone(),
            Some(_) => self
                .ssh_restore_errors
                .get(&Self::ssh_restore_key(
                    &pending.destination,
                    &pending.root.path,
                ))
                .cloned(),
        };
        let name = pending
            .root
            .path
            .file_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| pending.root.path.to_string())
            .to_uppercase();
        let state = crate::i18n::remote_text(
            self.ui_language,
            if error.is_some() {
                "offline"
            } else {
                "reconnecting"
            },
        );
        let label = format!("[{state}] {name}");
        let tooltip = error.clone().unwrap_or_else(|| state.to_string());
        let offline = error.is_some();
        super::tooltip::with_tooltip(
            crate::ui::list::tree_row(
                self.styles.sidebar_tree_row(),
                Default::default(),
                self.ui_scale,
            )
            .opacity(0.5)
            .id(("pending-ssh-root", index))
            .pl(px(self.scaled(ss.sidebar_root_padding_x_px)))
            .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_item_gap_px)))
            .child(
                gpui::svg()
                    .path(ROOT_SVG_PATH)
                    .size(px(self.scaled(ss.sidebar_header_root_icon_size_px)))
                    .flex_none()
                    .text_color(ss.sidebar_item_icon_color),
            )
            .child(
                crate::ui::typography::badge(self.styles.sidebar_kind_badge(false), self.ui_scale)
                    .flex_none()
                    .child("REMOTE"),
            )
            .child(
                crate::ui::typography::label(
                    self.styles.workspace_root_label(false),
                    self.ui_scale,
                )
                .min_w(px(0.0))
                .flex_1()
                .truncate()
                .child(label),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if offline {
                    this.retry_pending_ssh_workspace(pending.clone(), cx);
                }
            })),
            cx.entity(),
            tooltip,
            None::<SharedString>,
        )
    }

    /// One sidebar row per connected SSH workspace: the active one (`None`)
    /// or a parked one, which switches in when clicked.
    pub(super) fn render_remote_workspace_row(
        &self,
        parked: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(workspace) = (match parked {
            None => self.remote_workspace.as_ref(),
            Some(index) => self.parked_remote_workspaces.get(index),
        }) else {
            return div().into_any_element();
        };
        let current = parked.is_none() && self.remote_workspace_active;
        let ss = &self.styles.sidebar;
        let root = workspace.root.clone();
        let mut label = root
            .path
            .file_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| root.path.to_string())
            .to_uppercase();
        if workspace.reconnecting {
            label = format!(
                "[{}] {label}",
                crate::i18n::remote_text(self.ui_language, "reconnecting")
            );
        }
        let id: gpui::ElementId = match parked {
            None => "remote-workspace-root".into(),
            Some(index) => ("remote-workspace-parked-root", index).into(),
        };
        let row = crate::ui::list::tree_row(
            self.styles.sidebar_tree_row(),
            crate::style::TreeRowState {
                selected: current && self.file_tree_resource_selected(&root),
                resizing: self.sidebar_resizing,
                ..Default::default()
            },
            self.ui_scale,
        )
        .opacity(if current { 1.0 } else { 0.5 })
        .id(id)
        .pl(px(self.scaled(ss.sidebar_root_padding_x_px)))
        .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(self.scaled(ss.sidebar_item_gap_px)));
        let row = if parked.is_none() {
            row.debug_selector(|| "remote-workspace-root".into())
        } else {
            row
        };
        row.child(
            gpui::svg()
                .path(ROOT_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_header_root_icon_size_px)))
                .flex_none()
                .text_color(if current {
                    ss.sidebar_item_selected_icon_color
                } else {
                    ss.sidebar_item_icon_color
                }),
        )
        .child(
            crate::ui::typography::badge(self.styles.sidebar_kind_badge(false), self.ui_scale)
                .flex_none()
                .when(parked.is_none(), |tag| {
                    tag.debug_selector(|| "remote-workspace-tag".into())
                })
                .child("REMOTE"),
        )
        .child(
            crate::ui::typography::label(self.styles.workspace_root_label(current), self.ui_scale)
                .min_w(px(0.0))
                .flex_1()
                .truncate()
                .child(label),
        )
        .when(current, |this| {
            this.child(self.render_remote_workspace_actions(root.clone(), cx))
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                if let Some(index) = parked {
                    this.switch_remote_workspace(index, cx);
                }
                this.activate_remote_workspace(cx);
                if let Some(root) = this
                    .remote_workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone())
                {
                    this.select_remote_sidebar_item(&root, false, false);
                    if this.file_list_visible {
                        if let Some(workspace) = this.remote_workspace.as_mut() {
                            workspace.file_list_dir = root.clone();
                        }
                        this.load_remote_directory(root, cx);
                    }
                }
                window.focus(&this.focus_handle);
            }),
        )
        .drag_over::<super::DraggedResources>({
            let bg = ss.sidebar_item_selected_bg;
            move |style, _, _, _| style.bg(bg)
        })
        .drag_over::<super::DraggedSidebarItems>({
            let bg = ss.sidebar_item_selected_bg;
            move |style, _, _, _| style.bg(bg)
        })
        .on_drop({
            let target = root.clone();
            cx.listener(move |this, dragged: &super::DraggedResources, window, cx| {
                let copy = window.modifiers().alt;
                this.drop_resources_into(dragged.resources.clone(), target.clone(), copy, cx);
            })
        })
        .on_drop({
            let target = root.clone();
            cx.listener(
                move |this, dragged: &super::DraggedSidebarItems, window, cx| {
                    let copy = window.modifiers().alt;
                    let resources = dragged.paths.iter().map(|path| ResourcePath::local(path));
                    this.drop_resources_into(resources.collect(), target.clone(), copy, cx);
                },
            )
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
                if let Some(index) = parked {
                    // The menu acts on the active workspace.
                    this.switch_remote_workspace(index, cx);
                    this.activate_remote_workspace(cx);
                }
                this.open_file_tree_resource_context_menu(root.clone(), true, event, window, cx);
            }),
        )
        .into_any_element()
    }

    fn render_remote_workspace_actions(
        &self,
        root: ResourcePath,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_header_action_gap_px)))
            .child(self.render_sidebar_header_action_button(
                "remote-sidebar-search",
                FILETREE_SEARCH_SVG_PATH,
                crate::i18n::sidebar_action_label(self.ui_language, "search"),
                None,
                cx,
                |this, _, window, cx| this.toggle_workspace_search(window, cx),
            ))
            .child(self.render_sidebar_header_action_button(
                "remote-sidebar-sort",
                FILETREE_SORT_SVG_PATH,
                crate::i18n::sidebar_action_label(self.ui_language, "sort"),
                None,
                cx,
                |this, event, _, cx| this.open_sidebar_sort_menu(event.position, cx),
            ))
            .child(self.render_sidebar_header_action_button(
                "remote-sidebar-create",
                FILETREE_ADD_SVG_PATH,
                crate::i18n::sidebar_action_label(self.ui_language, "create"),
                None,
                cx,
                move |this, event, _, cx| {
                    this.open_remote_create_menu(root.clone(), event.position, cx);
                },
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_failure_does_not_restart_from_render_until_explicit_invalidation() {
        let path = ResourcePath {
            filesystem: FilesystemId::Ssh([1; 32]),
            path: NativePath::unix_absolute("/workspace").unwrap(),
        };
        let mut attempts = DirectoryLoadAttempts::default();
        assert!(attempts.should_start(&path, false, false, false));
        assert!(!attempts.should_start(&path, false, true, false));
        attempts.finish(&path, false);
        for _ in 0..100 {
            assert!(!attempts.should_start(&path, false, false, false));
        }
        assert!(attempts.should_start(&path, false, false, true));
        attempts.finish(&path, true);
        assert!(!attempts.should_start(&path, true, false, false));
        assert!(!attempts.should_start(&path, true, true, true));
        assert!(attempts.should_start(&path, true, false, true));
    }

    #[test]
    fn remote_file_timestamps_preserve_subseconds_and_missing_values() {
        assert_eq!(remote_metadata_time(None), None);
        assert_eq!(remote_metadata_time(Some(i128::MAX)), None);
        for nanos in [0, 1, -1, 1_700_000_000_123_456_789] {
            let value = remote_metadata_time(Some(nanos)).unwrap();
            let actual = match value.duration_since(std::time::UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos() as i128,
                Err(error) => -(error.duration().as_nanos() as i128),
            };
            assert_eq!(actual, nanos);
        }
    }

    #[test]
    fn remote_preview_failure_preserves_accepted_text_and_caches_initial_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        moeka_fs::local::native::write(&path, "# Saved\n\nPreview").unwrap();
        let entry = RemoteDirectoryEntry {
            resource: ResourcePath {
                filesystem: FilesystemId::Ssh([1; 32]),
                path: NativePath::unix_absolute("/workspace/note.md").unwrap(),
            },
            name: "note.md".into(),
            metadata: moeka_fs::local::native::metadata(&path).unwrap().into(),
        };
        let mut previews = HashMap::new();
        retain_remote_preview(&mut previews, &entry, Err("offline".into()));
        assert!(previews[&entry.resource].details.is_err());
        retain_remote_preview(
            &mut previews,
            &entry,
            Ok(super::super::file_list::FileListDetails {
                title: Some("Saved".into()),
                preview: "Preview".into(),
                image_paths: vec!["image.png".into()],
            }),
        );
        retain_remote_preview(&mut previews, &entry, Err("offline".into()));
        let details = previews[&entry.resource].details.as_ref().unwrap();
        assert_eq!(details.title.as_deref(), Some("Saved"));
        assert_eq!(details.preview, "Preview");
        assert_eq!(details.image_paths, ["image.png"]);
        let mut hidden = entry.clone();
        hidden.name = ".secret.md".into();
        assert!(remote_file_list_entry(&entry));
        assert!(!remote_file_list_entry(&hidden));
        hidden.name = "photo.png".into();
        assert!(!remote_file_list_entry(&hidden));
    }

    #[test]
    fn ssh_picker_filters_aliases_and_keeps_manual_destinations() {
        let hosts = vec!["production".into(), "staging".into(), "PROD-backup".into()];
        assert_eq!(ssh_host_choices(&hosts, ""), hosts);
        assert_eq!(
            ssh_host_choices(&hosts, "PROD"),
            ["production", "PROD-backup", "PROD"]
        );
        assert_eq!(ssh_host_choices(&hosts, "STAGING"), ["staging"]);
        assert_eq!(ssh_host_choices(&hosts, "me@new-host"), ["me@new-host"]);
        assert_eq!(
            ssh_host_choices(&hosts, "me@new-host /work with spaces"),
            ["me@new-host /work with spaces"]
        );
    }

    #[test]
    fn ssh_directory_query_splits_remote_unix_paths_without_local_path_rules() {
        assert_eq!(ssh_directory_query("/"), Some(("/", "")));
        assert_eq!(ssh_directory_query("/tmp/fo"), Some(("/tmp", "fo")));
        assert_eq!(ssh_directory_query("/a b/"), Some(("/a b", "")));
        assert_eq!(ssh_directory_query("relative"), None);
        assert_eq!(ssh_directory_query("/a\0b"), None);
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn ssh_picker_keyboard_selects_host_then_directory_and_escape_cancels(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        struct PickerHarness {
            editor: gpui::Entity<EditorView>,
            _subscription: gpui::Subscription,
        }
        impl gpui::Render for PickerHarness {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                self.editor.update(cx, |editor, cx| {
                    div().size_full().child(editor.render_remote_input(cx))
                })
            }
        }
        cx.update(|cx| {
            cx.bind_keys([
                gpui::KeyBinding::new(
                    "down",
                    crate::text_input::TextInputPickerDown,
                    Some("TextInput"),
                ),
                gpui::KeyBinding::new(
                    "up",
                    crate::text_input::TextInputPickerUp,
                    Some("TextInput"),
                ),
                gpui::KeyBinding::new(
                    "enter",
                    crate::text_input::TextInputSubmit,
                    Some("TextInput"),
                ),
            ]);
        });
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.last_window_width = 1000.0;
                editor.last_window_height = 800.0;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| PickerHarness {
            editor: editor.clone(),
            _subscription: cx.observe(&editor, |_, _, cx| cx.notify()),
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let base_placeholder_alpha = editor
                    .quick_open_text_input_styles()
                    .text_input_placeholder_color
                    .a;
                editor.on_connect_ssh(&crate::actions::ConnectSsh, window, cx);
                assert!(
                    editor
                        .remote_input
                        .as_ref()
                        .unwrap()
                        .editor
                        .read(cx)
                        .styles
                        .text_input_placeholder_color
                        .a
                        < base_placeholder_alpha
                );
                // The fixture owns discovery; reject the real background read.
                editor.remote_input_generation += 1;
                let picker = editor
                    .remote_input
                    .as_mut()
                    .unwrap()
                    .ssh_picker
                    .as_mut()
                    .unwrap();
                picker.hosts = vec!["alpha".into(), "beta".into()];
                picker.loading = false;
                picker.disable_browsing_for_test = true;
            });
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("ssh-host-picker").is_some());
        cx.simulate_keystrokes("down enter");
        cx.update(|_, cx| {
            let input = editor
                .read(cx)
                .remote_input
                .as_ref()
                .unwrap()
                .editor
                .read(cx);
            assert_eq!(input.text(), "/");
            assert_eq!(input.selection_range(), 1..1);
        });
        cx.simulate_input("w");
        cx.update(|_, cx| {
            let input = editor
                .read(cx)
                .remote_input
                .as_ref()
                .unwrap()
                .editor
                .read(cx);
            assert_eq!(input.text(), "/w");
            assert_eq!(input.selection_range(), 2..2);
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                let input = editor.remote_input.as_mut().unwrap();
                let picker = input.ssh_picker.as_mut().unwrap();
                picker.directories = vec!["/workspace/".into()];
                input
                    .editor
                    .update(cx, |input, cx| input.set_text("/wor", cx));
                editor.update_ssh_completion(cx);
            });
            let input = editor.read(cx).remote_input.as_ref().unwrap();
            assert_eq!(
                input.ssh_picker.as_ref().unwrap().destination.as_deref(),
                Some("beta")
            );
            assert_eq!(input.editor.read(cx).text(), "/wor");
        });
        cx.simulate_keystrokes("tab");
        cx.update(|_, cx| {
            assert_eq!(
                editor
                    .read(cx)
                    .remote_input
                    .as_ref()
                    .unwrap()
                    .editor
                    .read(cx)
                    .text(),
                "/workspace/"
            );
        });
        cx.run_until_parked();
        let back = cx
            .debug_bounds("ssh-host-back")
            .expect("SSH host back button");
        let icon = cx
            .debug_bounds("ssh-host-back-icon")
            .expect("SSH host back icon");
        assert!(icon.size.width > px(0.0) && icon.size.height > px(0.0));
        assert!(back.contains(&icon.center()));
        cx.simulate_click(back.center(), Default::default());
        cx.update(|_, cx| {
            let input = editor.read(cx).remote_input.as_ref().unwrap();
            assert!(input.ssh_picker.as_ref().unwrap().destination.is_none());
            assert_eq!(input.editor.read(cx).text(), "");
            assert_eq!(
                input.editor.read(cx).placeholder.as_ref(),
                crate::i18n::remote_text(editor.read(cx).ui_language, "host_placeholder")
            );
        });
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| assert!(editor.read(cx).remote_input.is_none()));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.on_connect_ssh(&crate::actions::ConnectSsh, window, cx);
                editor.remote_input_generation += 1;
                let picker = editor
                    .remote_input
                    .as_mut()
                    .unwrap()
                    .ssh_picker
                    .as_mut()
                    .unwrap();
                picker.hosts = vec!["alpha".into(), "beta".into()];
                picker.loading = false;
                picker.disable_browsing_for_test = true;
            });
        });
        cx.simulate_input("new@host");
        cx.simulate_keystrokes("enter");
        cx.update(|_, cx| {
            assert_eq!(
                editor
                    .read(cx)
                    .remote_input
                    .as_ref()
                    .unwrap()
                    .ssh_picker
                    .as_ref()
                    .unwrap()
                    .destination
                    .as_deref(),
                Some("new@host")
            );
        });
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| assert!(editor.read(cx).remote_input.is_none()));
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn root_escape_action_closes_the_ssh_picker(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext;
        struct RootHarness(gpui::Entity<EditorView>);
        impl gpui::Render for RootHarness {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0.clone()
            }
        }
        let editor =
            cx.update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, None, false)));
        let (_, cx) = cx.add_window_view(|_, _| RootHarness(editor.clone()));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.on_connect_ssh(&crate::actions::ConnectSsh, window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert!(editor.read(cx).remote_input.is_some()));
        cx.dispatch_action(crate::actions::Escape);
        cx.update(|_, cx| assert!(editor.read(cx).remote_input.is_none()));
    }

    #[cfg(all(unix, feature = "gpui-test-support"))]
    #[gpui::test]
    #[ignore = "requires an isolated loopback sshd, disposable HOME, and embedded native helper"]
    fn loopback_editor_opens_saves_reloads_and_preserves_disconnected_drafts(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        use moeka_fs::local::native as fs;

        struct RemoteHarness {
            editor: gpui::Entity<EditorView>,
            _subscription: gpui::Subscription,
        }
        impl gpui::Render for RemoteHarness {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                self.editor.update(cx, |editor, cx| {
                    div()
                        .size_full()
                        .flex()
                        .child(editor.render_sidebar(false, cx))
                        .child(editor.render_file_list(cx))
                        .child(editor.render_remote_input(cx))
                })
            }
        }

        fn wait_for(
            cx: &mut gpui::VisualTestContext,
            editor: &gpui::Entity<EditorView>,
            phase: &str,
            predicate: impl Fn(&EditorView) -> bool,
        ) {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                cx.run_until_parked();
                if cx.update(|_, cx| predicate(editor.read(cx))) {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "{phase}: {:?}",
                    cx.update(|_, cx| {
                        let editor = editor.read(cx);
                        editor.remote_input.as_ref().map(|input| {
                            let picker = input.ssh_picker.as_ref();
                            (
                                input.editor.read(cx).text().to_string(),
                                input.error.clone(),
                                picker.map(|picker| picker.directory_query_generation),
                                picker.map(|picker| picker.directories.clone()),
                                picker.is_some_and(|picker| picker.browsing_filesystem.is_some()),
                            )
                        })
                    })
                );
                cx.executor().advance_clock(Duration::from_millis(50));
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        let config =
            PathBuf::from(std::env::var_os("MOEKA_LOOPBACK_CONFIG").expect("loopback SSH config"));
        let home =
            PathBuf::from(std::env::var_os("MOEKA_LOOPBACK_HOME").expect("disposable remote HOME"));
        let options = SshOptions {
            destination: "moeka-loopback-test".into(),
            config_file: Some(config.clone()),
            port: None,
        };
        let output = options
            .command()
            .unwrap()
            .arg("printf '%s' \"$HOME\"")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            Path::new(std::str::from_utf8(&output.stdout).unwrap()),
            home
        );
        let workspace = tempfile::tempdir_in(&home).unwrap();
        let local_workspace = tempfile::tempdir().unwrap();
        let local_star = local_workspace.path().join("local-star.md");
        fs::write(&local_star, "# local star\n").unwrap();
        fs::create_dir(workspace.path().join("nested-folder")).unwrap();
        fs::create_dir(workspace.path().join("folder-copy")).unwrap();
        fs::write(
            workspace.path().join("folder-copy/child.md"),
            "# copied directory\n",
        )
        .unwrap();
        let note = workspace.path().join("note.md");
        fs::write(&note, "# initial needle\n\n![thumbnail](image.png)\n").unwrap();
        fs::write(workspace.path().join("late.md"), "# late\n").unwrap();
        image::RgbaImage::from_pixel(1024, 512, image::Rgba([1, 2, 3, 255]))
            .save(workspace.path().join("image.png"))
            .unwrap();
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor =
                    EditorView::new_with_mode(cx, true, false, Some("# local draft"), false);
                editor.workspaces = vec![local_workspace.path().to_path_buf()];
                editor.sidebar_root = local_workspace.path().to_path_buf();
                editor.file_list_dir = local_workspace.path().to_path_buf();
                editor.app_settings.starred_files = vec![local_star.clone()];
                editor.app_settings.starred_resources.clear();
                editor.sidebar_starred_collapsed = false;
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.);
                editor.last_window_height = 700.;
                editor
            })
        });
        let local_tab = cx.update(|cx| editor.read(cx).active_tab_id.clone().unwrap());
        let (_, cx) = cx.add_window_view(|_, cx| RemoteHarness {
            editor: editor.clone(),
            _subscription: cx.observe(&editor, |_, _, cx| cx.notify()),
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_remote_input(
                    RemoteInputMode::Connect,
                    "moeka-loopback-test".into(),
                    window,
                    cx,
                );
                editor
                    .remote_input
                    .as_mut()
                    .unwrap()
                    .ssh_picker
                    .as_mut()
                    .unwrap()
                    .config_file = Some(config.clone());
                editor.submit_remote_input(cx);
                let input = editor.remote_input.as_ref().unwrap();
                assert_eq!(
                    input.ssh_picker.as_ref().unwrap().destination.as_deref(),
                    Some("moeka-loopback-test")
                );
                assert!(input.ssh_picker.as_ref().unwrap().connecting);
                assert_eq!(input.editor.read(cx).text(), "");
                assert_eq!(
                    input.editor.read(cx).placeholder.as_ref(),
                    crate::i18n::remote_text(editor.ui_language, "installing_placeholder")
                );
            })
        });
        wait_for(cx, &editor, "directory connection", |editor| {
            editor
                .remote_input
                .as_ref()
                .and_then(|input| input.ssh_picker.as_ref())
                .is_some_and(|picker| picker.browsing_filesystem.is_some() && !picker.connecting)
        });
        wait_for(cx, &editor, "initial root directories", |editor| {
            editor
                .remote_input
                .as_ref()
                .and_then(|input| input.ssh_picker.as_ref())
                .is_some_and(|picker| picker.directories.iter().any(|path| path == "/Users/"))
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("ssh-picker-item-/Users/").is_some());
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .remote_input
                    .as_ref()
                    .unwrap()
                    .editor
                    .update(cx, |input, cx| input.set_text("/User", cx));
                cx.notify();
            });
        });
        wait_for(cx, &editor, "root directory suggestions", |editor| {
            editor
                .remote_input
                .as_ref()
                .and_then(|input| input.ssh_picker.as_ref())
                .is_some_and(|picker| picker.directories == ["/Users/"])
        });
        cx.update(|_, cx| {
            assert_eq!(
                editor
                    .read(cx)
                    .remote_input
                    .as_ref()
                    .unwrap()
                    .editor
                    .read(cx)
                    .completion_suffix(),
                Some("s/")
            );
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .remote_input
                    .as_ref()
                    .unwrap()
                    .editor
                    .update(cx, |input, cx| input.set_text("/Users/", cx));
                cx.notify();
            });
        });
        wait_for(cx, &editor, "user directory suggestions", |editor| {
            editor
                .remote_input
                .as_ref()
                .and_then(|input| input.ssh_picker.as_ref())
                .is_some_and(|picker| {
                    picker
                        .directories
                        .iter()
                        .any(|path| path == "/Users/Shared/")
                        && picker
                            .directories
                            .iter()
                            .any(|path| path.ends_with("/imeoer/"))
                })
        });
        let query = format!("{}/nested", workspace.path().display());
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .remote_input
                    .as_ref()
                    .unwrap()
                    .editor
                    .update(cx, |input, cx| {
                        input.set_text(&query, cx);
                    });
                editor.queue_ssh_directory_query(cx);
            });
        });
        wait_for(cx, &editor, "directory suggestions", |editor| {
            editor
                .remote_input
                .as_ref()
                .and_then(|input| input.ssh_picker.as_ref())
                .is_some_and(|picker| picker.directories == vec![format!("{query}-folder/")])
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                let input = editor.remote_input.as_ref().unwrap();
                input.editor.update(cx, |input, cx| {
                    input.set_text(workspace.path().to_str().unwrap(), cx);
                });
                editor.submit_remote_input(cx);
            })
        });
        wait_for(cx, &editor, "connect", |editor| {
            editor.remote_workspace.is_some()
        });
        cx.run_until_parked();
        let remote_root = cx
            .debug_bounds("remote-workspace-root")
            .expect("remote workspace root");
        let star_row = cx
            .debug_bounds(Box::leak(
                format!("sidebar-starred-{}", local_star.display()).into_boxed_str(),
            ))
            .expect("local favorite remains visible without remote favorites");
        assert!(star_row.bottom() <= remote_root.top());
        let sort = cx
            .debug_bounds("remote-sidebar-sort")
            .expect("remote sort action");
        let create = cx
            .debug_bounds("remote-sidebar-create")
            .expect("remote create action");
        assert_eq!(sort.origin.y, create.origin.y);
        assert!(sort.origin.y >= remote_root.origin.y);
        assert!(
            create.origin.y + create.size.height <= remote_root.origin.y + remote_root.size.height
        );
        assert!(cx.debug_bounds("sidebar-settings").is_some());
        assert!(cx.debug_bounds("workspace-roots").is_some());
        cx.simulate_click(create.center(), Default::default());
        cx.update(|_, cx| {
            assert!(matches!(
                editor.read(cx).context_menu.as_ref().map(|menu| &menu.kind),
                Some(ContextMenuKind::RemoteCreate { .. })
            ));
            editor.update(cx, |editor, cx| editor.close_context_menu_immediately(cx));
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let root = editor.remote_workspace.as_ref().unwrap().root.clone();
                editor.start_remote_sidebar_create(root, false, window, cx);
            });
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("sidebar-edit-row").is_some());
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| editor.cancel_remote_sidebar_edit(cx));
        });
        assert!(
            cx.debug_bounds(Box::leak(
                format!("workspace-root-{}", local_workspace.path().display()).into_boxed_str()
            ))
            .is_some()
        );
        let (note_resource, late_resource, nested_resource) = cx.update(|_, cx| {
            let root = editor
                .read(cx)
                .remote_workspace
                .as_ref()
                .unwrap()
                .root
                .clone();
            (
                root.join_child("note.md").unwrap(),
                root.join_child("late.md").unwrap(),
                root.join_child("nested-folder").unwrap(),
            )
        });
        let note_selector =
            Box::leak(format!("remote-{}", resource_key(&note_resource)).into_boxed_str());
        let note_bounds = cx
            .debug_bounds(note_selector)
            .expect("shared remote file row");
        cx.update(|_, cx| {
            let row_height = editor.read(cx).sidebar_item_height_px();
            assert!(
                (f32::from(note_bounds.size.height) - row_height).abs() < 0.5,
                "remote row={} expected={row_height}",
                f32::from(note_bounds.size.height)
            );
            editor.update(cx, |editor, cx| {
                editor.select_remote_sidebar_item(&note_resource, false, false);
                editor.select_remote_sidebar_item(&late_resource, true, false);
                assert!(
                    editor
                        .remote_workspace
                        .as_ref()
                        .unwrap()
                        .selected_paths
                        .len()
                        >= 2
                );
                editor.select_remote_sidebar_item(&nested_resource, false, false);
                editor.remote_sidebar_nav_right(cx);
                assert!(
                    editor
                        .remote_workspace
                        .as_ref()
                        .unwrap()
                        .expanded
                        .contains(&nested_resource)
                );
                editor.remote_sidebar_nav_left(cx);
                assert!(
                    !editor
                        .remote_workspace
                        .as_ref()
                        .unwrap()
                        .expanded
                        .contains(&nested_resource)
                );
                assert!(editor.sidebar_has_keyboard_focus());
            });
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert!(editor.remote_workspace_active);
                assert!(editor.all_tabs().any(|tab| tab.id == local_tab));
                let local_scroll = editor.sidebar_scroll_handle.clone();
                let local_list_scroll = editor.file_list_scroll_handle.clone();
                local_scroll.set_offset(gpui::point(px(0.0), px(-40.0)));
                local_list_scroll.set_offset(gpui::point(px(0.0), px(-60.0)));
                let workspace = editor.remote_workspace.as_ref().unwrap();
                let remote_scroll = workspace.tree_scroll.clone();
                let remote_list_scroll = workspace.file_list_scroll.clone();
                let expanded = workspace.expanded.clone();
                remote_scroll.set_offset(gpui::point(px(0.0), px(-80.0)));
                remote_list_scroll.set_offset(gpui::point(px(0.0), px(-100.0)));
                editor.switch_workspace(local_workspace.path().to_path_buf(), cx);
                assert!(!editor.remote_workspace_active);
                assert_eq!(editor.sidebar_root, local_workspace.path());
                assert_eq!(editor.sidebar_scroll_handle.offset().y, px(-40.0));
                assert_eq!(editor.file_list_scroll_handle.offset().y, px(-60.0));
                assert!(editor.all_tabs().any(|tab| tab.id == local_tab));
                editor.activate_remote_workspace(cx);
                assert!(editor.remote_workspace_active);
                let workspace = editor.remote_workspace.as_ref().unwrap();
                assert_eq!(workspace.tree_scroll.offset().y, px(-80.0));
                assert_eq!(workspace.file_list_scroll.offset().y, px(-100.0));
                assert_eq!(workspace.expanded, expanded);
                assert_eq!(local_scroll.offset().y, px(-40.0));
                assert_eq!(local_list_scroll.offset().y, px(-60.0));
            })
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.focus_sidebar_filter(window, cx))
        });
        cx.simulate_input("child.md");
        let filtered_parent = note_resource
            .parent()
            .unwrap()
            .join_child("folder-copy")
            .unwrap();
        let filtered_child = filtered_parent.join_child("child.md").unwrap();
        wait_for(cx, &editor, "filtered descendant visible", |editor| {
            editor.sidebar_filter_active()
                && editor
                    .remote_visible_resources()
                    .iter()
                    .any(|(path, _)| path == &filtered_child)
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.select_remote_sidebar_item(&filtered_parent, false, false);
                editor.sidebar_nav_left(cx);
                assert!(
                    !editor
                        .remote_visible_resources()
                        .iter()
                        .any(|(path, _)| path == &filtered_child)
                );
                editor.sidebar_nav_right(cx);
                editor.sidebar_nav_right(cx);
                assert_eq!(
                    editor.remote_workspace.as_ref().unwrap().focused.as_ref(),
                    Some(&filtered_child)
                );
                editor.clear_sidebar_filter_text(cx);
            })
        });
        let resource = note_resource;
        cx.run_until_parked();
        let note_bounds = cx.debug_bounds(note_selector).expect("remote note row");
        cx.simulate_click(note_bounds.center(), Default::default());
        wait_for(cx, &editor, "open", |editor| {
            editor.buffer.text().starts_with("# initial needle")
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                let remote_tab = editor.active_tab_id.clone().unwrap();
                editor.switch_to_tab(&local_tab, cx);
                assert!(!editor.remote_workspace_active);
                assert_eq!(editor.buffer.text(), "# local draft");
                editor.switch_to_tab(&remote_tab, cx);
                assert!(editor.remote_workspace_active);
                assert!(editor.buffer.text().starts_with("# initial needle"));
            })
        });
        wait_for(cx, &editor, "thumbnail", |editor| {
            editor.image_cache.iter().any(|(key, image)| {
                key.starts_with("ssh-thumbnail-")
                    && image.as_ref().is_some_and(|image| {
                        image.size(0).width.0 == 256 && image.size(0).height.0 == 256
                    })
            })
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .buffer
                    .replace(0..editor.buffer.len(), "# saved needle\n");
                let index = editor.active_tab_index().unwrap();
                editor.tabs[index].is_dirty = true;
                editor.save_remote_active(false, window, cx);
            })
        });
        wait_for(cx, &editor, "save", |editor| {
            editor.remote_saves_inflight.is_empty()
                && editor
                    .active_tab_index()
                    .is_some_and(|index| !editor.tabs[index].is_dirty)
        });
        assert_eq!(fs::read_to_string(&note).unwrap(), "# saved needle\n");
        let root = resource.parent().unwrap();
        let copied_note = root.join_child("note-1.md").unwrap();
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_resource_clipboard(vec![resource.clone()], false);
                editor.paste_resource_clipboard(root.clone(), cx);
            })
        });
        wait_for(cx, &editor, "copy file", |editor| {
            editor.remote_workspace.as_ref().is_some_and(|workspace| {
                workspace.directories.get(&root).is_some_and(|entries| {
                    entries.iter().any(|entry| entry.resource == copied_note)
                })
            })
        });
        assert_eq!(
            fs::read_to_string(workspace.path().join("note-1.md")).unwrap(),
            "# saved needle\n"
        );
        let nested = root.join_child("nested-folder").unwrap();
        let moved_note = nested.join_child("note-1.md").unwrap();
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_resource_clipboard(vec![copied_note.clone()], true);
                editor.paste_resource_clipboard(nested.clone(), cx);
            })
        });
        wait_for(cx, &editor, "cut file", |editor| {
            editor.remote_workspace.as_ref().is_some_and(|workspace| {
                workspace
                    .directories
                    .get(&nested)
                    .is_some_and(|entries| entries.iter().any(|entry| entry.resource == moved_note))
            })
        });
        assert_eq!(
            fs::metadata(workspace.path().join("note-1.md"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            fs::read_to_string(workspace.path().join("nested-folder/note-1.md")).unwrap(),
            "# saved needle\n"
        );
        let copied_directory = root.join_child("folder-copy-1").unwrap();
        let source_directory = root.join_child("folder-copy").unwrap();
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_resource_clipboard(vec![source_directory.clone()], false);
                editor.paste_resource_clipboard(root.clone(), cx);
            })
        });
        wait_for(cx, &editor, "copy directory", |editor| {
            editor.remote_workspace.as_ref().is_some_and(|workspace| {
                workspace.directories.get(&root).is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|entry| entry.resource == copied_directory)
                })
            })
        });
        assert_eq!(
            fs::read_to_string(workspace.path().join("folder-copy-1/child.md")).unwrap(),
            "# copied directory\n"
        );
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.workspace_search_query = "needle".into();
                editor.run_workspace_search(cx);
            })
        });
        wait_for(cx, &editor, "search", |editor| {
            !editor.workspace_search_has_more
                && editor
                    .workspace_search_results
                    .iter()
                    .any(|result| result.path == remote_path_buf(&resource.path))
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_resources_starred(&[resource.clone()], true, cx);
                editor.rename_remote_entry(resource.clone(), "renamed.md".into(), cx);
            })
        });
        let renamed = resource.parent().unwrap().join_child("renamed.md").unwrap();
        wait_for(cx, &editor, "rename", |editor| {
            editor.active_tab_index().is_some_and(|index| {
                editor.tabs[index]
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.resource.as_ref())
                    == Some(&renamed)
            })
        });
        cx.update(|_, cx| {
            assert!(
                editor
                    .read(cx)
                    .app_settings
                    .starred_resources
                    .contains(&renamed)
            )
        });
        let renamed_path = workspace.path().join("renamed.md");
        fs::write(&renamed_path, "# externally changed\n").unwrap();
        wait_for(cx, &editor, "external reload", |editor| {
            editor.buffer.text() == "# externally changed\n"
        });
        let epoch = cx.update(|_, cx| {
            editor
                .read(cx)
                .remote_workspace
                .as_ref()
                .unwrap()
                .resource_epoch
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                editor
                    .buffer
                    .replace(0..editor.buffer.len(), "# unsaved draft\n");
                let index = editor.active_tab_index().unwrap();
                editor.tabs[index].is_dirty = true;
            })
        });
        fs::write(&renamed_path, "# competing writer\n").unwrap();
        wait_for(cx, &editor, "dirty refresh", |editor| {
            editor.remote_workspace.as_ref().is_some_and(|workspace| {
                workspace.resource_epoch != epoch
                    && !workspace.document_refresh_running
                    && workspace.documents_to_refresh.is_empty()
            })
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                assert_eq!(editor.buffer.text(), "# unsaved draft\n");
                editor.save_remote_active(false, window, cx);
            })
        });
        wait_for(cx, &editor, "save conflict", |editor| {
            editor.remote_saves_inflight.is_empty()
        });
        assert_eq!(
            fs::read_to_string(&renamed_path).unwrap(),
            "# competing writer\n"
        );
        let tab_count = cx.update(|_, cx| editor.read(cx).tabs.len());
        // Restore through the session reader, not the interactive host picker.
        let restored = cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                editor.persist_active_tab_state();
                session_store::persist_session(editor);
            });
            cx.new(|cx| {
                let mut restored = EditorView::new_with_mode(cx, true, false, None, false);
                session_store::restore_session(&mut restored, cx);
                restored
            })
        });
        wait_for(cx, &restored, "silent session reconnect", |editor| {
            editor.remote_workspace.is_some()
                && editor
                    .active_tab_index()
                    .is_some_and(|i| !editor.tabs[i].remote_binding_lost)
        });
        cx.update(|_, cx| {
            let view = restored.read(cx);
            assert!(view.dialog.is_none() && view.remote_input.is_none());
            assert_eq!(view.remote_workspace.as_ref().unwrap().root, root);
        });
        assert_eq!(
            fs::read_to_string(&renamed_path).unwrap(),
            "# competing writer\n"
        );
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert_eq!(editor.buffer.text(), "# unsaved draft\n");
                editor
                    .open_remote_file(renamed.parent().unwrap().join_child("late.md").unwrap(), cx);
                editor.disconnect_remote(cx);
            })
        });
        for _ in 0..50 {
            cx.run_until_parked();
            cx.executor().advance_clock(Duration::from_millis(50));
            std::thread::sleep(Duration::from_millis(5));
        }
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.remote_workspace.is_none());
            assert_eq!(
                editor.tabs.len(),
                tab_count,
                "closed connection must reject late opens"
            );
            assert_eq!(editor.buffer.text(), "# unsaved draft\n");
            let tab = &editor.tabs[editor.active_tab_index().unwrap()];
            assert!(tab.is_dirty && tab.remote_binding_lost);
        });
    }

    #[test]
    fn remote_watch_reloads_only_affected_directory_snapshots() {
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([4; 32]),
            path: NativePath::unix_absolute("/work").unwrap(),
        };
        let directories = ["/work", "/work/a", "/work/a/nested", "/work/ab", "/work/b"]
            .map(|path| root.with_path(NativePath::unix_absolute(path).unwrap()));
        let file_change = vec![NativePath::unix_absolute("/work/a/note.md").unwrap()];
        assert_eq!(
            directories
                .iter()
                .map(|directory| remote_watch_affects_directory(directory, &file_change))
                .collect::<Vec<_>>(),
            vec![false, true, false, false, false]
        );
        let directory_change = vec![NativePath::unix_absolute("/work/a").unwrap()];
        assert_eq!(
            directories
                .iter()
                .map(|directory| remote_watch_affects_directory(directory, &directory_change))
                .collect::<Vec<_>>(),
            vec![true, true, true, false, false]
        );
    }

    #[test]
    fn remote_text_serialization_preserves_crlf_without_double_carriage_returns() {
        assert!(remote_uses_crlf("a\r\nb\r\n"));
        assert!(!remote_uses_crlf("a\r\nb\n"));
        assert_eq!(serialize_remote_text("a\r\nb\n", true), b"a\r\nb\r\n");
        assert_eq!(serialize_remote_text("a\nb\n", false), b"a\nb\n");
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn remote_observations_refresh_clean_tabs_but_preserve_drafts_and_reject_stale_replies(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        cx.update(|cx| {
            let editor =
                cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("original"), false));
            editor.update(cx, |editor, cx| {
                let index = editor.active_tab_index().unwrap();
                let id = editor.tabs[index].id.clone();
                let root = ResourcePath {
                    filesystem: FilesystemId::Ssh([9; 32]),
                    path: NativePath::unix_absolute("/workspace").unwrap(),
                };
                let resource = root.join_child("note.md").unwrap();
                let original = Revision {
                    size: 8,
                    sha256: [1; 32],
                };
                let updated = Revision {
                    size: 3,
                    sha256: [2; 32],
                };
                editor.tabs[index].is_dirty = false;
                editor.tabs[index].is_remote = true;
                editor.tabs[index].file_kind = Some(FileKind::Markdown);
                editor.tabs[index].remote = Some(config_store::RemoteTabState {
                    peer_scope: None,
                    destination: "host".into(),
                    config_file: None,
                    root,
                    resource: Some(resource.clone()),
                    revision: Some(original.clone()),
                    save_uncertain: false,
                    pending_resource: None,
                    line_ending_crlf: false,
                });
                editor.remote_documents.insert(
                    id.clone(),
                    RemoteDocument {
                        resource: Some(resource.clone()),
                        revision: Some(original.clone()),
                        save_uncertain: false,
                    },
                );
                let expected = HashMap::from([(id.clone(), Some(original))]);
                editor.apply_remote_document_observation(
                    &resource,
                    &expected,
                    RemoteDocumentObservation::Present {
                        text: Some("新".into()),
                        revision: updated.clone(),
                    },
                    cx,
                );
                assert_eq!(editor.buffer.text(), "新");
                assert_eq!(editor.remote_documents[&id].revision, Some(updated.clone()));
                editor.apply_remote_document_observation(
                    &resource,
                    &expected,
                    RemoteDocumentObservation::Present {
                        text: Some("stale".into()),
                        revision: updated.clone(),
                    },
                    cx,
                );
                assert_eq!(editor.buffer.text(), "新");
                editor.buffer = crate::buffer::Buffer::from_text("draft");
                editor.tabs[index].is_dirty = true;
                let expected = HashMap::from([(id.clone(), Some(updated.clone()))]);
                editor.apply_remote_document_observation(
                    &resource,
                    &expected,
                    RemoteDocumentObservation::Present {
                        text: Some("external".into()),
                        revision: Revision {
                            size: 8,
                            sha256: [3; 32],
                        },
                    },
                    cx,
                );
                assert_eq!(editor.buffer.text(), "draft");
                assert_eq!(editor.remote_documents[&id].revision, Some(updated));
                assert!(editor.tabs[index].is_dirty);
                editor.apply_remote_document_observation(
                    &resource,
                    &expected,
                    RemoteDocumentObservation::Missing,
                    cx,
                );
                assert!(editor.tabs[index].is_deleted);
                assert_eq!(editor.buffer.text(), "draft");
            });
        });
    }

    #[test]
    fn remote_search_filters_apply_to_relative_paths_and_excluded_ancestors() {
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([8; 32]),
            path: NativePath::unix_absolute("/work").unwrap(),
        };
        let mut results = ["src/main.rs", "src/vendor/hidden.rs", "src/readme.md"]
            .into_iter()
            .map(|path| RemoteSearchResult {
                resource: path.split('/').fold(root.clone(), |parent, child| {
                    parent.join_child(child).unwrap()
                }),
                snippet: String::new(),
                line: 0,
                start: 0,
                end: 0,
            })
            .collect::<Vec<_>>();
        let filter =
            super::super::workspace_search::SearchPathFilter::new(r"\.rs$", r"^src/vendor$")
                .unwrap();
        filter_remote_search_results(&root, &filter, &mut results);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].resource,
            root.join_child("src")
                .unwrap()
                .join_child("main.rs")
                .unwrap()
        );
        let invalid_name = NativePath::Unix(b"bad-\xff.rs".to_vec());
        let non_utf8 = root.join_native_child(&invalid_name).unwrap();
        let mut results = vec![RemoteSearchResult {
            resource: non_utf8.clone(),
            snippet: String::new(),
            line: 0,
            start: 0,
            end: 0,
        }];
        let filter = super::super::workspace_search::SearchPathFilter::new("bad-�", "").unwrap();
        filter_remote_search_results(&root, &filter, &mut results);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].resource, non_utf8);
    }

    #[test]
    fn save_attempt_ids_and_pending_destination_are_distinct() {
        assert_ne!(next_remote_save_id(), next_remote_save_id());
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([8; 32]),
            path: NativePath::unix_absolute("/work").unwrap(),
        };
        let original = root.join_child("original.md").unwrap();
        let target = root.join_child("copy.md").unwrap();
        let state = config_store::RemoteTabState {
            peer_scope: None,
            destination: "host".into(),
            config_file: Some(PathBuf::from("ssh-config")),
            root,
            resource: Some(original.clone()),
            revision: None,
            save_uncertain: false,
            pending_resource: None,
            line_ending_crlf: false,
        };
        let pending = pending_remote_save(&state, &target);
        assert_eq!(pending.resource, Some(original));
        assert_eq!(pending.pending_resource, Some(target));
        assert!(pending.save_uncertain);
        assert!(!state.save_uncertain);
    }

    #[test]
    fn remote_text_is_strict_and_rebase_preserves_unix_bytes() {
        assert!(decode_remote_text(vec![0xff]).is_err());
        assert_eq!(decode_remote_text("文".as_bytes().to_vec()).unwrap(), "文");
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([5; 32]),
            path: NativePath::unix_absolute("/work").unwrap(),
        };
        let source = root.join_child("old").unwrap();
        let target = root.join_child("new").unwrap();
        let old = source
            .join_native_child(&NativePath::Unix(vec![0xff]))
            .unwrap();
        assert_eq!(
            rebase_resource(&old, &source, &target),
            Some(
                target
                    .join_native_child(&NativePath::Unix(vec![0xff]))
                    .unwrap()
            )
        );
        assert_eq!(
            remote_relative_resource(&source.join_child("doc.md").unwrap(), "../img%20a.png")
                .unwrap(),
            root.join_child("img a.png").unwrap()
        );
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn cancelling_input_preserves_connection_and_watch_epochs(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext;
        cx.update(|cx| {
            let editor = cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false));
            editor.update(cx, |editor, cx| {
                editor.remote_generation = 4;
                editor.remote_watch_generation = 7;
                editor.remote_search_generation = 9;
                editor.cancel_remote_input(cx);
                assert_eq!(editor.remote_generation, 4);
                assert_eq!(editor.remote_watch_generation, 7);
                assert_eq!(editor.remote_search_generation, 9);
            });
        });
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn remote_image_replacement_keeps_markdown_and_uses_encoded_remote_url(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        cx.update(|cx| {
            let editor = cx.new(|cx| {
                EditorView::new_with_mode(cx, true, false, Some("![image](old.png)\nnext"), false)
            });
            editor.update(cx, |editor, cx| {
                editor.replace_image_url_with_text(0, "../assets/new%20image.png", cx);
                assert_eq!(
                    editor.buffer.text(),
                    "![image](../assets/new%20image.png)\nnext"
                );
            });
        });
    }

    #[test]
    fn restored_remote_binding_requires_same_authority_and_root() {
        let filesystem = FilesystemId::Ssh([3; 32]);
        let root = ResourcePath {
            filesystem: filesystem.clone(),
            path: NativePath::unix_absolute("/workspace").unwrap(),
        };
        let state = config_store::RemoteTabState {
            peer_scope: None,
            destination: "dev-host".to_string(),
            config_file: None,
            root: root.clone(),
            resource: Some(root.join_child("note.md").unwrap()),
            revision: None,
            save_uncertain: false,
            pending_resource: None,
            line_ending_crlf: false,
        };

        assert!(remote_state_matches_workspace(&state, "dev-host", &root));
        assert!(!remote_state_matches_workspace(&state, "other-host", &root));
        assert!(!remote_state_matches_workspace(
            &state,
            "dev-host",
            &ResourcePath {
                filesystem: filesystem.clone(),
                path: NativePath::unix_absolute("/other").unwrap(),
            }
        ));
        assert!(!remote_state_matches_workspace(
            &state,
            "dev-host",
            &ResourcePath {
                filesystem: FilesystemId::Ssh([4; 32]),
                path: root.path.clone(),
            }
        ));
    }

    #[cfg(all(unix, feature = "gpui-test-support"))]
    #[gpui::test]
    #[ignore = "freezes a real helper process and rewrites HOME/PATH for the whole test binary"]
    fn frozen_ssh_server_is_reconnected_automatically(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        use std::os::unix::fs::PermissionsExt;
        let sandbox = tempfile::tempdir().unwrap();
        let bin = sandbox.path().join("bin");
        let home = sandbox.path().join("home");
        let work = sandbox.path().join("work");
        for dir in [&bin, &home, &work] {
            moeka_fs::local::native::create_dir(dir).unwrap();
        }
        // A stand-in `ssh` that runs the remote command locally.
        let fake = bin.join("ssh");
        moeka_fs::local::native::write(
            &fake,
            "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift; shift\nexec /bin/sh -c \"$*\"\n",
        )
        .unwrap();
        moeka_fs::local::native::set_permissions(
            &fake,
            moeka_fs::local::native::Permissions::from_mode(0o755),
        )
        .unwrap();
        // SAFETY: single-threaded test setup before any connection thread starts.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            );
        }
        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        let (_, cx) = cx.add_window_view(|_, cx| {
            struct Host(gpui::Entity<EditorView>, gpui::Subscription);
            impl gpui::Render for Host {
                fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                    div()
                }
            }
            Host(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let wait =
            |cx: &mut gpui::VisualTestContext, what: &str, ok: &dyn Fn(&EditorView) -> bool| {
                let deadline = std::time::Instant::now() + Duration::from_secs(150);
                loop {
                    cx.run_until_parked();
                    if cx.update(|_, cx| ok(editor.read(cx))) {
                        return;
                    }
                    assert!(std::time::Instant::now() < deadline, "timed out: {what}");
                    cx.executor().advance_clock(Duration::from_millis(500));
                    std::thread::sleep(Duration::from_millis(20));
                }
            };
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_remote_input(RemoteInputMode::Connect, String::new(), window, cx);
                editor.start_remote_connection(
                    "fake".into(),
                    NativePath::from_local(&work),
                    None,
                    None,
                    true,
                    cx,
                );
            })
        });
        wait(cx, "initial connect", &|editor| {
            editor.remote_workspace.is_some()
        });
        let first = cx.update(|_, cx| {
            editor
                .read(cx)
                .remote_workspace
                .as_ref()
                .unwrap()
                .filesystem
                .clone()
        });
        let pids = String::from_utf8(
            std::process::Command::new("pgrep")
                .args(["-f", "moeka-remote-server"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(!pids.trim().is_empty(), "no server process found");
        for pid in pids.split_whitespace() {
            // SAFETY: plain signal to the helper processes this test spawned.
            unsafe { libc::kill(pid.parse().unwrap(), libc::SIGSTOP) };
        }
        wait(cx, "replacement connection", &|editor| {
            editor.remote_workspace.as_ref().is_some_and(|workspace| {
                !workspace.reconnecting
                    && !workspace.filesystem.is_closed()
                    && !std::sync::Arc::ptr_eq(&workspace.filesystem, &first)
            })
        });
    }
}
