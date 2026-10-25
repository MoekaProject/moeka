use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

pub(super) fn refresh_clean_snapshot(
    snapshot: &mut super::EditorSnapshot,
    is_dirty: bool,
    new_text: String,
) -> bool {
    if is_dirty || snapshot.text == new_text {
        return false;
    }
    let clamp = |offset: usize| new_text.floor_char_boundary(offset.min(new_text.len()));
    snapshot.cursor_offset = clamp(snapshot.cursor_offset);
    snapshot.cursor_anchor = clamp(snapshot.cursor_anchor);
    for (offset, anchor) in &mut snapshot.extra_cursors {
        *offset = clamp(*offset);
        *anchor = clamp(*anchor);
    }
    snapshot.text = new_text;
    true
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct WatchRequest {
    pub root: PathBuf,
    pub directories: HashSet<PathBuf>,
    pub file_list_dir: Option<PathBuf>,
    pub documents: HashSet<PathBuf>,
}

#[derive(Debug)]
pub(super) enum FileObservation {
    File(Vec<u8>),
    Directory,
    Missing,
    Unavailable(String),
}

impl FileObservation {
    pub fn exists(&self) -> Option<bool> {
        match self {
            Self::File(_) | Self::Directory => Some(true),
            Self::Missing => Some(false),
            Self::Unavailable(_) => None,
        }
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::File(bytes) => Some(bytes),
            _ => None,
        }
    }
}

fn observation_error(error: io::Error) -> FileObservation {
    if error.kind() == io::ErrorKind::NotFound {
        FileObservation::Missing
    } else {
        FileObservation::Unavailable(error.to_string())
    }
}

pub(super) fn observe_paths(
    paths: impl IntoIterator<Item = PathBuf>,
) -> io::Result<HashMap<PathBuf, FileObservation>> {
    let mut observations = HashMap::new();
    for path in paths {
        crate::platform::fs_task::check_deadline()?;
        if observations.contains_key(&path) {
            continue;
        }
        let observation = match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => FileObservation::Directory,
            Ok(metadata) if metadata.is_file() => {
                crate::platform::fs_task::check_deadline()?;
                match std::fs::read(&path) {
                    Ok(bytes) => FileObservation::File(bytes),
                    Err(error) => observation_error(error),
                }
            }
            Ok(_) => FileObservation::Unavailable("not a regular file".into()),
            Err(error) => observation_error(error),
        };
        observations.insert(path, observation);
    }
    Ok(observations)
}

fn recent_errors() -> &'static Mutex<HashMap<(&'static str, PathBuf), std::time::Instant>> {
    static ERRORS: OnceLock<Mutex<HashMap<(&'static str, PathBuf), std::time::Instant>>> =
        OnceLock::new();
    ERRORS.get_or_init(Default::default)
}

pub(super) fn operation_suppressed(operation: &'static str, path: &Path) -> bool {
    recent_errors()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(operation, path.to_path_buf()))
        .is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(30))
}

pub(super) fn error_notification_due(operation: &'static str, path: &Path) -> bool {
    use std::time::{Duration, Instant};
    let now = Instant::now();
    let mut errors = recent_errors()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    errors.retain(|_, last| now.duration_since(*last) < Duration::from_secs(30));
    let key = (operation, path.to_path_buf());
    if errors.contains_key(&key) {
        return false;
    }
    if errors.len() >= 512
        && let Some(oldest) = errors
            .iter()
            .min_by_key(|(_, last)| **last)
            .map(|(key, _)| key.clone())
    {
        errors.remove(&oldest);
    }
    errors.insert(key, now);
    true
}

// Native state only leaves this persistent registry inside filesystem workers.
// The registry lock protects metadata, never native creation, polling or drop.
// Each service has its own try-locked slot so a stuck mount cannot lock out
// unrelated windows. Retiring slots count towards the same bounded capacity.
const MAX_WATCH_SERVICES: usize = 8;
type WatchState = Arc<Mutex<Option<FileWatch>>>;

struct WatchEntry {
    owner: Weak<()>,
    root: PathBuf,
    state: WatchState,
    retiring: Arc<AtomicBool>,
}

fn watch_registry() -> &'static Mutex<Vec<WatchEntry>> {
    static WATCHERS: OnceLock<Mutex<Vec<WatchEntry>>> = OnceLock::new();
    WATCHERS.get_or_init(Default::default)
}

fn watch_state(
    registry: &Mutex<Vec<WatchEntry>>,
    owner: &Arc<()>,
    root: &Path,
) -> io::Result<WatchState> {
    let mut reclaimed_native = Vec::new();
    let result = {
        let mut entries = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let owner = Arc::downgrade(owner);
        if let Some(entry) = entries.iter_mut().find(|entry| entry.owner.ptr_eq(&owner)) {
            entry.root = root.to_path_buf();
            return Ok(entry.state.clone());
        }
        if entries.len() >= MAX_WATCH_SERVICES {
            entries.retain(|entry| {
                if entry.owner.strong_count() != 0 {
                    return true;
                }
                let Ok(mut state) = entry.state.try_lock() else {
                    return true;
                };
                if let Some(native) = state.take() {
                    reclaimed_native.push(native);
                }
                false
            });
        }
        if entries.len() >= MAX_WATCH_SERVICES {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "file watcher capacity (8 active or retiring services) is exhausted",
            ))
        } else {
            let state = Arc::new(Mutex::new(None));
            entries.push(WatchEntry {
                owner,
                root: root.to_path_buf(),
                state: state.clone(),
                retiring: Arc::new(AtomicBool::new(false)),
            });
            Ok(state)
        }
    };
    drop(reclaimed_native);
    result
}

struct WatchRetirement {
    owner: Weak<()>,
    root: PathBuf,
    pending: Arc<AtomicBool>,
}

impl Drop for WatchRetirement {
    fn drop(&mut self) {
        self.pending.store(false, Ordering::Relaxed);
    }
}

