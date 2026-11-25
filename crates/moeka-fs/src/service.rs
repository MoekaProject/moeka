//! Bounded, pull-based directory/search/watch services and the shared wire contract.

use std::collections::{HashMap, VecDeque};
use std::fs::ReadDir;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

use crate::backend::{
    DirectoryEntry, ExpectedRevision, Filesystem, FsError, FsResult, LocalBackend, MAX_BATCH,
    Metadata, ReadChunk, ReadSession, Revision, SaveOutcome, SaveRequest, StagedSave,
};
use crate::path::NativePath;
use crate::protocol::PROTOCOL_VERSION;
use crate::search::{
    CompiledSearchPathFilters, SearchCursor, SearchPathFilters, relative_path_text,
    should_skip_path,
};
use crate::search_snapshot::{SearchSnapshot, SearchSnapshotStep, SearchText, SourceOffsets};

const MAX_SESSIONS: usize = 16;
const MAX_SEARCHES: usize = 4;
const MAX_SAVES: usize = 8;
const MAX_DEPTH: usize = 64;
const MAX_SEARCH_QUERY: usize = 4096;
const SEARCH_PAGE: usize = 16;
const SEARCH_STEPS: usize = 64;
const WATCH_CAPACITY: usize = 128;
const WATCH_PAGE: usize = 16;
const MAX_WATCH_EVENT_BYTES: usize = 32 * 1024;
const WATCH_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub id: u64,
    pub request: Request,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildIdentity {
    pub server_version: String,
    pub app_version: String,
    pub source_sha: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerArtifact {
    pub build: BuildIdentity,
    pub target: String,
    pub revision: Revision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Hello {
        protocol: u32,
        expected_build: Option<BuildIdentity>,
    },
    PeerIdentity,
    HomeDirectory,
    Metadata {
        path: NativePath,
        follow: bool,
    },
    Canonicalize {
        path: NativePath,
    },
    ReadLink {
        path: NativePath,
    },
    CreateSymlink {
        target: NativePath,
        link: NativePath,
        directory: Option<bool>,
    },
    MetadataBatch {
        paths: Vec<NativePath>,
        follow: bool,
    },
    Read {
        path: NativePath,
        offset: u64,
        length: usize,
    },
    ReadPreview {
        path: NativePath,
        length: usize,
    },
    OpenRead {
        path: NativePath,
    },
    NextRead {
        session: u64,
        length: usize,
    },
    OpenDirectory {
        path: NativePath,
    },
    NextDirectory {
        session: u64,
    },
    CreateFile {
        path: NativePath,
    },
    CreateDirectory {
        path: NativePath,
        recursive: bool,
    },
    CopyFile {
        source: NativePath,
        target: NativePath,
    },
    Rename {
        source: NativePath,
        target: NativePath,
    },
    Trash {
        path: NativePath,
    },
    Save {
        save: SaveRequest,
    },
    BeginSave {
        operation_id: String,
        path: NativePath,
        expected: ExpectedRevision,
        total_bytes: u64,
    },
    AppendSave {
        session: u64,
        offset: u64,
        bytes: Vec<u8>,
    },
    CommitSave {
        session: u64,
    },
    SaveOutcome {
        operation_id: String,
    },
    StartSearch {
        root: NativePath,
        options: SearchOptions,
    },
    StartFilteredSearch {
        root: NativePath,
        options: SearchOptions,
        filters: SearchPathFilters,
    },
    NextSearch {
        session: u64,
    },
    StartWatch {
        root: NativePath,
        generation: u64,
        recursive: bool,
    },
    PollWatch {
        session: u64,
        after_sequence: u64,
    },
    Cancel {
        session: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reply {
    pub id: u64,
    pub result: FsResult<Response>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Response {
    Path(NativePath),
    PeerIdentity([u8; 32]),
    Hello {
        protocol: u32,
        build: BuildIdentity,
        capabilities: Vec<String>,
    },
    Metadata(Metadata),
    MetadataBatch(Vec<FsResult<Metadata>>),
    Read(ReadChunk),
    Preview(crate::backend::ReadPreview),
    ReadOpened {
        session: u64,
        revision: Revision,
    },
    Session(u64),
    Directory {
        entries: Vec<FsResult<DirectoryEntry>>,
        done: bool,
    },
    Search {
        items: Vec<SearchItem>,
        done: bool,
    },
    Watch(WatchBatch),
    Copied(u64),
    Save(SaveOutcome),
    Unit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchOptions {
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SearchItem {
    Match {
        path: NativePath,
        revision: Revision,
        start: usize,
        end: usize,
        line: usize,
        snippet: String,
    },
    Error(FsError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchEvent {
    pub sequence: u64,
    pub paths: Vec<NativePath>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchBatch {
    pub generation: u64,
    pub sequence: u64,
    pub invalidated: bool,
    pub error: Option<FsError>,
    pub events: Vec<WatchEvent>,
}

struct WatchQueue {
    sequence: u64,
    invalidated: bool,
    error: Option<FsError>,
    events: VecDeque<WatchEvent>,
}

impl WatchQueue {
    fn push(&mut self, paths: Vec<NativePath>) {
        self.sequence = self.sequence.saturating_add(1);
        let encoded_bound = paths.iter().fold(0_usize, |sum, path| {
            let units = match path {
                NativePath::Unix(units) => units.len(),
                NativePath::Windows(units) => units.len(),
            };
            sum.saturating_add(128)
                .saturating_add(units.saturating_mul(6))
        });
        if paths.is_empty()
            || paths.len() > MAX_BATCH
            || encoded_bound > MAX_WATCH_EVENT_BYTES
            || self.events.len() >= WATCH_CAPACITY
        {
            self.events.clear();
            self.invalidated = true;
        } else {
            self.events.push_back(WatchEvent {
                sequence: self.sequence,
                paths,
            });
        }
    }

    fn invalidate(&mut self) {
        self.sequence = self.sequence.saturating_add(1);
        self.events.clear();
        self.invalidated = true;
    }

    fn poll(&mut self, generation: u64, after: u64) -> WatchBatch {
        let next = self.events.front().map(|event| event.sequence);
        let gap = after > self.sequence
            || next.is_some_and(|seq| seq > after.saturating_add(1))
            || (next.is_none() && after < self.sequence);
        if self.invalidated || gap {
            self.events.clear();
            self.invalidated = false;
            return WatchBatch {
                generation,
                sequence: self.sequence,
                invalidated: true,
                error: self.error.take(),
                events: Vec::new(),
            };
        }
        while self
            .events
            .front()
            .is_some_and(|event| event.sequence <= after)
        {
            self.events.pop_front();
        }
        let events: Vec<_> = self
            .events
            .drain(..self.events.len().min(WATCH_PAGE))
            .collect();
        let sequence = events.last().map_or(after, |event| event.sequence);
        WatchBatch {
            generation,
            sequence,
            invalidated: false,
            error: None,
            events,
        }
    }
}

#[derive(Default)]
struct WatchSubscribers {
    queues: Vec<Weak<Mutex<WatchQueue>>>,
    error: Option<FsError>,
}

impl WatchSubscribers {
    fn publish(&mut self, event: notify::Result<notify::Event>) {
        if matches!(&event, Ok(event) if !event.need_rescan() && matches!(
            event.kind,
            notify::EventKind::Access(_)
                | notify::EventKind::Modify(notify::event::ModifyKind::Metadata(
                    notify::event::MetadataKind::AccessTime
                ))
        )) {
            return;
        }
        let paths = match event {
            Ok(event) if event.need_rescan() || event.paths.len() > MAX_BATCH => None,
            Ok(event) => Some(
                event
                    .paths
                    .iter()
                    .map(|path| NativePath::from_local(path))
                    .collect::<Vec<_>>(),
            ),
            Err(error) => {
                let path = error.paths.first().cloned();
                let cause = match error.kind {
                    notify::ErrorKind::Io(error) => error,
                    kind => io::Error::other(format!("{kind:?}")),
                };
                self.error = Some(FsError::from_io("watch", path.as_deref(), cause));
                None
            }
        };
        self.queues.retain(|subscriber| {
            let Some(queue) = subscriber.upgrade() else {
                return false;
            };
            let mut queue = queue.lock().unwrap_or_else(|error| error.into_inner());
            match &paths {
                Some(paths) => queue.push(paths.clone()),
                None => {
                    queue.error = self.error.clone();
                    queue.invalidate();
                }
            }
            true
        });
    }
}

struct WatchRoot {
    _watcher: Option<notify::RecommendedWatcher>,
    subscribers: Arc<Mutex<WatchSubscribers>>,
}

impl WatchRoot {
    fn new(root: &std::path::Path, recursive: bool) -> Self {
        let subscribers = Arc::new(Mutex::new(WatchSubscribers::default()));
        let events = subscribers.clone();
        let watcher = notify::recommended_watcher(move |event| {
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .publish(event);
        })
        .and_then(|mut watcher| {
            watcher.watch(
                root,
                if recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                },
            )?;
            Ok(watcher)
        });
        let watcher = match watcher {
            Ok(watcher) => Some(watcher),
            Err(error) => {
                let cause = match error.kind {
                    notify::ErrorKind::Io(error) => error,
                    kind => io::Error::other(format!("{kind:?}")),
                };
                subscribers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .error = Some(FsError::from_io("watch", Some(root), cause));
                None
            }
        };
        Self {
            _watcher: watcher,
            subscribers,
        }
    }

    fn subscribe(self: &Arc<Self>, generation: u64) -> WatchSession {
        let mut subscribers = self
            .subscribers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let queue = Arc::new(Mutex::new(WatchQueue {
            sequence: 0,
            invalidated: true,
            error: subscribers.error.clone(),
            events: VecDeque::new(),
        }));
        subscribers.queues.retain(|queue| queue.strong_count() > 0);
        subscribers.queues.push(Arc::downgrade(&queue));
        WatchSession {
            _root: self.clone(),
            queue,
            generation,
            last_reconcile: Instant::now(),
        }
    }
}

struct WatchSession {
    _root: Arc<WatchRoot>,
    queue: Arc<Mutex<WatchQueue>>,
    generation: u64,
    last_reconcile: Instant,
}

impl WatchSession {
    fn poll(&mut self, after: u64, now: Instant) -> WatchBatch {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        if now.saturating_duration_since(self.last_reconcile) >= WATCH_RECONCILE_INTERVAL {
            queue.invalidate();
            self.last_reconcile = now;
        }
        queue.poll(self.generation, after)
    }
}

struct SearchSession {
    cursor: SearchCursor,
    base: PathBuf,
    filters: CompiledSearchPathFilters,
    root: Option<PathBuf>,
    dirs: Vec<ReadDir>,
    pending: Option<PendingMatches>,
    loading: Option<SearchSnapshot>,
}

struct PendingMatches {
    path: NativePath,
    text: SearchText,
    revision: Revision,
    line: usize,
    line_end: usize,
    cursor: SearchCursor,
    offsets: SourceOffsets,
}

impl PendingMatches {
    fn next(&mut self) -> Option<SearchItem> {
        let text = self.text.as_str();
        let range = self.cursor.next_match(text)?;
        while range.start > self.line_end && self.line_end < text.len() {
            self.line += 1;
            let start = self.line_end + 1;
            self.line_end = text[start..]
                .find('\n')
                .map_or(text.len(), |index| start + index);
        }
        let snippet_start = range.start;
        let mut end = self.line_end.min(snippet_start.saturating_add(128));
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Some(SearchItem::Match {
            path: self.path.clone(),
            revision: self.revision.clone(),
            start: self.text.source_offset(range.start, &mut self.offsets),
            end: self.text.source_offset(range.end, &mut self.offsets),
            line: self.line,
            snippet: text[snippet_start..end].to_owned(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PageKind {
    Directory,
    Search,
}

/// One service per connection. State is bounded, and sessions are explicitly cancellable.
pub struct Service {
    backend: Arc<LocalBackend>,
    build: BuildIdentity,
    hello: bool,
    next_id: u64,
    directories: HashMap<u64, ReadDir>,
    reads: HashMap<u64, ReadSession>,
    saves: HashMap<u64, StagedSave>,
    searches: HashMap<u64, SearchSession>,
    watches: HashMap<u64, WatchSession>,
    watch_roots: HashMap<(NativePath, bool), Weak<WatchRoot>>,
    completed_pages: VecDeque<(u64, PageKind)>,
}

impl Service {
    pub fn new(backend: Arc<LocalBackend>, build: BuildIdentity) -> Self {
        Self {
            backend,
            build,
            hello: false,
            next_id: 1,
            directories: HashMap::new(),
            reads: HashMap::new(),
            saves: HashMap::new(),
            searches: HashMap::new(),
            watches: HashMap::new(),
            watch_roots: HashMap::new(),
            completed_pages: VecDeque::new(),
        }
    }

    fn id(&mut self) -> FsResult<u64> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| FsError::invalid("session", "session IDs exhausted"))?;
        Ok(id)
    }

    fn start_search(
        &mut self,
        root: NativePath,
        options: SearchOptions,
        filters: SearchPathFilters,
    ) -> FsResult<Response> {
        // A superseded search the client never released must not lock new ones out;
        // the oldest session ends and reports itself done if it is ever polled.
        while self.searches.len() >= MAX_SEARCHES {
            let Some(oldest) = self.searches.keys().min().copied() else {
                break;
            };
            self.searches.remove(&oldest);
            self.complete_pages(oldest, PageKind::Search);
        }
        if options.query.is_empty() || options.query.len() > MAX_SEARCH_QUERY {
            return Err(FsError::invalid("search", "invalid query length"));
        }
        let cursor = SearchCursor::new(
            &options.query,
            options.case_sensitive,
            options.whole_word,
            options.regex,
        )
        .map_err(|error| FsError::invalid("search_regex", &error.to_string()))?;
        let filters = filters
            .compile()
            .map_err(|error| FsError::invalid("search_path_regex", &error.to_string()))?;
        let root = root
            .to_local()
            .map_err(|error| FsError::from_io("search", None, error))?;
        let id = self.id()?;
        self.searches.insert(
            id,
            SearchSession {
                cursor,
                base: root.clone(),
                filters,
                root: Some(root),
                dirs: Vec::new(),
                pending: None,
                loading: None,
            },
        );
        Ok(Response::Session(id))
    }

    fn complete_pages(&mut self, session: u64, kind: PageKind) {
        if self.completed_pages.len() == MAX_SESSIONS * 2 {
            self.completed_pages.pop_front();
        }
        self.completed_pages.push_back((session, kind));
    }

    pub fn dispatch(&mut self, envelope: Envelope) -> Reply {
        Reply {
            id: envelope.id,
            result: self.execute(envelope.request),
        }
    }

    pub fn execute(&mut self, request: Request) -> FsResult<Response> {
        if let Request::Hello {
            protocol,
            expected_build,
        } = request
        {
            if self.hello {
                return Err(FsError::invalid("hello", "already negotiated"));
            }
            if protocol != PROTOCOL_VERSION
                || expected_build
                    .as_ref()
                    .is_some_and(|build| build != &self.build)
            {
                return Err(FsError::unsupported("hello", "protocol or build mismatch"));
            }
            self.hello = true;
            let mut capabilities: Vec<String> = [
                "metadata",
                "canonicalize",
                "read_link",
                "metadata_batch",
                "read_chunks",
                "read_sessions",
                "directory_pages",
                "page_credit_window",
                "create",
                "copy_no_replace",
                "save_bounded_process_outcomes",
                "save_staged_chunks",
                "save_staged_large_files",
                "search_pages",
                "search_path_filters",
                "native_watch_pull",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            if cfg!(any(unix, windows)) {
                capabilities.push("create_symlink".into());
            }
            if cfg!(any(target_os = "linux", target_os = "macos")) {
                capabilities.push("peer_instance_binding".into());
                capabilities.push("rename_no_replace".into());
                capabilities.push("native_trash".into());
            }
            return Ok(Response::Hello {
                protocol: PROTOCOL_VERSION,
                build: self.build.clone(),
                capabilities,
            });
        }
        if !self.hello {
            return Err(FsError::invalid("handshake", "hello required"));
        }
        match request {
            Request::Hello { .. } => unreachable!(),
            Request::HomeDirectory => {
                let home = std::env::var_os("HOME")
                    .ok_or_else(|| FsError::invalid("home_directory", "HOME is not set"))?;
                self.backend
                    .canonicalize(&NativePath::from_local(std::path::Path::new(&home)))
                    .map(Response::Path)
            }
            Request::PeerIdentity => crate::peer::identity()
                .map(Response::PeerIdentity)
                .map_err(|error| FsError::from_io("peer_identity", None, error)),
            Request::Canonicalize { path } => self.backend.canonicalize(&path).map(Response::Path),
            Request::ReadLink { path } => self.backend.read_link(&path).map(Response::Path),
            Request::CreateSymlink {
                target,
                link,
                directory,
            } => self
                .backend
                .create_symlink(&target, &link, directory)
                .map(|()| Response::Unit),
            Request::Metadata { path, follow } => {
                self.backend.metadata(&path, follow).map(Response::Metadata)
            }
            Request::MetadataBatch { paths, follow } => self
                .backend
                .metadata_batch(&paths, follow)
                .map(Response::MetadataBatch),
            Request::Read {
                path,
                offset,
                length,
            } => self
                .backend
                .read_chunk(&path, offset, length)
                .map(Response::Read),
            Request::ReadPreview { path, length } => self
                .backend
                .read_preview(&path, length)
                .map(Response::Preview),
            Request::OpenRead { path } => {
                if self.reads.len() >= MAX_SESSIONS {
                    return Err(FsError::invalid("open_read", "read session limit reached"));
                }
                let read = self.backend.open_read(&path)?;
                let revision = read.revision().clone();
                let session = self.id()?;
                self.reads.insert(session, read);
                Ok(Response::ReadOpened { session, revision })
            }
            Request::NextRead { session, length } => {
                let read = self
                    .reads
                    .get_mut(&session)
                    .ok_or_else(|| FsError::invalid("next_read", "unknown session"))?;
                let result = read.next_chunk(length);
                if result.as_ref().map_or(true, |chunk| chunk.eof) {
                    self.reads.remove(&session);
                }
                result.map(Response::Read)
            }
            Request::CreateFile { path } => {
                self.backend.create_file(&path).map(|()| Response::Unit)
            }
            Request::CreateDirectory { path, recursive } => self
                .backend
                .create_dir(&path, recursive)
                .map(|()| Response::Unit),
            Request::CopyFile { source, target } => self
                .backend
                .copy_file(&source, &target)
                .map(Response::Copied),
            Request::Rename { source, target } => self
                .backend
                .rename(&source, &target)
                .map(|()| Response::Unit),
            Request::Trash { path } => self.backend.trash(&path).map(|()| Response::Unit),
            Request::Save { save } => self.backend.save(save).map(Response::Save),
            Request::BeginSave {
                operation_id,
                path,
                expected,
                total_bytes,
            } => {
                // Staging consumes disk incrementally, not declared-size memory.
                // Bound active handles; each append and wire frame remains bounded.
                if self.saves.len() >= MAX_SAVES {
                    return Err(FsError::invalid(
                        "begin_save",
                        "staging session limit exceeded",
                    ));
                }
                let staged = self
                    .backend
                    .begin_save(operation_id, path, expected, total_bytes)?;
                let session = self.id()?;
                self.saves.insert(session, staged);
                Ok(Response::Session(session))
            }
            Request::AppendSave {
                session,
                offset,
                bytes,
            } => {
                let staged = self
                    .saves
                    .get_mut(&session)
                    .ok_or_else(|| FsError::invalid("append_save", "unknown session"))?;
                let result = staged.append(offset, &bytes);
                if result.is_err() {
                    self.saves.remove(&session);
                }
                result.map(|()| Response::Unit)
            }
            Request::CommitSave { session } => {
                let staged = self
                    .saves
                    .get(&session)
                    .ok_or_else(|| FsError::invalid("commit_save", "unknown session"))?;
                if staged.written() != staged.total_bytes() {
                    return Err(FsError::invalid(
                        "commit_save",
                        "staged upload is incomplete",
                    ));
                }
                let staged = self.saves.remove(&session).expect("session exists");
                self.backend.commit_save(staged).map(Response::Save)
            }
            Request::SaveOutcome { operation_id } => {
                Ok(Response::Save(self.backend.save_outcome(&operation_id)))
            }
            Request::OpenDirectory { path } => {
                if self.directories.len() >= MAX_SESSIONS {
                    return Err(FsError::invalid(
                        "open_directory",
                        "directory session limit reached",
                    ));
                }
                let path = path
                    .to_local()
                    .map_err(|e| FsError::from_io("open_directory", None, e))?;
                let dir = self
                    .backend
                    .local()
                    .read_dir(&path)
                    .map_err(|e| FsError::from_io("open_directory", Some(&path), e))?;
                let id = self.id()?;
                self.directories.insert(id, dir);
                Ok(Response::Session(id))
            }
            Request::NextDirectory { session } => {
                if self
                    .completed_pages
                    .contains(&(session, PageKind::Directory))
                {
                    return Ok(Response::Directory {
                        entries: Vec::new(),
                        done: true,
                    });
                }
                let dir = self
                    .directories
                    .get_mut(&session)
                    .ok_or_else(|| FsError::invalid("next_directory", "unknown session"))?;
                let mut entries = Vec::new();
                let mut done = false;
                for _ in 0..MAX_BATCH {
                    match dir.next() {
                        None => {
                            done = true;
                            break;
                        }
                        Some(Err(e)) => {
                            entries.push(Err(FsError::from_io("read_directory", None, e)))
                        }
                        Some(Ok(entry)) => {
                            let path = entry.path();
                            let metadata =
                                self.backend.metadata(&NativePath::from_local(&path), false);
                            entries.push(Ok(DirectoryEntry {
                                path: NativePath::from_local(&path),
                                metadata,
                            }));
                        }
                    }
                }
                if done {
                    self.directories.remove(&session);
                    self.complete_pages(session, PageKind::Directory);
                }
                Ok(Response::Directory { entries, done })
            }
            Request::StartSearch { root, options } => {
                self.start_search(root, options, SearchPathFilters::default())
            }
            Request::StartFilteredSearch {
                root,
                options,
                filters,
            } => self.start_search(root, options, filters),
            Request::NextSearch { session } => {
                if self.completed_pages.contains(&(session, PageKind::Search)) {
                    return Ok(Response::Search {
                        items: Vec::new(),
                        done: true,
                    });
                }
                let search = self
                    .searches
                    .get_mut(&session)
                    .ok_or_else(|| FsError::invalid("next_search", "unknown session"))?;
                let (items, done) = search.next_page(&self.backend);
                if done {
                    self.searches.remove(&session);
                    self.complete_pages(session, PageKind::Search);
                }
                Ok(Response::Search { items, done })
            }
            Request::StartWatch {
                root,
                generation,
                recursive,
            } => {
                if self.watches.len() >= MAX_SESSIONS {
                    return Err(FsError::invalid("watch", "watch session limit reached"));
                }
                self.watch_roots.retain(|_, root| root.strong_count() > 0);
                let key = (root, recursive);
                let watch_root = match self.watch_roots.get(&key).and_then(Weak::upgrade) {
                    Some(root) => root,
                    None => {
                        let path = key
                            .0
                            .to_local()
                            .map_err(|error| FsError::from_io("watch", None, error))?;
                        let root = Arc::new(WatchRoot::new(&path, recursive));
                        self.watch_roots.insert(key, Arc::downgrade(&root));
                        root
                    }
                };
                let id = self.id()?;
                self.watches.insert(id, watch_root.subscribe(generation));
                Ok(Response::Session(id))
            }
            Request::PollWatch {
                session,
                after_sequence,
            } => {
                let watch = self
                    .watches
                    .get_mut(&session)
                    .ok_or_else(|| FsError::invalid("poll_watch", "unknown session"))?;
                let batch = watch.poll(after_sequence, Instant::now());
                Ok(Response::Watch(batch))
            }
            Request::Cancel { session } => {
                let completed = self
                    .completed_pages
                    .iter()
                    .position(|(id, _)| *id == session);
                let completed = completed
                    .and_then(|index| self.completed_pages.remove(index))
                    .is_some();
                let found = self.directories.remove(&session).is_some()
                    | self.reads.remove(&session).is_some()
                    | self.saves.remove(&session).is_some()
                    | self.searches.remove(&session).is_some()
                    | self.watches.remove(&session).is_some()
                    | completed;
                if !found {
                    return Err(FsError::invalid("cancel", "unknown session"));
                }
                Ok(Response::Unit)
            }
        }
    }
}

impl SearchSession {
    fn next_page(&mut self, backend: &LocalBackend) -> (Vec<SearchItem>, bool) {
        let mut items = Vec::new();
        let mut steps = 0;
        while items.len() < SEARCH_PAGE && steps < SEARCH_STEPS {
            if let Some(pending) = &mut self.pending {
                if let Some(item) = pending.next() {
                    items.push(item);
                    continue;
                }
                self.pending = None;
            }
            steps += 1;
            if let Some(loading) = &mut self.loading {
                match loading.step(backend.local()) {
                    Ok(SearchSnapshotStep::Text(text, revision)) => {
                        let line_end = text.as_str().find('\n').unwrap_or(text.as_str().len());
                        self.pending = Some(PendingMatches {
                            path: NativePath::from_local(loading.path()),
                            text,
                            revision,
                            line: 0,
                            line_end,
                            cursor: self.cursor.clone(),
                            offsets: SourceOffsets::default(),
                        });
                        self.loading = None;
                    }
                    Ok(SearchSnapshotStep::Pending) => {}
                    Ok(SearchSnapshotStep::Binary) => self.loading = None,
                    Err(error) => {
                        items.push(SearchItem::Error(FsError::from_io(
                            "search_read",
                            Some(loading.path()),
                            error,
                        )));
                        self.loading = None;
                    }
                }
                continue;
            }
            let (path, is_root) = if let Some(root) = self.root.take() {
                (root, true)
            } else if let Some(dir) = self.dirs.last_mut() {
                match dir.next() {
                    Some(Ok(entry)) => (entry.path(), false),
                    Some(Err(e)) => {
                        items.push(SearchItem::Error(FsError::from_io(
                            "search_directory",
                            None,
                            e,
                        )));
                        continue;
                    }
                    None => {
                        self.dirs.pop();
                        continue;
                    }
                }
            } else {
                break;
            };
            let relative = match path.strip_prefix(&self.base) {
                Ok(path) => relative_path_text(path),
                Err(error) => {
                    items.push(SearchItem::Error(FsError::invalid(
                        "search_path",
                        &error.to_string(),
                    )));
                    continue;
                }
            };
            if !is_root && self.filters.is_excluded(&relative) {
                continue;
            }
            let meta = match backend.local().symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(e) => {
                    items.push(SearchItem::Error(FsError::from_io(
                        "search_metadata",
                        Some(&path),
                        e,
                    )));
                    continue;
                }
            };
            if meta.file_type().is_symlink() || (!is_root && should_skip_path(&path, meta.is_dir()))
            {
                continue;
            }
            if meta.is_dir() {
                if self.dirs.len() >= MAX_DEPTH {
                    items.push(SearchItem::Error(FsError::invalid(
                        "search",
                        "directory depth limit exceeded",
                    )));
                    continue;
                }
                match backend.local().read_dir(&path) {
                    Ok(dir) => self.dirs.push(dir),
                    Err(e) => items.push(SearchItem::Error(FsError::from_io(
                        "search_directory",
                        Some(&path),
                        e,
                    ))),
                }
            } else if meta.is_file() {
                if !is_root && !self.filters.accepts_file(&relative) {
                    continue;
                }
                if !crate::content::classify_content(&path, &[]).supports_text_search() {
                    continue;
                }
                match SearchSnapshot::open(backend.local(), &path) {
                    Ok(snapshot) => self.loading = Some(snapshot),
                    Err(error) => items.push(SearchItem::Error(FsError::from_io(
                        "search_open",
                        Some(&path),
                        error,
                    ))),
                }
            }
        }
        let done = self.root.is_none()
            && self.dirs.is_empty()
            && self.pending.is_none()
            && self.loading.is_none();
        (items, done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Filesystem;
    use std::fs;
    use std::path::Path;

    #[test]
    fn watcher_overflow_and_sequence_gaps_invalidate() {
        let mut q = WatchQueue {
            sequence: 0,
            invalidated: false,
            error: None,
            events: VecDeque::new(),
        };
        let path = NativePath::from_local(Path::new("x"));
        for _ in 0..=WATCH_CAPACITY {
            q.push(vec![path.clone()]);
        }

        let batch = q.poll(9, 0);
        assert!(batch.invalidated);
        assert_eq!(batch.generation, 9);
        q.push(vec![path]);
        assert!(q.poll(9, 0).invalidated);
        q.push(vec![NativePath::Unix(vec![255; MAX_WATCH_EVENT_BYTES])]);
        assert!(q.poll(9, q.sequence - 1).invalidated);
    }

    #[test]
    fn watch_reconciliation_repairs_missed_events_and_reports_native_watch_failure() {
        let now = Instant::now();
        let root = Arc::new(WatchRoot {
            _watcher: None,
            subscribers: Arc::new(Mutex::new(WatchSubscribers {
                error: Some(FsError::from_io(
                    "watch",
                    None,
                    io::Error::new(io::ErrorKind::PermissionDenied, "fixture"),
                )),
                ..WatchSubscribers::default()
            })),
        });
        let mut watch = root.subscribe(7);
        watch.last_reconcile = now;
        let initial = watch.poll(0, now);
        assert!(initial.invalidated);
        assert_eq!(initial.error.unwrap().kind, "PermissionDenied");
        assert!(!watch.poll(initial.sequence, now).invalidated);
        let reconciled = watch.poll(initial.sequence, now + WATCH_RECONCILE_INTERVAL);
        assert!(reconciled.invalidated);
        assert_eq!(reconciled.generation, 7);
        assert!(
            !watch
                .poll(reconciled.sequence, now + WATCH_RECONCILE_INTERVAL)
                .invalidated
        );
    }

    #[test]
    fn shared_watch_roots_keep_subscriber_cursors_and_lifetimes_independent() {
        let directory = tempfile::tempdir().unwrap();
        let path = NativePath::from_local(directory.path());
        let root = Arc::new(WatchRoot {
            _watcher: None,
            subscribers: Arc::new(Mutex::new(WatchSubscribers::default())),
        });
        let mut service = ready_service();
        service
            .watch_roots
            .insert((path.clone(), true), Arc::downgrade(&root));
        let mut start = |generation| {
            let Response::Session(id) = service
                .execute(Request::StartWatch {
                    root: path.clone(),
                    generation,
                    recursive: true,
                })
                .unwrap()
            else {
                panic!("expected watch session");
            };
            id
        };
        let first = start(7);
        let second = start(8);
        for id in [first, second] {
            let watch = service.watches.get_mut(&id).unwrap();
            assert!(Arc::ptr_eq(&watch._root, &root));
            assert!(watch.poll(0, Instant::now()).invalidated);
        }
        for kind in [
            notify::EventKind::Access(notify::event::AccessKind::Read),
            notify::EventKind::Modify(notify::event::ModifyKind::Metadata(
                notify::event::MetadataKind::AccessTime,
            )),
        ] {
            root.subscribers.lock().unwrap().publish(Ok(
                notify::Event::new(kind).add_path(directory.path().join("note.md"))
            ));
            for id in [first, second] {
                let batch = service
                    .watches
                    .get_mut(&id)
                    .unwrap()
                    .poll(0, Instant::now());
                assert!(batch.events.is_empty());
                assert!(!batch.invalidated);
            }
        }
        let publish = || {
            root.subscribers
                .lock()
                .unwrap()
                .publish(Ok(notify::Event::new(notify::EventKind::Any)
                    .add_path(directory.path().join("note.md"))));
        };
        publish();
        let first_batch = service
            .watches
            .get_mut(&first)
            .unwrap()
            .poll(0, Instant::now());
        assert_eq!(first_batch.generation, 7);
        assert_eq!(first_batch.events.len(), 1);
        assert!(
            !service
                .watches
                .get_mut(&first)
                .unwrap()
                .poll(first_batch.sequence, Instant::now())
                .invalidated
        );
        let second_batch = service
            .watches
            .get_mut(&second)
            .unwrap()
            .poll(0, Instant::now());
        assert_eq!(second_batch.generation, 8);
        assert_eq!(
            second_batch.events[0].sequence,
            first_batch.events[0].sequence
        );
        assert!(!second_batch.invalidated);

        service.execute(Request::Cancel { session: first }).unwrap();
        publish();
        let remaining = service
            .watches
            .get_mut(&second)
            .unwrap()
            .poll(second_batch.sequence, Instant::now());
        assert_eq!(remaining.events.len(), 1);
        assert_eq!(root.subscribers.lock().unwrap().queues.len(), 1);
        service
            .execute(Request::Cancel { session: second })
            .unwrap();
        let weak = Arc::downgrade(&root);
        drop(root);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn watcher_pages_fit_the_frame_even_with_large_paths() {
        let mut q = WatchQueue {
            sequence: 0,
            invalidated: false,
            error: None,
            events: VecDeque::new(),
        };
        for _ in 0..WATCH_PAGE {
            q.push(vec![NativePath::Windows(vec![65535; 5_000])]);
        }
        let batch = q.poll(3, 0);
        assert!(!batch.invalidated);
        assert_eq!(batch.events.len(), WATCH_PAGE);
        let mut output = Vec::new();
        crate::protocol::write_frame(
            &mut output,
            &Reply {
                id: 1,
                result: Ok(Response::Watch(batch)),
            },
        )
        .unwrap();
        assert!(output.len() <= crate::protocol::MAX_FRAME_BYTES + 4);
    }

    fn ready_service() -> Service {
        let mut service = Service::new(
            Arc::new(LocalBackend::new()),
            BuildIdentity {
                server_version: "test".into(),
                app_version: "test".into(),
                source_sha: "test".into(),
            },
        );
        service
            .execute(Request::Hello {
                protocol: PROTOCOL_VERSION,
                expected_build: None,
            })
            .unwrap();
        service
    }

    #[cfg(unix)]
    #[test]
    fn home_directory_uses_the_server_environment() {
        let home = std::env::var_os("HOME").expect("test HOME");
        let response = ready_service().execute(Request::HomeDirectory).unwrap();
        let Response::Path(path) = response else {
            panic!("expected home directory path");
        };
        assert_eq!(
            path.to_local().unwrap(),
            std::fs::canonicalize(std::path::Path::new(&home)).unwrap()
        );
    }

    #[test]
    fn starting_searches_beyond_the_limit_ends_the_oldest_instead_of_failing() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "needle").unwrap();
        let mut service = ready_service();
        let mut sessions = Vec::new();
        for _ in 0..MAX_SEARCHES + 2 {
            let Response::Session(session) = service
                .execute(Request::StartSearch {
                    root: NativePath::from_local(directory.path()),
                    options: SearchOptions {
                        query: "needle".into(),
                        case_sensitive: true,
                        whole_word: false,
                        regex: false,
                    },
                })
                .unwrap()
            else {
                panic!("expected search session");
            };
            sessions.push(session);
        }
        assert_eq!(service.searches.len(), MAX_SEARCHES);
        let Response::Search { items, done } = service
            .execute(Request::NextSearch {
                session: sessions[0],
            })
            .unwrap()
        else {
            panic!("expected search page");
        };
        assert!(items.is_empty() && done);
        assert!(service.searches.contains_key(sessions.last().unwrap()));
    }

    #[test]
    fn filtered_search_skips_excluded_subtrees_and_nonmatching_file_reads() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("skip")).unwrap();
        fs::write(root.join("skip/bad.md"), [255]).unwrap();
        fs::write(root.join("bad.txt"), [255]).unwrap();
        fs::write(root.join("keep.md"), "needle").unwrap();
        let mut service = ready_service();
        let options = SearchOptions {
            query: "needle".into(),
            case_sensitive: true,
            whole_word: false,
            regex: false,
        };
        assert!(
            service
                .execute(Request::StartFilteredSearch {
                    root: NativePath::from_local(root),
                    options: options.clone(),
                    filters: SearchPathFilters {
                        include: "[".into(),
                        exclude: String::new()
                    },
                })
                .is_err()
        );
        assert!(service.searches.is_empty());
        let Response::Session(session) = service
            .execute(Request::StartFilteredSearch {
                root: NativePath::from_local(root),
                options,
                filters: SearchPathFilters {
                    include: r"\.md$".into(),
                    exclude: "^skip$".into(),
                },
            })
            .unwrap()
        else {
            panic!("expected search session");
        };
        let mut matches = Vec::new();
        loop {
            let Response::Search { items, done } =
                service.execute(Request::NextSearch { session }).unwrap()
            else {
                panic!("expected search page");
            };
            for item in items {
                match item {
                    SearchItem::Match { path, .. } => matches.push(path),
                    SearchItem::Error(error) => panic!("excluded file was read: {error}"),
                }
            }
            if done {
                break;
            }
        }
        assert_eq!(matches, vec![NativePath::from_local(&root.join("keep.md"))]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn filtered_search_matches_display_text_without_losing_native_filename_bytes() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(OsStr::from_bytes(b"draft\\\xff.md"));
        fs::write(&path, "needle").unwrap();
        let mut service = ready_service();
        let Response::Session(session) = service
            .execute(Request::StartFilteredSearch {
                root: NativePath::from_local(directory.path()),
                options: SearchOptions {
                    query: "needle".into(),
                    case_sensitive: true,
                    whole_word: false,
                    regex: false,
                },
                filters: SearchPathFilters {
                    include: r"^draft\\.*\.md$".into(),
                    exclude: String::new(),
                },
            })
            .unwrap()
        else {
            panic!("expected search session");
        };
        let Response::Search { items, .. } =
            service.execute(Request::NextSearch { session }).unwrap()
        else {
            panic!("expected search page");
        };
        assert!(
            matches!(items.as_slice(), [SearchItem::Match { path: found, .. }] if found == &NativePath::from_local(&path))
        );
    }

    #[test]
    fn pipeline_tails_only_accept_known_completed_page_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = ready_service();
        let path = NativePath::from_local(directory.path());
        let Response::Session(list) = service
            .execute(Request::OpenDirectory { path: path.clone() })
            .unwrap()
        else {
            panic!("missing directory session")
        };
        let Response::Session(search) = service
            .execute(Request::StartSearch {
                root: path,
                options: SearchOptions {
                    query: "needle".into(),
                    case_sensitive: true,
                    whole_word: false,
                    regex: false,
                },
            })
            .unwrap()
        else {
            panic!("missing search session")
        };
        for _ in 0..4 {
            assert!(
                matches!(service.execute(Request::NextDirectory { session: list }).unwrap(), Response::Directory { entries, done: true } if entries.is_empty())
            );
            assert!(
                matches!(service.execute(Request::NextSearch { session: search }).unwrap(), Response::Search { items, done: true } if items.is_empty())
            );
        }
        assert!(
            service
                .execute(Request::NextSearch { session: list })
                .is_err()
        );
        assert!(
            service
                .execute(Request::NextDirectory { session: 999 })
                .is_err()
        );
        service
            .execute(Request::Cancel { session: search })
            .unwrap();
        assert!(
            service
                .execute(Request::NextSearch { session: search })
                .is_err()
        );
    }

    #[test]
    fn large_staged_save_and_read_sessions_stream_and_detect_changes() {
        use crate::backend::MAX_READ_BYTES;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes");
        fs::write(&file, b"old").unwrap();
        let path = NativePath::from_local(&file);
        let mut service = ready_service();
        let expected = ExpectedRevision::Present(service.backend.revision(&file).unwrap());
        let bytes: Vec<u8> = (0..MAX_READ_BYTES * 3 + 137)
            .map(|i| (i % 251) as u8)
            .collect();
        let Response::Session(session) = service
            .execute(Request::BeginSave {
                operation_id: "large".into(),
                path: path.clone(),
                expected,
                total_bytes: bytes.len() as u64,
            })
            .unwrap()
        else {
            panic!("missing save session")
        };
        assert!(service.execute(Request::CommitSave { session }).is_err());
        for (index, chunk) in bytes.chunks(MAX_READ_BYTES).enumerate() {
            service
                .execute(Request::AppendSave {
                    session,
                    offset: (index * MAX_READ_BYTES) as u64,
                    bytes: chunk.to_vec(),
                })
                .unwrap();
        }
        assert!(matches!(
            service.execute(Request::CommitSave { session }).unwrap(),
            Response::Save(SaveOutcome::Saved(_))
        ));
        assert_eq!(fs::read(&file).unwrap(), bytes);
        let Response::ReadOpened { session, revision } = service
            .execute(Request::OpenRead { path: path.clone() })
            .unwrap()
        else {
            panic!("missing read session")
        };
        let mut result = Vec::new();
        loop {
            let Response::Read(chunk) = service
                .execute(Request::NextRead {
                    session,
                    length: MAX_READ_BYTES,
                })
                .unwrap()
            else {
                panic!("missing read chunk")
            };
            assert_eq!(chunk.offset, result.len() as u64);
            assert_eq!(chunk.revision, revision);
            result.extend(chunk.bytes);
            if chunk.eof {
                break;
            }
        }
        assert_eq!(result, bytes);
        assert!(
            service
                .execute(Request::NextRead { session, length: 1 })
                .is_err()
        );

        let Response::ReadOpened { session, .. } =
            service.execute(Request::OpenRead { path }).unwrap()
        else {
            panic!("missing read session")
        };
        service
            .execute(Request::NextRead {
                session,
                length: MAX_READ_BYTES,
            })
            .unwrap();
        fs::write(&file, vec![b'x'; bytes.len()]).unwrap();
        assert!(
            service
                .execute(Request::NextRead {
                    session,
                    length: MAX_READ_BYTES
                })
                .is_ok()
        );
        assert!(
            service
                .execute(Request::NextRead {
                    session,
                    length: MAX_READ_BYTES
                })
                .is_ok()
        );
        assert!(
            service
                .execute(Request::NextRead {
                    session,
                    length: MAX_READ_BYTES
                })
                .is_err()
        );
        assert!(
            service
                .execute(Request::NextRead {
                    session,
                    length: MAX_READ_BYTES
                })
                .is_err()
        );
    }

    #[test]
    fn staged_large_uploads_bound_chunks_and_sessions_not_declared_file_sizes() {
        let dir = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let file = dir.path().join("large");
        let mut service = ready_service();
        let mut sessions = Vec::new();
        for index in 0..MAX_SAVES {
            let Response::Session(session) = service
                .execute(Request::BeginSave {
                    operation_id: format!("large-{index}"),
                    path: NativePath::from_local(&file),
                    expected: ExpectedRevision::Missing,
                    total_bytes: u64::MAX,
                })
                .unwrap()
            else {
                panic!("expected staged session");
            };
            sessions.push(session);
        }
        assert!(
            service
                .execute(Request::BeginSave {
                    operation_id: "overflow".into(),
                    path: NativePath::from_local(&file),
                    expected: ExpectedRevision::Missing,
                    total_bytes: 1,
                })
                .is_err()
        );
        service
            .execute(Request::AppendSave {
                session: sessions[0],
                offset: 0,
                bytes: vec![7; crate::backend::MAX_READ_BYTES],
            })
            .unwrap();
        let allocated: u64 = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum();
        assert_eq!(allocated, crate::backend::MAX_READ_BYTES as u64);
        assert!(
            service
                .execute(Request::AppendSave {
                    session: sessions[1],
                    offset: 0,
                    bytes: vec![0; crate::backend::MAX_READ_BYTES + 1],
                })
                .is_err()
        );
        sessions.remove(1);
        assert!(
            service
                .execute(Request::CommitSave {
                    session: sessions[0]
                })
                .is_err()
        );
        assert!(!file.exists());
        for session in sessions {
            service.execute(Request::Cancel { session }).unwrap();
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_protocol_preserves_dangling_native_targets_and_never_overwrites() {
        use crate::protocol::{read_frame, write_frame};
        let dir = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let link = dir.path().join("link");
        let native_link = NativePath::from_local(&link);
        let target = NativePath::Unix(b"../missing\xff".to_vec());
        let mut service = ready_service();
        let request = Request::CreateSymlink {
            target: target.clone(),
            link: native_link.clone(),
            directory: None,
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &request).unwrap();
        let request = read_frame::<Request>(&mut wire.as_slice())
            .unwrap()
            .unwrap();
        assert!(matches!(service.execute(request).unwrap(), Response::Unit));
        assert!(
            service
                .backend
                .metadata(&native_link, false)
                .unwrap()
                .is_symlink
        );
        assert_eq!(service.backend.read_link(&native_link).unwrap(), target);
        let error = service
            .execute(Request::CreateSymlink {
                target: NativePath::Unix(b"different".to_vec()),
                link: native_link.clone(),
                directory: None,
            })
            .unwrap_err();
        assert_eq!(error.kind, "AlreadyExists");
        assert_eq!(service.backend.read_link(&native_link).unwrap(), target);
        let regular = dir.path().join("regular");
        fs::write(&regular, b"keep").unwrap();
        assert_eq!(
            service
                .execute(Request::CreateSymlink {
                    target,
                    link: NativePath::from_local(&regular),
                    directory: None,
                })
                .unwrap_err()
                .kind,
            "AlreadyExists"
        );
        assert_eq!(fs::read(&regular).unwrap(), b"keep");
    }

    #[cfg(windows)]
    #[test]
    fn symlink_protocol_rejects_unknown_windows_link_kinds_without_mutation() {
        let dir = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let link = dir.path().join("link");
        let mut service = ready_service();
        assert_eq!(
            service
                .execute(Request::CreateSymlink {
                    target: NativePath::from_local(std::path::Path::new("missing")),
                    link: NativePath::from_local(&link),
                    directory: None,
                })
                .unwrap_err()
                .kind,
            "Unsupported"
        );
        assert!(fs::symlink_metadata(link).is_err());
    }

    #[test]
    fn staged_upload_rejects_bad_offsets_and_detects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes");
        fs::write(&file, b"old").unwrap();
        let path = NativePath::from_local(&file);
        let mut service = ready_service();
        let expected = ExpectedRevision::Present(service.backend.revision(&file).unwrap());
        let begin = || Request::BeginSave {
            operation_id: "id".into(),
            path: path.clone(),
            expected: expected.clone(),
            total_bytes: 3,
        };
        let Response::Session(session) = service.execute(begin()).unwrap() else {
            panic!()
        };
        assert!(
            service
                .execute(Request::AppendSave {
                    session,
                    offset: 1,
                    bytes: b"bad".to_vec()
                })
                .is_err()
        );
        assert!(service.execute(Request::CommitSave { session }).is_err());
        let Response::Session(session) = service.execute(begin()).unwrap() else {
            panic!()
        };
        service
            .execute(Request::AppendSave {
                session,
                offset: 0,
                bytes: b"new".to_vec(),
            })
            .unwrap();
        fs::write(&file, b"external").unwrap();
        assert!(matches!(
            service.execute(Request::CommitSave { session }).unwrap(),
            Response::Save(SaveOutcome::Conflict(_))
        ));
        assert_eq!(fs::read(&file).unwrap(), b"external");
    }

    #[test]
    fn staged_create_is_no_clobber_and_cancellation_drops_staging() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("new");
        let path = NativePath::from_local(&file);
        let mut service = ready_service();
        let begin = || Request::BeginSave {
            operation_id: "new".into(),
            path: path.clone(),
            expected: ExpectedRevision::Missing,
            total_bytes: 3,
        };
        let Response::Session(session) = service.execute(begin()).unwrap() else {
            panic!()
        };
        service
            .execute(Request::AppendSave {
                session,
                offset: 0,
                bytes: b"one".to_vec(),
            })
            .unwrap();
        service.execute(Request::Cancel { session }).unwrap();
        assert!(!file.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        let Response::Session(session) = service.execute(begin()).unwrap() else {
            panic!()
        };
        service
            .execute(Request::AppendSave {
                session,
                offset: 0,
                bytes: b"one".to_vec(),
            })
            .unwrap();
        fs::write(&file, b"external").unwrap();
        assert!(matches!(
            service.execute(Request::CommitSave { session }).unwrap(),
            Response::Save(SaveOutcome::Conflict(_))
        ));
        assert_eq!(fs::read(&file).unwrap(), b"external");
        fs::remove_file(&file).unwrap();
        let Response::Session(session) = service
            .execute(Request::BeginSave {
                operation_id: "new2".into(),
                path,
                expected: ExpectedRevision::Missing,
                total_bytes: 3,
            })
            .unwrap()
        else {
            panic!()
        };
        service
            .execute(Request::AppendSave {
                session,
                offset: 0,
                bytes: b"one".to_vec(),
            })
            .unwrap();
        assert!(matches!(
            service.execute(Request::CommitSave { session }).unwrap(),
            Response::Save(SaveOutcome::Saved(_))
        ));
        assert_eq!(fs::read(file).unwrap(), b"one");
    }

    #[test]
    fn search_pages_preserve_matches_and_skip_binary_files_without_errors() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes"), "Äpfel äPFEL\nÄpfel").unwrap();
        fs::write(dir.path().join(".hidden"), "Äpfel").unwrap();
        fs::write(dir.path().join("invalid"), [255, 254]).unwrap();
        fs::write(dir.path().join("picture.PNG"), "Äpfel").unwrap();
        fs::write(dir.path().join("pdf"), "%PDF-1.7\nÄpfel").unwrap();
        fs::write(dir.path().join("binary"), "Äpfel\0").unwrap();
        fs::write(dir.path().join("large"), vec![b'x'; 256 * 1024 + 1]).unwrap();
        let backend = Arc::new(LocalBackend::new());
        let mut service = Service::new(
            backend,
            BuildIdentity {
                server_version: "test".into(),
                app_version: "test".into(),
                source_sha: "test".into(),
            },
        );
        service
            .execute(Request::Hello {
                protocol: PROTOCOL_VERSION,
                expected_build: None,
            })
            .unwrap();
        let id = match service
            .execute(Request::StartSearch {
                root: NativePath::from_local(dir.path()),
                options: SearchOptions {
                    query: "äpfel".into(),
                    case_sensitive: false,
                    whole_word: false,
                    regex: false,
                },
            })
            .unwrap()
        {
            Response::Session(id) => id,
            _ => panic!("missing session"),
        };
        let mut matches = Vec::new();
        let mut errors = Vec::new();
        loop {
            let Response::Search { items, done } = service
                .execute(Request::NextSearch { session: id })
                .unwrap()
            else {
                panic!("missing page")
            };
            for item in items {
                match item {
                    SearchItem::Match {
                        start, end, line, ..
                    } => matches.push((start, end, line)),
                    SearchItem::Error(error) => errors.push(error),
                }
            }
            if done {
                break;
            }
        }
        matches.sort();
        assert_eq!(matches, vec![(0, 6, 0), (7, 13, 0), (14, 20, 1)]);
        assert!(errors.is_empty(), "{errors:?}");
        let Response::Session(session) = service
            .execute(Request::StartSearch {
                root: NativePath::from_local(&dir.path().join("missing")),
                options: SearchOptions {
                    query: "needle".into(),
                    case_sensitive: true,
                    whole_word: false,
                    regex: false,
                },
            })
            .unwrap()
        else {
            panic!("missing session");
        };
        let Response::Search { items, done } =
            service.execute(Request::NextSearch { session }).unwrap()
        else {
            panic!("missing page");
        };
        assert!(done);
        assert!(matches!(
            &items[..],
            [SearchItem::Error(error)] if error.kind == "NotFound"
        ));
    }

    #[test]
    fn directory_batch_reports_missing_entries_without_hiding_success() {
        let dir = tempfile::tempdir().unwrap();
        let good = NativePath::from_local(dir.path());
        let missing = NativePath::from_local(&dir.path().join("missing"));
        let results = LocalBackend::new()
            .metadata_batch(&[good, missing], false)
            .unwrap();
        assert!(results[0].is_ok());
        assert_eq!(results[1].as_ref().unwrap_err().kind, "NotFound");
    }

    #[test]
    fn search_streams_all_results_across_pages() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes");
        fs::write(&file, "word ".repeat(80)).unwrap();
        let mut service = Service::new(
            Arc::new(LocalBackend::new()),
            BuildIdentity {
                server_version: "test".into(),
                app_version: "test".into(),
                source_sha: "test".into(),
            },
        );
        service
            .execute(Request::Hello {
                protocol: PROTOCOL_VERSION,
                expected_build: None,
            })
            .unwrap();
        let Response::Session(session) = service
            .execute(Request::StartSearch {
                root: NativePath::from_local(&file),
                options: SearchOptions {
                    query: "word".into(),
                    case_sensitive: true,
                    whole_word: true,
                    regex: false,
                },
            })
            .unwrap()
        else {
            panic!("missing session")
        };
        let mut offsets = Vec::new();
        loop {
            let Response::Search { items, done } =
                service.execute(Request::NextSearch { session }).unwrap()
            else {
                panic!("missing page")
            };
            for item in items {
                let SearchItem::Match { start, .. } = item else {
                    panic!("unexpected search error")
                };
                offsets.push(start);
            }
            if done {
                break;
            }
        }
        assert_eq!(offsets, (0..80).map(|i| i * 5).collect::<Vec<_>>());
    }

    #[test]
    fn large_file_search_keeps_cross_chunk_multiline_matches_and_yields_during_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes");
        let start = 5 * 1024 * 1024 - 3;
        let mut contents = "x".repeat(start);
        contents.push_str("start\nend");
        fs::write(&file, contents).unwrap();
        for (query, regex) in [("START\nEND", false), ("start\\nend", true)] {
            let mut service = ready_service();
            let Response::Session(session) = service
                .execute(Request::StartSearch {
                    root: NativePath::from_local(&file),
                    options: SearchOptions {
                        query: query.into(),
                        case_sensitive: false,
                        whole_word: false,
                        regex,
                    },
                })
                .unwrap()
            else {
                panic!("missing session")
            };
            let mut yielded = false;
            let mut found = Vec::new();
            loop {
                let Response::Search { items, done } =
                    service.execute(Request::NextSearch { session }).unwrap()
                else {
                    panic!("missing page")
                };
                yielded |= items.is_empty() && !done;
                for item in items {
                    match item {
                        SearchItem::Match {
                            start, end, line, ..
                        } => found.push((start, end, line)),
                        SearchItem::Error(error) => panic!("search failed: {error}"),
                    }
                }
                if done {
                    break;
                }
            }
            assert!(yielded);
            assert_eq!(found, [(start, start + 9, 0)]);
        }
    }
}