fn retire_watch(registry: &Mutex<Vec<WatchEntry>>, owner: &Weak<()>) -> io::Result<()> {
    let state = {
        let entries = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = entries.iter().find(|entry| entry.owner.ptr_eq(owner)) else {
            return Ok(());
        };
        if entry.owner.strong_count() != 0 {
            return Ok(());
        }
        entry.state.clone()
    };
    let native = state
        .try_lock()
        .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "file watcher is still running"))?
        .take();
    // Destruction may itself block. No registry or per-service lock is held.
    drop(native);
    let mut entries = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    entries.retain(|entry| !entry.owner.ptr_eq(owner));
    Ok(())
}

pub(super) fn retire_abandoned(executor: &gpui::BackgroundExecutor) {
    let retirements = {
        let entries = watch_registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries
            .iter()
            .filter_map(|entry| {
                if entry.owner.strong_count() != 0 || entry.retiring.swap(true, Ordering::Relaxed) {
                    return None;
                }
                Some(WatchRetirement {
                    owner: entry.owner.clone(),
                    root: entry.root.clone(),
                    pending: entry.retiring.clone(),
                })
            })
            .collect::<Vec<_>>()
    };
    for retirement in retirements {
        executor
            .spawn(async move {
                let path = retirement.root.clone();
                let _ = crate::platform::fs_task::run("Retire file watcher", path, move || {
                    // Keep the scheduling permit in the worker even after timeout.
                    let retirement = retirement;
                    retire_watch(watch_registry(), &retirement.owner)
                })
                .await;
            })
            .detach();
    }
}
#[derive(Default)]
pub(super) struct WatchService {
    lifetime: Arc<()>,
    rescan_after_error: Arc<AtomicBool>,
}

impl WatchService {
    pub async fn poll(&self, request: WatchRequest) -> io::Result<WatchBatch> {
        let lifetime = self.lifetime.clone();
        let rescan = self.rescan_after_error.clone();
        let outcome =
            crate::platform::fs_task::run("Watch files", request.root.clone(), move || {
                let state = watch_state(watch_registry(), &lifetime, &request.root)?;
                let mut state = state.try_lock().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "a previous operation for this file watcher is still running",
                    )
                })?;
                if state.is_none() {
                    *state = Some(
                        FileWatch::new().map_err(|error| io::Error::other(error.to_string()))?,
                    );
                }
                let watcher = state.as_mut().expect("watcher initialized above");
                if rescan.swap(false, Ordering::Relaxed) {
                    watcher.overflowed.store(true, Ordering::Relaxed);
                }
                watcher.poll(request)
            })
            .await;
        // A timed-out worker may have drained events or advanced fingerprints
        // before its result was discarded. Reconcile every document on retry.
        if outcome.is_err() {
            self.rescan_after_error.store(true, Ordering::Relaxed);
        }
        outcome
    }

    pub fn request_rescan(&self) {
        self.rescan_after_error.store(true, Ordering::Relaxed);
    }
}

pub(super) struct WatchBatch {
    pub events: Vec<Event>,
    pub documents_to_check: Vec<PathBuf>,
    pub contents: HashMap<PathBuf, FileObservation>,
}

const MAX_WATCHED_DIRECTORIES: usize = 64;
const MAX_DIR_SNAPSHOT_ENTRIES: usize = 4096;
const MAX_FILE_LIST_STAMP_ENTRIES: usize = 512;
const POLLS_PER_FALLBACK_SCAN: u8 = 4;
const DOCUMENT_CONTENT_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocStamp {
    File {
        len: u64,
        modified: Option<std::time::SystemTime>,
    },
    Directory,
    Missing,
}

fn probe_doc_stamp(path: &Path) -> io::Result<DocStamp> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(DocStamp::Directory),
        Ok(metadata) if metadata.is_file() => Ok(DocStamp::File {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }),
        Ok(_) => Err(io::Error::other("not a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DocStamp::Missing),
        Err(error) => Err(error),
    }
}

fn stamp_modified_recently(stamp: DocStamp) -> bool {
    let DocStamp::File {
        modified: Some(modified),
        ..
    } = stamp
    else {
        return false;
    };
    std::time::SystemTime::now()
        .duration_since(modified)
        .map_or(true, |age| age < std::time::Duration::from_secs(2))
}

fn read_dir_snapshot(dir: &Path) -> io::Result<Option<HashMap<std::ffi::OsString, bool>>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut snapshot = HashMap::new();
    for entry in entries.take(MAX_DIR_SNAPSHOT_ENTRIES) {
        crate::platform::fs_task::check_deadline()?;
        let entry = entry?;
        let is_dir = entry.file_type()?.is_dir();
        let path = entry.path();
        if super::should_skip_path(&path, is_dir)
            || super::file_ops::is_atomic_write_temp_path(&path)
        {
            continue;
        }
        snapshot.insert(entry.file_name(), is_dir);
    }
    Ok(Some(snapshot))
}

fn read_file_list_markdown_stamps(
    dir: &Path,
) -> io::Result<HashMap<PathBuf, (u64, Option<std::time::SystemTime>)>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let mut stamps = HashMap::new();
    for entry in entries.take(MAX_FILE_LIST_STAMP_ENTRIES) {
        crate::platform::fs_task::check_deadline()?;
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        if super::should_skip_path(&path, false)
            || super::file_ops::is_atomic_write_temp_path(&path)
            || !super::is_markdown_file(&path)
        {
            continue;
        }
        if let Ok(metadata) = entry.metadata() {
            stamps.insert(path, (metadata.len(), metadata.modified().ok()));
        }
    }
    Ok(stamps)
}

fn effective_watched_directories(request: &WatchRequest) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |path: PathBuf| {
        if !path.as_os_str().is_empty()
            && seen.len() < MAX_WATCHED_DIRECTORIES
            && seen.insert(path.clone())
        {
            dirs.push(path);
        }
    };
    if !request.root.as_os_str().is_empty() {
        push(request.root.clone());
    }
    if let Some(file_list_dir) = &request.file_list_dir {
        push(file_list_dir.clone());
    }
    for dir in &request.directories {
        push(dir.clone());
    }
    for doc in &request.documents {
        if let Some(parent) = doc.parent() {
            push(parent.to_path_buf());
        }
    }
    dirs
}

pub(super) struct FileWatch {
    watcher: RecommendedWatcher,
    events: Receiver<notify::Result<Event>>,
    watched: HashMap<PathBuf, RecursiveMode>,
    poll_only_targets: HashMap<PathBuf, RecursiveMode>,
    documents: HashSet<PathBuf>,
    fingerprints: HashMap<PathBuf, Option<u64>>,
    doc_stamps: HashMap<PathBuf, DocStamp>,
    content_checked_at: HashMap<PathBuf, std::time::Instant>,
    dir_snapshots: HashMap<PathBuf, HashMap<std::ffi::OsString, bool>>,
    file_list_dir: Option<PathBuf>,
    file_list_stamps: HashMap<PathBuf, (u64, Option<std::time::SystemTime>)>,
    polls_since_scan: u8,
    overflowed: Arc<AtomicBool>,
    request: Option<WatchRequest>,
    resolved_documents: Vec<(PathBuf, PathBuf)>,
}

fn file_fingerprint(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn resolved_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        path.parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name))
            .unwrap_or_else(|| path.to_path_buf())
    })
}

fn watch_targets(request: &WatchRequest) -> HashMap<PathBuf, RecursiveMode> {
    let mut targets = HashMap::new();
    let root = (!request.root.as_os_str().is_empty()).then(|| resolved_path(&request.root));
    if let Some(root) = &root {
        targets.insert(root.clone(), RecursiveMode::Recursive);
    }
    for document in &request.documents {
        for path in [document.clone(), resolved_path(document)] {
            if let Some(parent) = path.parent().filter(|parent| parent.is_dir()) {
                let parent = resolved_path(parent);
                if root.as_ref().is_none_or(|root| !parent.starts_with(root)) {
                    targets.entry(parent).or_insert(RecursiveMode::NonRecursive);
                }
            }
        }
    }
    targets
}

impl FileWatch {
    pub fn new() -> notify::Result<Self> {
        let (sender, events) = mpsc::sync_channel(256);
        let overflowed = Arc::new(AtomicBool::new(false));
        let overflow = overflowed.clone();
        let watcher = notify::recommended_watcher(move |event| {
            if sender.try_send(event).is_err() {
                overflow.store(true, Ordering::Relaxed);
            }
        })?;
        Ok(Self {
            watcher,
            events,
            watched: HashMap::new(),
            poll_only_targets: HashMap::new(),
            documents: HashSet::new(),
            fingerprints: HashMap::new(),
            doc_stamps: HashMap::new(),
            content_checked_at: HashMap::new(),
            dir_snapshots: HashMap::new(),
            file_list_dir: None,
            file_list_stamps: HashMap::new(),
            polls_since_scan: 0,
            overflowed,
            request: None,
            resolved_documents: Vec::new(),
        })
    }

    fn reconcile_directory_snapshot(
        &mut self,
        dir: &Path,
        documents: &HashSet<PathBuf>,
        existing_event_paths: &HashSet<PathBuf>,
        events: &mut Vec<Event>,
        checks: &mut HashSet<PathBuf>,
    ) -> io::Result<()> {
        let current = read_dir_snapshot(dir)?;
        let Some(previous) = self.dir_snapshots.get(dir).cloned() else {
            if let Some(snapshot) = current {
                self.dir_snapshots.insert(dir.to_path_buf(), snapshot);
            }
            return Ok(());
        };
        let Some(current) = current else {
            self.dir_snapshots.remove(dir);
            let dir_buf = dir.to_path_buf();
            if !existing_event_paths.contains(&dir_buf) {
                events.push(
                    Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Folder))
                        .add_path(dir_buf),
                );
            }
            for doc in documents {
                if doc.starts_with(dir) {
                    checks.insert(doc.clone());
                }
            }
            return Ok(());
        };
        if previous == current {
            return Ok(());
        }

        let mut removed = Vec::new();
        for (name, was_dir) in &previous {
            if current.get(name) != Some(was_dir) {
                removed.push((name.clone(), *was_dir));
            }
        }
        let mut added = Vec::new();
        for (name, is_dir) in &current {
            if previous.get(name) != Some(is_dir) {
                added.push((name.clone(), *is_dir));
            }
        }

        self.dir_snapshots.insert(dir.to_path_buf(), current);

        let removed_dirs = removed
            .iter()
            .filter(|(_, was_dir)| *was_dir)
            .map(|(name, _)| dir.join(name))
            .collect::<Vec<_>>();
        let added_dirs = added
            .iter()
            .filter(|(_, is_dir)| *is_dir)
            .map(|(name, _)| dir.join(name))
            .collect::<Vec<_>>();

        let mut paired_old_dirs = HashSet::new();
        let mut paired_new_dirs = HashSet::new();
        if removed_dirs.len() == 1 && added_dirs.len() == 1 {
            paired_old_dirs.insert(removed_dirs[0].clone());
            paired_new_dirs.insert(added_dirs[0].clone());
            self.emit_directory_rename(&removed_dirs[0], &added_dirs[0], documents, events, checks);
        } else if !removed_dirs.is_empty() && !added_dirs.is_empty() {
            for old_dir in &removed_dirs {
                let Some(new_dir) = added_dirs.iter().find(|candidate| {
                    !paired_new_dirs.contains(*candidate)
                        && documents.iter().any(|doc| {
                            doc.strip_prefix(old_dir)
                                .is_ok_and(|rel| candidate.join(rel).exists())
                        })
                }) else {
                    continue;
                };
                paired_old_dirs.insert(old_dir.clone());
                paired_new_dirs.insert(new_dir.clone());
                self.emit_directory_rename(old_dir, new_dir, documents, events, checks);
            }
        }

        for (name, was_dir) in removed {
            let old_path = dir.join(name);
            if was_dir && paired_old_dirs.contains(&old_path) {
                continue;
            }
            for doc in documents {
                if doc == &old_path || doc.starts_with(&old_path) {
                    checks.insert(doc.clone());
                }
            }
            if !existing_event_paths.contains(&old_path) {
                events.push(
                    Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Any))
                        .add_path(old_path),
                );
            }
        }
        for (name, is_dir) in added {
            let new_path = dir.join(name);
            if is_dir && paired_new_dirs.contains(&new_path) {
                continue;
            }
            if !existing_event_paths.contains(&new_path) {
                events.push(
                    Event::new(notify::EventKind::Create(notify::event::CreateKind::Any))
                        .add_path(new_path),
                );
            }
        }
        Ok(())
    }

    fn emit_directory_rename(
        &mut self,
        old_dir: &Path,
        new_dir: &Path,
        documents: &HashSet<PathBuf>,
        events: &mut Vec<Event>,
        checks: &mut HashSet<PathBuf>,
    ) {
        let snapshots = std::mem::take(&mut self.dir_snapshots);
        for (path, snapshot) in snapshots {
            let relocated = path
                .strip_prefix(old_dir)
                .map(|rel| {
                    if rel.as_os_str().is_empty() {
                        new_dir.to_path_buf()
                    } else {
                        new_dir.join(rel)
                    }
                })
                .unwrap_or(path);
            self.dir_snapshots.insert(relocated, snapshot);
        }
        for doc in documents {
            if let Ok(rel) = doc.strip_prefix(old_dir) {
                checks.insert(new_dir.join(rel));
            }
        }
        events.push(
            Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Name(
                notify::event::RenameMode::Both,
            )))
            .add_path(old_dir.to_path_buf())
            .add_path(new_dir.to_path_buf()),
        );
    }

    pub fn poll(&mut self, request: WatchRequest) -> io::Result<WatchBatch> {
        crate::platform::fs_task::check_deadline()?;
        let watch_configuration_changed = self
            .request
            .as_ref()
            .is_none_or(|prev| prev.root != request.root || prev.documents != request.documents);
        let targets = if watch_configuration_changed {
            watch_targets(&request)
        } else {
            self.watched
                .iter()
                .chain(self.poll_only_targets.iter())
                .map(|(path, mode)| (path.clone(), *mode))
                .collect()
        };
        let mut checks = request
            .documents
            .difference(&self.documents)
            .cloned()
            .collect::<HashSet<_>>();
        self.poll_only_targets
            .retain(|path, mode| targets.get(path) == Some(mode));
        let obsolete = self
            .watched
            .iter()
            .filter(|(path, mode)| targets.get(*path) != Some(*mode))
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        for path in obsolete {
            crate::platform::fs_task::check_deadline()?;
            self.watcher.unwatch(&path).map_err(|error| {
                io::Error::other(format!("Stop watching {}: {error}", path.display()))
            })?;
            self.watched.remove(&path);
        }
        for (path, mode) in targets {
            if self.watched.contains_key(&path) || self.poll_only_targets.contains_key(&path) {
                continue;
            }
            crate::platform::fs_task::check_deadline()?;
            match self.watcher.watch(&path, mode) {
                Ok(()) => {
                    self.watched.insert(path, mode);
                    checks.extend(request.documents.iter().cloned());
                }
                Err(error) => {
                    if path.exists() {
                        // Mounts that do not support native FSEvents/kqueue still
                        // reconcile via periodic directory and metadata polling.
                        self.poll_only_targets.insert(path, mode);
                        checks.extend(request.documents.iter().cloned());
                    } else {
                        return Err(io::Error::other(format!(
                            "Register watch for {}: {error}",
                            path.display()
                        )));
                    }
                }
            }
        }

        if watch_configuration_changed {
            self.resolved_documents.clear();
            for path in &request.documents {
                crate::platform::fs_task::check_deadline()?;
                self.resolved_documents
                    .push((path.clone(), resolved_path(path)));
            }
        }
        let root = self
            .watched
            .iter()
            .chain(self.poll_only_targets.iter())
            .find(|(_, mode)| **mode == RecursiveMode::Recursive)
            .map(|(root, _)| root.clone());
        let mut events = Vec::new();
        let overflowed = self.overflowed.swap(false, Ordering::Relaxed);
        if overflowed {
            checks.extend(request.documents.iter().cloned());
            self.dir_snapshots.clear();
            self.file_list_stamps.clear();
            if !request.root.as_os_str().is_empty() {
                events.push(Event::new(notify::EventKind::Any).add_path(request.root.clone()));
            }
        }
        for result in self.events.try_iter().take(256) {
            crate::platform::fs_task::check_deadline()?;
            match result {
                Ok(mut event) => {
                    if matches!(event.kind, notify::EventKind::Access(_)) {
                        continue;
                    }
                    if event.need_rescan() || event.paths.is_empty() {
                        checks.extend(request.documents.iter().cloned());
                    }
                    for path in &mut event.paths {
                        let resolved = path.clone();
                        for (original, document) in &self.resolved_documents {
                            if original == &resolved
                                || original.starts_with(&resolved)
                                || document == &resolved
                                || document.starts_with(&resolved)
                            {
                                checks.insert(original.clone());
                            }
                        }
                        if let Some(root) = &root
                            && let Ok(relative) = resolved.strip_prefix(root)
                        {
                            *path = request.root.join(relative);
                        }
                    }
                    events.push(event);
                }
                Err(error) => {
                    return Err(io::Error::other(format!("File watcher event: {error}")));
                }
            }
        }

        let watched_dirs = effective_watched_directories(&request);
        let watched_dirs_set = watched_dirs.iter().cloned().collect::<HashSet<_>>();
        self.dir_snapshots
            .retain(|dir, _| watched_dirs_set.contains(dir));
        for dir in &watched_dirs {
            if !self.dir_snapshots.contains_key(dir) {
                if let Some(snapshot) = read_dir_snapshot(dir)? {
                    self.dir_snapshots.insert(dir.clone(), snapshot);
                }
            }
        }
        if self.file_list_dir != request.file_list_dir {
            self.file_list_dir = request.file_list_dir.clone();
            self.file_list_stamps.clear();
            if let Some(dir) = &self.file_list_dir {
                self.file_list_stamps = read_file_list_markdown_stamps(dir)?;
            }
        }

        if !events.is_empty() {
            let existing_event_paths = events
                .iter()
                .flat_map(|event| event.paths.iter().cloned())
                .collect::<HashSet<_>>();
            let touched_dirs = existing_event_paths
                .iter()
                .flat_map(|path| [Some(path.as_path()), path.parent()])
                .flatten()
                .filter(|dir| self.dir_snapshots.contains_key(*dir))
                .map(Path::to_path_buf)
                .collect::<HashSet<_>>();
            for dir in touched_dirs {
                self.reconcile_directory_snapshot(
                    &dir,
                    &request.documents,
                    &existing_event_paths,
                    &mut events,
                    &mut checks,
                )?;
            }
        }

        let mut preobserved = HashMap::new();
        self.polls_since_scan += 1;
        if self.polls_since_scan >= POLLS_PER_FALLBACK_SCAN {
            self.polls_since_scan = 0;
            let existing_event_paths = events
                .iter()
                .flat_map(|event| event.paths.iter().cloned())
                .collect::<HashSet<_>>();
            for dir in &watched_dirs {
                self.reconcile_directory_snapshot(
                    dir,
                    &request.documents,
                    &existing_event_paths,
                    &mut events,
                    &mut checks,
                )?;
            }
            if let Some(file_list_dir) = &self.file_list_dir {
                let current_stamps = read_file_list_markdown_stamps(file_list_dir)?;
                for (path, stamp) in &current_stamps {
                    if self
                        .file_list_stamps
                        .get(path)
                        .is_some_and(|prev| prev != stamp)
                        && !existing_event_paths.contains(path)
                    {
                        events.push(
                            Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
                                notify::event::DataChange::Any,
                            )))
                            .add_path(path.clone()),
                        );
                    }
                }
                self.file_list_stamps = current_stamps;
            }

            self.fingerprints
                .retain(|path, _| request.documents.contains(path));
            self.doc_stamps
                .retain(|path, _| request.documents.contains(path));
            self.content_checked_at
                .retain(|path, _| request.documents.contains(path));
            // Remote mounts may preserve/cache size and mtime and emit no
            // native event. Revalidate one oldest document per scan, not the tree.
            let content_probe = request
                .documents
                .iter()
                .filter_map(|path| {
                    if !matches!(self.doc_stamps.get(path), Some(DocStamp::File { .. })) {
                        return None;
                    }
                    let checked = self.content_checked_at.get(path)?;
                    (checked.elapsed() >= DOCUMENT_CONTENT_CHECK_INTERVAL)
                        .then_some((path, checked))
                })
                .min_by_key(|(_, checked)| **checked)
                .map(|(path, _)| path.clone());
            for path in &request.documents {
                crate::platform::fs_task::check_deadline()?;
                let stamp = match probe_doc_stamp(path) {
                    Ok(stamp) => stamp,
                    Err(error) => {
                        return Err(io::Error::other(format!(
                            "Check {}: {error}",
                            path.display()
                        )));
                    }
                };
                let prev_stamp = self.doc_stamps.insert(path.clone(), stamp);
                if stamp == DocStamp::Directory {
                    continue;
                }
                let needs_content_check = prev_stamp != Some(stamp)
                    || !self.fingerprints.contains_key(path)
                    || stamp_modified_recently(stamp)
                    || content_probe.as_ref() == Some(path);
                if !needs_content_check {
                    continue;
                }
                let observation = observe_paths([path.clone()])?
                    .remove(path)
                    .unwrap_or(FileObservation::Missing);
                let fingerprint = match &observation {
                    FileObservation::File(bytes) => Some(file_fingerprint(bytes)),
                    FileObservation::Missing => None,
                    FileObservation::Unavailable(error) => {
                        return Err(io::Error::other(format!(
                            "Check {}: {error}",
                            path.display()
                        )));
                    }
                    FileObservation::Directory => continue,
                };
                let prev_fingerprint = self.fingerprints.insert(path.clone(), fingerprint);
                if prev_fingerprint.is_some_and(|previous| previous != fingerprint)
                    || (prev_stamp.is_some() && prev_stamp != Some(stamp))
                {
                    checks.insert(path.clone());
                }
                preobserved.insert(path.clone(), observation);
            }
        }
        self.documents = request.documents.clone();
        let tracked_documents = request.documents;
        self.request = Some(WatchRequest {
            documents: tracked_documents.clone(),
            ..request
        });
        let mut paths = checks.iter().cloned().collect::<HashSet<_>>();
        let open_doc_parents = tracked_documents
            .iter()
            .filter_map(|doc| doc.parent())
            .collect::<HashSet<_>>();
        // Only rename/create candidates need content matching; never read every
        // .git/build/access event simply because it was delivered by notify.
        for event in &events {
            if matches!(
                event.kind,
                notify::EventKind::Create(_)
                    | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
            ) {
                paths.extend(
                    event
                        .paths
                        .iter()
                        .filter(|path| {
                            super::is_markdown_file(path)
                                || path
                                    .parent()
                                    .is_some_and(|parent| open_doc_parents.contains(parent))
                        })
                        .cloned(),
                );
            }
        }
        paths.retain(|path| !preobserved.contains_key(path));
        let mut contents = observe_paths(paths)?;
        for (path, observation) in preobserved {
            contents.insert(path, observation);
        }
        for (path, observation) in &contents {
            if tracked_documents.contains(path) {
                match observation {
                    FileObservation::File(bytes) => {
                        self.content_checked_at
                            .insert(path.clone(), std::time::Instant::now());
                        self.fingerprints
                            .insert(path.clone(), Some(file_fingerprint(bytes)));
                        if let Ok(stamp) = probe_doc_stamp(path) {
                            self.doc_stamps.insert(path.clone(), stamp);
                        }
                    }
                    FileObservation::Missing => {
                        self.fingerprints.insert(path.clone(), None);
                        self.doc_stamps.insert(path.clone(), DocStamp::Missing);
                    }
                    FileObservation::Directory | FileObservation::Unavailable(_) => {}
                }
            }
        }
        Ok(WatchBatch {
            events,
            documents_to_check: checks.into_iter().collect(),
            contents,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn request(root: &Path, paths: &[PathBuf]) -> WatchRequest {
        WatchRequest {
            root: root.to_path_buf(),
            directories: HashSet::new(),
            file_list_dir: None,
            documents: paths.iter().cloned().collect(),
        }
    }

    #[test]
    fn errors_suppress_repeated_notifications_and_automatic_submissions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("unavailable.md");
        assert!(!operation_suppressed("test observation", &path));
        assert!(error_notification_due("test observation", &path));
        assert!(operation_suppressed("test observation", &path));
        assert!(!error_notification_due("test observation", &path));
        assert!(!operation_suppressed("different observation", &path));
    }

    #[test]
    fn service_slots_are_independent_and_bounded_for_multiple_windows() {
        let registry = Mutex::new(Vec::new());
        let owners = (0..MAX_WATCH_SERVICES)
            .map(|_| Arc::new(()))
            .collect::<Vec<_>>();
        let first = watch_state(&registry, &owners[0], Path::new("/unavailable")).unwrap();
        let held = first.try_lock().unwrap();
        for owner in &owners[1..] {
            let local = watch_state(&registry, owner, Path::new("/local")).unwrap();
            assert!(local.try_lock().is_ok());
        }
        assert!(watch_state(&registry, &owners[0], Path::new("/unavailable")).is_ok());
        assert!(first.try_lock().is_err());
        assert_eq!(
            watch_state(&registry, &Arc::new(()), Path::new("/overflow"))
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock,
        );
        drop(held);
    }

    #[test]
    fn retirement_reclaims_only_abandoned_service_slots() {
        let registry = Mutex::new(Vec::new());
        let active = Arc::new(());
        let abandoned = Arc::new(());
        watch_state(&registry, &active, Path::new("/active")).unwrap();
        watch_state(&registry, &abandoned, Path::new("/abandoned")).unwrap();
        let owner = Arc::downgrade(&abandoned);
        retire_watch(&registry, &Arc::downgrade(&active)).unwrap();
        assert_eq!(registry.try_lock().unwrap().len(), 2);
        drop(abandoned);
        retire_watch(&registry, &owner).unwrap();
        let entries = registry.try_lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].owner.ptr_eq(&Arc::downgrade(&active)));
    }

    #[test]
    fn busy_retired_service_does_not_hold_the_registry() {
        let registry = Mutex::new(Vec::new());
        let owner = Arc::new(());
        let state = watch_state(&registry, &owner, Path::new("/unavailable")).unwrap();
        let retired = Arc::downgrade(&owner);
        let held = state.try_lock().unwrap();
        drop(owner);
        assert_eq!(
            retire_watch(&registry, &retired).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let local = watch_state(&registry, &Arc::new(()), Path::new("/local")).unwrap();
        assert!(local.try_lock().is_ok());
        drop(held);
        retire_watch(&registry, &retired).unwrap();
    }

    #[test]
    fn watcher_service_does_not_own_thread_affine_native_state() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WatchService>();
    }

    #[test]
    fn unavailable_observations_are_not_deletions() {
        for kind in [
            io::ErrorKind::TimedOut,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
        ] {
            let observation = observation_error(io::Error::new(kind, "filesystem unavailable"));
            assert_eq!(observation.exists(), None);
            assert!(observation.bytes().is_none());
        }
        assert_eq!(
            observation_error(io::Error::from(io::ErrorKind::NotFound)).exists(),
            Some(false)
        );
    }

    #[test]
    fn observations_distinguish_files_directories_and_missing_paths() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        let missing = directory.path().join("missing.md");
        std::fs::write(&path, "contents").unwrap();
        let observations = observe_paths([
            path.clone(),
            directory.path().to_path_buf(),
            missing.clone(),
        ])
        .unwrap();
        assert_eq!(observations[&path].bytes(), Some(b"contents".as_slice()));
        assert!(matches!(
            observations[directory.path()],
            FileObservation::Directory
        ));
        assert!(matches!(observations[&missing], FileObservation::Missing));
    }

    #[test]
    fn watcher_events_have_a_bounded_drain_budget() {
        let directory = tempfile::tempdir().unwrap();
        let mut watcher = FileWatch::new().unwrap();
        watcher.poll(request(directory.path(), &[])).unwrap();
        let (sender, receiver) = mpsc::channel();
        watcher.events = receiver;
        for _ in 0..300 {
            sender
                .send(Ok(Event::new(notify::EventKind::Modify(
                    notify::event::ModifyKind::Any,
                ))))
                .unwrap();
        }
        let batch = watcher.poll(request(directory.path(), &[])).unwrap();
        assert_eq!(batch.events.len(), 256);
        assert_eq!(watcher.events.try_iter().count(), 44);
    }

    #[test]
    fn standalone_tabs_watch_deduplicated_parents_without_a_workspace() {
        let directory = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(directory.path()).unwrap();
        let documents = [parent.join("one.md"), parent.join("two.md")];
        let targets = watch_targets(&request(Path::new(""), &documents));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets.get(&parent), Some(&RecursiveMode::NonRecursive));
    }

    #[test]
    fn workspace_and_external_parents_survive_workspace_switch_and_close() {
        let directory = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(directory.path()).unwrap();
        let first = parent.join("first");
        let second = parent.join("second");
        std::fs::create_dir_all(first.join("nested")).unwrap();
        std::fs::create_dir(&second).unwrap();
        let documents = [first.join("nested/doc.md"), second.join("doc.md")];
        let mut watcher = FileWatch::new().unwrap();
        for root in [&first, &second, &PathBuf::new()] {
            watcher.poll(request(root, &documents)).unwrap();
            assert_eq!(watcher.watched, watch_targets(&request(root, &documents)));
            assert_eq!(watcher.watched.len(), 2);
        }
        watcher.poll(request(Path::new(""), &[])).unwrap();
        assert!(watcher.watched.is_empty());
    }

    #[test]
    fn opening_another_document_in_an_existing_watch_requests_a_reload() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("one.md");
        let second = directory.path().join("two.md");
        let mut watcher = FileWatch::new().unwrap();
        watcher
            .poll(request(Path::new(""), std::slice::from_ref(&first)))
            .unwrap();
        let batch = watcher
            .poll(request(Path::new(""), &[first, second.clone()]))
            .unwrap();
        assert!(batch.documents_to_check.contains(&second));
    }

    #[test]
    fn polling_detects_remote_style_content_changes_without_events() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        std::fs::write(&path, "old").unwrap();
        let mut watcher = FileWatch::new().unwrap();
        for _ in 0..6 {
            watcher
                .poll(request(Path::new(""), std::slice::from_ref(&path)))
                .unwrap();
        }
        std::fs::write(&path, "new text").unwrap();
        let mut changed = false;
        for _ in 0..6 {
            changed |= watcher
                .poll(request(Path::new(""), std::slice::from_ref(&path)))
                .unwrap()
                .documents_to_check
                .contains(&path);
        }
        assert!(changed);
    }

    #[test]
    fn bounded_content_probes_detect_unchanged_remote_metadata_without_events() {
        let directory = tempfile::tempdir().unwrap();
        let paths: Vec<_> = (0..3)
            .map(|index| directory.path().join(format!("{index}.md")))
            .collect();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(120);
        for path in &paths {
            std::fs::write(path, "old").unwrap();
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(old_time)
                .unwrap();
        }
        let mut watcher = FileWatch::new().unwrap();
        let request = request(Path::new(""), &paths);
        watcher.poll(request.clone()).unwrap();
        // Remove native delivery, as on a mount that accepts a watch but never
        // forwards remote mutations.
        for path in watcher.watched.keys() {
            watcher.watcher.unwatch(path).unwrap();
        }
        for path in &paths {
            std::fs::write(path, "new").unwrap();
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(old_time)
                .unwrap();
            assert_eq!(probe_doc_stamp(path).unwrap(), watcher.doc_stamps[path]);
            watcher
                .content_checked_at
                .insert(path.clone(), Instant::now() - Duration::from_secs(60));
        }
        while watcher.events.try_recv().is_ok() {}
        let mut detected = HashSet::new();
        for _ in 0..paths.len() {
            watcher.polls_since_scan = POLLS_PER_FALLBACK_SCAN - 1;
            let batch = watcher.poll(request.clone()).unwrap();
            assert_eq!(batch.contents.len(), 1);
            assert_eq!(batch.documents_to_check.len(), 1);
            detected.extend(batch.documents_to_check);
        }
        assert_eq!(detected, paths.into_iter().collect());
        watcher.polls_since_scan = POLLS_PER_FALLBACK_SCAN - 1;
        let idle = watcher.poll(request).unwrap();
        assert!(idle.contents.is_empty());
        assert!(idle.documents_to_check.is_empty());
    }

    #[test]
    fn clean_tabs_reload_but_dirty_tabs_and_unchanged_snapshots_are_preserved() {
        let mut snapshot = super::super::EditorSnapshot {
            text: "local draft".into(),
            cursor_offset: 11,
            cursor_anchor: 7,
            scroll_offset: 80.0,
            ..Default::default()
        };
        let before = snapshot.clone();
        assert!(!refresh_clean_snapshot(&mut snapshot, true, "disk".into()));
        assert_eq!(snapshot, before);
        assert!(!refresh_clean_snapshot(
            &mut snapshot,
            false,
            "local draft".into()
        ));
        assert_eq!(snapshot, before);
        assert!(refresh_clean_snapshot(&mut snapshot, false, "disk".into()));
        assert_eq!(snapshot.text, "disk");
        assert_eq!(snapshot.cursor_offset, 4);
        assert_eq!(snapshot.cursor_anchor, 4);
        assert_eq!(snapshot.scroll_offset, 80.0);
    }

    #[test]
    fn external_reload_clamps_all_cursor_offsets_to_utf8_boundaries() {
        let mut snapshot = super::super::EditorSnapshot {
            text: "previous ascii document".into(),
            cursor_offset: 2,
            cursor_anchor: 4,
            extra_cursors: vec![(3, usize::MAX)],
            ..Default::default()
        };
        assert!(refresh_clean_snapshot(
            &mut snapshot,
            false,
            "a\u{1f642}b".into()
        ));
        assert_eq!(snapshot.cursor_offset, 1);
        assert_eq!(snapshot.cursor_anchor, 1);
        assert_eq!(snapshot.extra_cursors, vec![(1, snapshot.text.len())]);
        for offset in [snapshot.cursor_offset, snapshot.cursor_anchor]
            .into_iter()
            .chain(
                snapshot
                    .extra_cursors
                    .iter()
                    .flat_map(|&(offset, anchor)| [offset, anchor]),
            )
        {
            assert!(snapshot.text.is_char_boundary(offset));
        }
    }

    #[test]
    fn registration_catches_changes_before_the_first_native_notification() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        std::fs::write(&path, "old").unwrap();
        let mut snapshot = super::super::EditorSnapshot {
            text: "old".into(),
            ..Default::default()
        };
        let mut watcher = FileWatch::new().unwrap();
        std::fs::write(&path, "new before registration").unwrap();
        let batch = watcher
            .poll(request(Path::new(""), std::slice::from_ref(&path)))
            .unwrap();
        for path in batch.documents_to_check {
            assert!(refresh_clean_snapshot(
                &mut snapshot,
                false,
                std::fs::read_to_string(path).unwrap()
            ));
        }
        assert_eq!(snapshot.text, "new before registration");
    }

    #[cfg(unix)]
    #[test]
    fn canonical_events_still_target_tabs_opened_through_a_directory_alias() {
        let directory = tempfile::tempdir().unwrap();
        let real = directory.path().join("real");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let path = alias.join("note.md");
        std::fs::write(&path, "content").unwrap();
        let mut watcher = FileWatch::new().unwrap();
        watcher
            .poll(request(Path::new(""), std::slice::from_ref(&path)))
            .unwrap();
        let (sender, events) = mpsc::channel();
        watcher.events = events;
        sender
            .send(Ok(Event::new(notify::EventKind::Modify(
                notify::event::ModifyKind::Data(notify::event::DataChange::Content),
            ))
            .add_path(std::fs::canonicalize(&path).unwrap())))
            .unwrap();
        let batch = watcher
            .poll(request(Path::new(""), std::slice::from_ref(&path)))
            .unwrap();
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.documents_to_check, vec![path]);
    }

    #[test]
    fn closing_one_tab_retains_its_shared_parent_until_the_last_tab_closes() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("one.md");
        let second = directory.path().join("two.md");
        let mut watcher = FileWatch::new().unwrap();
        watcher
            .poll(request(Path::new(""), &[first.clone(), second]))
            .unwrap();
        watcher.poll(request(Path::new(""), &[first])).unwrap();
        assert_eq!(watcher.watched.len(), 1);
        watcher.poll(request(Path::new(""), &[])).unwrap();
        assert!(watcher.watched.is_empty());
    }

    #[test]
    fn native_parent_watch_reports_writes_atomic_replacement_and_followup_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(directory.path())
            .unwrap()
            .join("note.md");
        std::fs::write(&path, "before").unwrap();
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
        watcher
            .watch(path.parent().unwrap(), RecursiveMode::NonRecursive)
            .unwrap();
        let wait_for_path = || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let event: notify::Result<Event> = receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("file change notification");
                if event
                    .unwrap()
                    .paths
                    .iter()
                    .any(|changed| resolved_path(changed) == path)
                {
                    break;
                }
            }
        };
        std::fs::write(&path, "modified").unwrap();
        wait_for_path();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "modified");
        for _ in receiver.try_iter() {}
        let replacement = path.with_file_name("replacement.tmp");
        std::fs::write(&replacement, "replacement").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        wait_for_path();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        for _ in receiver.try_iter() {}
        std::fs::write(&path, "after replacement").unwrap();
        wait_for_path();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after replacement");
    }

    #[test]
    fn rapid_workspace_switches_reclaim_abandoned_slots_inline() {
        let registry = Mutex::new(Vec::new());
        for index in 0..(MAX_WATCH_SERVICES * 3) {
            let owner = Arc::new(());
            let root = PathBuf::from(format!("/workspace-{index}"));
            let state = watch_state(&registry, &owner, &root).unwrap();
            assert!(state.try_lock().is_ok());
        }
        assert!(registry.lock().unwrap().len() <= MAX_WATCH_SERVICES);
    }

    #[test]
    fn polling_detects_remote_directory_and_file_mutations_without_native_events() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let folder_a = root.join("folder-a");
        std::fs::create_dir(&folder_a).unwrap();
        let open_doc = folder_a.join("open.md");
        std::fs::write(&open_doc, "initial text").unwrap();

        let mut watcher = FileWatch::new().unwrap();
        let (_silent_tx, silent_rx) = mpsc::channel();
        watcher.events = silent_rx;

        let make_request = |doc: &Path, extra_dir: &Path| WatchRequest {
            root: root.to_path_buf(),
            directories: [root.to_path_buf(), extra_dir.to_path_buf()]
                .into_iter()
                .collect(),
            file_list_dir: Some(root.to_path_buf()),
            documents: [doc.to_path_buf()].into_iter().collect(),
        };

        // Initial poll establishes directory and document baselines.
        watcher.poll(make_request(&open_doc, &folder_a)).unwrap();

        // 1. External file creation in root (without native events).
        let created_file = root.join("created.md");
        std::fs::write(&created_file, "# Created").unwrap();
        let mut saw_create = false;
        for _ in 0..POLLS_PER_FALLBACK_SCAN {
            let batch = watcher.poll(make_request(&open_doc, &folder_a)).unwrap();
            saw_create |= batch
                .events
                .iter()
                .any(|event| event.paths.contains(&created_file));
        }
        assert!(saw_create);

        // 2. External directory rename (folder-a -> folder-b) containing an open tab.
        let folder_b = root.join("folder-b");
        std::fs::rename(&folder_a, &folder_b).unwrap();
        let relocated_doc = folder_b.join("open.md");
        let mut saw_dir_rename = false;
        let mut saw_relocated_doc = false;
        for _ in 0..POLLS_PER_FALLBACK_SCAN {
            let batch = watcher.poll(make_request(&open_doc, &folder_a)).unwrap();
            saw_dir_rename |= batch.events.iter().any(|event| {
                matches!(
                    event.kind,
                    notify::EventKind::Modify(notify::event::ModifyKind::Name(
                        notify::event::RenameMode::Both
                    ))
                ) && event.paths == vec![folder_a.clone(), folder_b.clone()]
            });
            saw_relocated_doc |= batch.contents.contains_key(&relocated_doc);
        }
        assert!(saw_dir_rename);
        assert!(saw_relocated_doc);

        // 3. External file deletion in root.
        std::fs::remove_file(&created_file).unwrap();
        let mut saw_remove = false;
        for _ in 0..POLLS_PER_FALLBACK_SCAN {
            let batch = watcher
                .poll(make_request(&relocated_doc, &folder_b))
                .unwrap();
            saw_remove |= batch
                .events
                .iter()
                .any(|event| event.paths.contains(&created_file));
        }
        assert!(saw_remove);
    }
}
