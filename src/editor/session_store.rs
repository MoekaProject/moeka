use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};
use std::{fs, io};

use crate::platform::fs_task;

#[derive(Default)]
pub(crate) struct SessionRestoreState {
    pending: HashMap<String, PendingTabRestore>,
    workspace_access: HashMap<PathBuf, WorkspaceAccess>,
    queues: HashMap<PathBuf, VecDeque<String>>,
    loading_scopes: HashSet<PathBuf>,
}

struct WorkspaceAccess {
    bookmark: String,
    loading: bool,
    resolved: bool,
    request_root: Option<PathBuf>,
}

impl WorkspaceAccess {
    fn begin(&mut self, root: &Path) -> Option<String> {
        if self.loading || self.resolved {
            return None;
        }
        self.loading = true;
        self.request_root = Some(root.to_path_buf());
        Some(self.bookmark.clone())
    }
}

struct PendingTabRestore {
    record: TabSessionRecord,
    initial_text: String,
    loading: bool,
}

static BOOKMARK_EXECUTOR: OnceLock<gpui::BackgroundExecutor> = OnceLock::new();

#[cfg(target_os = "macos")]
fn bookmark_attempts() -> &'static Mutex<HashSet<PathBuf>> {
    static ATTEMPTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    ATTEMPTS.get_or_init(Mutex::default)
}

use super::config_store::{
    self, PaneLayoutRecord, SessionConfig, TabHistoryIndex, TabRecoveryState, TabSessionRecord,
    TabViewState,
};
use super::history_store;
use super::*;

enum SessionWrite {
    Save(config_store::SessionsFile),
    Flush(mpsc::Sender<()>),
}

fn run_session_writer(
    receiver: mpsc::Receiver<SessionWrite>,
    mut write: impl FnMut(&config_store::SessionsFile),
) {
    'writer: while let Ok(message) = receiver.recv() {
        match message {
            SessionWrite::Save(mut config) => loop {
                match receiver.try_recv() {
                    Ok(SessionWrite::Save(next)) => config = next,
                    Ok(SessionWrite::Flush(ack)) => {
                        write(&config);
                        let _ = ack.send(());
                        continue 'writer;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        write(&config);
                        continue 'writer;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        write(&config);
                        return;
                    }
                }
            },
            SessionWrite::Flush(ack) => {
                let _ = ack.send(());
            }
        }
    }
}

fn session_cache() -> &'static Mutex<config_store::SessionsFile> {
    static CACHE: OnceLock<Mutex<config_store::SessionsFile>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(config_store::load_sessions()))
}

fn session_writer() -> &'static mpsc::Sender<SessionWrite> {
    static WRITER: OnceLock<mpsc::Sender<SessionWrite>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || run_session_writer(receiver, config_store::save_sessions));
        sender
    })
}

pub(crate) fn flush_session_writes() {
    let (sender, receiver) = mpsc::channel();
    if session_writer().send(SessionWrite::Flush(sender)).is_ok() {
        let _ = receiver.recv_timeout(Duration::from_millis(500));
    }
}

pub(crate) fn persist_session(editor: &EditorView) {
    let mut cached = session_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config = &mut *cached;
    config.workspaces.roots = editor.workspaces.clone();
    config.workspaces.active_root = if editor.sidebar_root.as_os_str().is_empty() {
        None
    } else {
        Some(editor.sidebar_root.clone())
    };
    config
        .workspaces
        .bookmarks
        .retain(|root, _| editor.workspaces.contains(root));
    let leaf_ids = editor.pane_layout.leaf_ids();
    let focused_pane_leaf = leaf_ids.iter().position(|id| *id == editor.focused_pane_id);
    let session = SessionConfig {
        active_tab_id: editor.active_tab_id.clone(),
        next_tab_id: Some(editor.next_tab_id),
        // Legacy flat list: the currently focused pane's tabs.
        tabs: editor
            .tabs
            .iter()
            .map(|tab| session_tab_to_record(editor, tab))
            .collect(),
        pane_layout: Some(pane_layout_to_record(editor, &editor.pane_layout)),
        focused_pane_leaf,
    };
    // Startup/theme refreshes can operate on the default welcome tab before a
    // real restored session is ready. Preserve a previously saved work session
    // instead of overwriting it with that placeholder state.
    if !is_default_welcome_session(editor) || !session_contains_nonwelcome_tabs(&config.sessions) {
        config.sessions = session;
    }
    let snapshot = config.clone();
    let _ = session_writer().send(SessionWrite::Save(snapshot.clone()));
    drop(cached);
    persist_workspace_bookmarks(&snapshot.workspaces);
}

fn with_bookmark_pool<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "macos")]
    {
        objc::rc::autoreleasepool(f)
    }
    #[cfg(not(target_os = "macos"))]
    {
        f()
    }
}

fn persist_workspace_bookmarks(workspaces: &config_store::WorkspaceConfig) {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = workspaces;
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let Some(executor) = BOOKMARK_EXECUTOR.get() else {
            return;
        };
        for root in &workspaces.roots {
            if workspaces.bookmarks.contains_key(root)
                || !bookmark_attempts()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(root.clone())
            {
                continue;
            }
            let root = root.clone();
            executor
                .spawn(async move {
                    let bookmark_root = root.clone();
                    let result =
                        fs_task::run_value("Create workspace bookmark", root.clone(), move || {
                            with_bookmark_pool(|| {
                                crate::platform::security_scope::bookmark_for(&bookmark_root)
                            })
                        })
                        .await;
                    match result {
                        Ok(Some(bookmark)) => {
                            let mut cached =
                                session_cache().lock().unwrap_or_else(|e| e.into_inner());
                            if !cached.workspaces.roots.contains(&root) {
                                return;
                            }
                            cached.workspaces.bookmarks.insert(root, bookmark);
                            let snapshot = cached.clone();
                            let _ = session_writer().send(SessionWrite::Save(snapshot));
                            drop(cached);
                        }
                        Err(error) => log_error!("{error}"),
                        Ok(None) => {}
                    }
                })
                .detach();
        }
    }
}

fn is_default_welcome_session(editor: &EditorView) -> bool {
    let tabs = editor.all_tabs().collect::<Vec<_>>();
    tabs.len() == 1 && is_placeholder_welcome_tab(tabs[0])
}

fn is_placeholder_welcome_tab(tab: &OpenTab) -> bool {
    tab.id == WELCOME_TAB_ID && tab.path.is_none() && !tab.is_dirty
}

fn session_contains_nonwelcome_tabs(session: &SessionConfig) -> bool {
    fn layout_contains_nonwelcome(layout: &PaneLayoutRecord) -> bool {
        match layout {
            PaneLayoutRecord::Leaf { tabs, .. } => tabs
                .iter()
                .any(|tab| tab.id != WELCOME_TAB_ID || tab.path.is_some()),
            PaneLayoutRecord::Split { children, .. } => {
                children.iter().any(layout_contains_nonwelcome)
            }
        }
    }

    session
        .pane_layout
        .as_ref()
        .is_some_and(layout_contains_nonwelcome)
        || session
            .tabs
            .iter()
            .any(|tab| tab.id != WELCOME_TAB_ID || tab.path.is_some())
}

fn pane_layout_to_record(editor: &EditorView, layout: &PaneLayout) -> PaneLayoutRecord {
    match layout {
        PaneLayout::Leaf(pane_id) => {
            let (tabs, active_tab_id) = if *pane_id == editor.current_pane_id() {
                (&editor.tabs, &editor.active_tab_id)
            } else if let Some(index) = editor.pane_index(*pane_id) {
                (
                    &editor.panes[index].state.tabs,
                    &editor.panes[index].state.active_tab_id,
                )
            } else {
                (&editor.tabs, &editor.active_tab_id)
            };
            PaneLayoutRecord::Leaf {
                tabs: tabs
                    .iter()
                    .map(|tab| session_tab_to_record(editor, tab))
                    .collect(),
                active_tab_id: active_tab_id.clone(),
            }
        }
        PaneLayout::Split {
            axis,
            children,
            flexes,
        } => PaneLayoutRecord::Split {
            axis: match axis {
                SplitAxis::Horizontal => "horizontal".to_string(),
                SplitAxis::Vertical => "vertical".to_string(),
            },
            flexes: flexes.clone(),
            children: children
                .iter()
                .map(|child| pane_layout_to_record(editor, child))
                .collect(),
        },
    }
}

pub(crate) fn restore_session(editor: &mut EditorView, cx: &mut Context<EditorView>) {
    let _ = BOOKMARK_EXECUTOR.set(cx.background_executor().clone());
    let config = session_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    editor.workspaces = config.workspaces.roots.clone();
    for root in &editor.workspaces {
        fs_task::register_workspace_root(root);
    }
    if let Some(active_root) = config.workspaces.active_root.clone() {
        fs_task::register_workspace_root(&active_root);
        editor.sidebar_root = active_root.clone();
        editor.sidebar_visible = true;
        editor.expanded_dirs.clear();
        editor.expanded_dirs.insert(active_root);
    }

    let restored_layout = config.sessions.pane_layout.as_ref().is_some_and(|layout| {
        restore_pane_layout(
            editor,
            layout,
            config.sessions.focused_pane_leaf,
            config.sessions.next_tab_id,
        )
    });
    if !restored_layout {
        let tabs = config
            .sessions
            .tabs
            .iter()
            .filter_map(|record| restore_tab_metadata(record, editor.ui_language))
            .collect::<Vec<_>>();
        if !tabs.is_empty() {
            editor.tabs = tabs;
            editor.active_tab_id = config
                .sessions
                .active_tab_id
                .clone()
                .filter(|id| editor.tabs.iter().any(|tab| &tab.id == id))
                .or_else(|| editor.tabs.first().map(|tab| tab.id.clone()));
            editor.next_tab_id = config
                .sessions
                .next_tab_id
                .unwrap_or_else(|| infer_next_tab_id(&editor.tabs, editor.next_tab_id));
            editor.active_tab_index_cache.set(None);
            hydrate_active_tab_state(editor);
        }
    }

    fn collect(layout: &PaneLayoutRecord, records: &mut Vec<TabSessionRecord>) {
        match layout {
            PaneLayoutRecord::Leaf { tabs, .. } => records.extend(tabs.iter().cloned()),
            PaneLayoutRecord::Split { children, .. } => {
                for child in children {
                    collect(child, records);
                }
            }
        }
    }
    let mut records = Vec::new();
    if restored_layout {
        collect(config.sessions.pane_layout.as_ref().unwrap(), &mut records);
    } else {
        records = config.sessions.tabs.clone();
    }
    for record in records {
        if record.kind == "editor" && editor.pane_id_for_tab(&record.id).is_some() {
            let initial_text = editor
                .all_tabs()
                .find(|tab| tab.id == record.id)
                .map(|tab| tab.snapshot.text.clone())
                .unwrap_or_default();
            editor.session_restore.pending.insert(
                record.id.clone(),
                PendingTabRestore {
                    record,
                    initial_text,
                    loading: false,
                },
            );
        }
    }
    if cfg!(target_os = "macos") {
        for (root, bookmark) in config.workspaces.bookmarks {
            if editor.workspaces.contains(&root) {
                editor.session_restore.workspace_access.insert(
                    root,
                    WorkspaceAccess {
                        bookmark,
                        loading: false,
                        resolved: false,
                        request_root: None,
                    },
                );
            }
        }
    }
    schedule_pending_restores(editor, None, cx);
    // Roots without restored documents still need their security scope reopened.
    let roots = editor
        .session_restore
        .workspace_access
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for root in roots {
        schedule_workspace_access(editor, root, cx);
    }
}

pub(crate) fn restoring_path<'a>(editor: &'a EditorView, tab_id: &str) -> Option<&'a Path> {
    editor
        .session_restore
        .pending
        .get(tab_id)?
        .record
        .path
        .as_deref()
}

pub(crate) fn forget_restoring_tab(editor: &mut EditorView, tab_id: &str) {
    editor.session_restore.pending.remove(tab_id);
}

fn schedule_pending_restores(
    editor: &mut EditorView,
    root: Option<&Path>,
    cx: &mut Context<EditorView>,
) {
    let mut ids = editor
        .all_tabs()
        .filter(|tab| {
            editor
                .session_restore
                .pending
                .get(&tab.id)
                .is_some_and(|pending| {
                    root.is_none_or(|root| {
                        pending
                            .record
                            .path
                            .as_ref()
                            .is_some_and(|path| path.starts_with(root))
                    })
                })
        })
        .map(|tab| tab.id.clone())
        .collect::<Vec<_>>();
    ids.sort_by_key(|id| editor.active_tab_id.as_ref() != Some(id));
    for id in ids {
        schedule_tab_restore(editor, &id, cx);
    }
}

async fn run_restore_job<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    executor: &gpui::BackgroundExecutor,
    work: impl Fn() -> io::Result<T> + Send + Sync + 'static,
) -> io::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let work = Arc::new(work);
    loop {
        let job = work.clone();
        let remaining = deadline.saturating_duration_since(Instant::now());
        let result =
            fs_task::run_with_timeout(operation, path.clone(), remaining, move || job()).await;
        if result
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::WouldBlock)
        {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                executor
                    .timer(remaining.min(Duration::from_millis(50)))
                    .await;
                if Instant::now() < deadline {
                    continue;
                }
            }
        }
        return result;
    }
}

fn schedule_workspace_access(editor: &mut EditorView, root: PathBuf, cx: &mut Context<EditorView>) {
    let Some(access) = editor.session_restore.workspace_access.get_mut(&root) else {
        return;
    };
    let Some(bookmark) = access.begin(&root) else {
        return;
    };
    cx.spawn(async move |this, cx| {
        let result = run_restore_job(
            "Restore workspace access",
            root.clone(),
            cx.background_executor(),
            move || {
                Ok(with_bookmark_pool(|| {
                    crate::platform::security_scope::start_access(&bookmark)
                }))
            },
        )
        .await;
        this.update(cx, |editor, cx| {
            // An enclosing bookmark may have relocated this root while its access job ran.
            let Some(access_root) = editor
                .session_restore
                .workspace_access
                .iter()
                .find(|(_, access)| access.request_root.as_ref() == Some(&root))
                .map(|(root, _)| root.clone())
            else {
                return;
            };
            let access = editor
                .session_restore
                .workspace_access
                .get_mut(&access_root)
                .unwrap();
            access.loading = false;
            access.request_root = None;
            let resolved = match result {
                Ok(Some(resolved)) => {
                    access.resolved = true;
                    resolved
                }
                Ok(None) => {
                    log_error!("lost access to workspace {}", access_root.display());
                    access.resolved = true;
                    session_cache()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .workspaces
                        .bookmarks
                        .remove(&access_root);
                    schedule_pending_restores(editor, Some(&access_root), cx);
                    return;
                }
                Err(error) => {
                    editor.notify_error(error.to_string(), cx);
                    return;
                }
            };
            fs_task::register_workspace_root(&resolved);
            if resolved != access_root {
                relocate_restoring_workspace(editor, &access_root, &resolved, cx);
            }
            schedule_pending_restores(editor, Some(&resolved), cx);
        })
        .ok();
    })
    .detach();
}

fn relocate_record(record: &mut TabSessionRecord, from: &Path, to: &Path) {
    for path in [
        &mut record.path,
        &mut record.recovery.source_path,
        &mut record.history.undo_log_path,
    ] {
        if let Some(current) = path.as_ref()
            && let Ok(relative) = current.strip_prefix(from)
        {
            *path = Some(to.join(relative));
        }
    }
    if let Some(identity) = record.history.source_identity.as_ref()
        && let Ok(relative) = Path::new(identity).strip_prefix(from)
    {
        record.history.source_identity = Some(to.join(relative).to_string_lossy().into_owned());
    }
}

fn relocate_restoring_workspace(
    editor: &mut EditorView,
    from: &Path,
    to: &Path,
    cx: &mut Context<EditorView>,
) {
    for workspace in &mut editor.workspaces {
        if let Ok(relative) = workspace.strip_prefix(from) {
            *workspace = to.join(relative);
            fs_task::register_workspace_root(workspace);
        }
    }
    let access = std::mem::take(&mut editor.session_restore.workspace_access);
    for (root, state) in access {
        let relocated = root
            .strip_prefix(from)
            .map(|relative| to.join(relative))
            .unwrap_or(root);
        editor
            .session_restore
            .workspace_access
            .insert(relocated, state);
    }
    for pending in editor.session_restore.pending.values_mut() {
        relocate_record(&mut pending.record, from, to);
    }
    {
        let mut cached = session_cache().lock().unwrap_or_else(|e| e.into_inner());
        let bookmarks = std::mem::take(&mut cached.workspaces.bookmarks);
        for (root, bookmark) in bookmarks {
            let relocated = root
                .strip_prefix(from)
                .map(|relative| to.join(relative))
                .unwrap_or(root);
            cached.workspaces.bookmarks.insert(relocated, bookmark);
        }
    }
    if editor.sidebar_root == from {
        editor.switch_workspace(to.to_path_buf(), cx);
    }
    persist_session(editor);
}

fn restore_scope_for_path(path: &Path, roots: &[PathBuf]) -> PathBuf {
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .cloned()
        .unwrap_or_else(|| path.components().take(3).collect())
}

pub(crate) fn schedule_tab_restore(
    editor: &mut EditorView,
    tab_id: &str,
    cx: &mut Context<EditorView>,
) {
    let Some(pending) = editor.session_restore.pending.get(tab_id) else {
        return;
    };
    if pending.loading {
        return;
    }
    if let Some(path) = &pending.record.path {
        let access_root = editor
            .session_restore
            .workspace_access
            .iter()
            .filter(|(root, access)| path.starts_with(root) && !access.resolved)
            .max_by_key(|(root, _)| root.components().count())
            .map(|(root, _)| root.clone());
        if let Some(root) = access_root {
            schedule_workspace_access(editor, root, cx);
            return;
        }
    }
    let path = pending
        .record
        .path
        .clone()
        .or_else(|| pending.record.history.undo_log_path.clone())
        .unwrap_or_else(|| history_store::default_journal_path(tab_id, None));
    let scope = restore_scope_for_path(&path, &editor.workspaces);
    editor
        .session_restore
        .pending
        .get_mut(tab_id)
        .unwrap()
        .loading = true;
    editor
        .session_restore
        .queues
        .entry(scope.clone())
        .or_default()
        .push_back(tab_id.to_string());
    start_next_tab_restore(editor, scope, cx);
}

fn start_next_tab_restore(editor: &mut EditorView, scope: PathBuf, cx: &mut Context<EditorView>) {
    if editor.session_restore.loading_scopes.contains(&scope) {
        return;
    }
    let (record, initial_text) = loop {
        let Some(id) = editor
            .session_restore
            .queues
            .get_mut(&scope)
            .and_then(VecDeque::pop_front)
        else {
            return;
        };
        let Some(pending) = editor.session_restore.pending.get(&id) else {
            continue;
        };
        if editor.pane_id_for_tab(&id).is_none()
            || editor
                .all_tabs()
                .any(|tab| tab.id == id && tab.path.is_some())
        {
            editor.session_restore.pending.remove(&id);
            continue;
        }
        break (pending.record.clone(), pending.initial_text.clone());
    };
    editor.session_restore.loading_scopes.insert(scope.clone());
    let job_path = record
        .path
        .clone()
        .or_else(|| record.history.undo_log_path.clone())
        .unwrap_or_else(|| history_store::default_journal_path(&record.id, None));
    let load_record = record.clone();
    let ui_language = editor.ui_language;
    cx.spawn(async move |this, cx| {
        let result = run_restore_job(
            "Restore tab",
            job_path,
            cx.background_executor(),
            move || load_restored_tab(&load_record, ui_language),
        )
        .await;
        this.update(cx, |editor, cx| {
            let pause = result.as_ref().is_err_and(|error| {
                matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                )
            });
            finish_tab_restore(editor, &record, &initial_text, result, cx);
            editor.session_restore.loading_scopes.remove(&scope);
            if pause {
                // A stalled mount must not consume more workers or flood errors for every tab.
                if let Some(queued) = editor.session_restore.queues.remove(&scope) {
                    for id in queued {
                        if let Some(pending) = editor.session_restore.pending.get_mut(&id) {
                            pending.loading = false;
                        }
                    }
                }
            } else {
                start_next_tab_restore(editor, scope, cx);
            }
        })
        .ok();
    })
    .detach();
}

fn finish_tab_restore(
    editor: &mut EditorView,
    record: &TabSessionRecord,
    initial_text: &str,
    result: io::Result<RestoredTabData>,
    cx: &mut Context<EditorView>,
) {
    let Some(owner) = editor.pane_id_for_tab(&record.id) else {
        editor.session_restore.pending.remove(&record.id);
        return;
    };
    if editor
        .all_tabs()
        .any(|tab| tab.id == record.id && tab.path.is_some())
    {
        editor.session_restore.pending.remove(&record.id);
        return;
    }
    let Some(pending) = editor.session_restore.pending.get_mut(&record.id) else {
        return;
    };
    pending.loading = false;
    if pending.record != *record {
        return;
    }
    let mut loaded = match result {
        Ok(loaded) => loaded,
        Err(error) => {
            editor.notify_error(format!("{}: {error}", editor.t(TextKey::OpenFailed)), cx);
            return;
        }
    };
    editor.with_pane_if_alive(owner, |editor| {
        let Some(index) = editor.tabs.iter().position(|tab| tab.id == record.id) else {
            return;
        };
        let active = editor.active_tab_id.as_deref() == Some(record.id.as_str());
        let current_text = if active {
            editor.buffer.text()
        } else {
            &editor.tabs[index].snapshot.text
        };
        if (active && editor.pending_history_transaction.is_some())
            || !restore_can_apply(&editor.tabs[index], current_text, initial_text)
        {
            // Persist the live edit, not the stale snapshot that preceded this read.
            if active {
                editor.persist_active_tab_state();
            }
            return;
        }
        if record.recovery.text.is_some() {
            let current = if active {
                editor.current_snapshot()
            } else {
                editor.tabs[index].snapshot.clone()
            };
            loaded.snapshot.cursor_offset = current.cursor_offset;
            loaded.snapshot.cursor_anchor = current.cursor_anchor;
            loaded.snapshot.scroll_offset = current.scroll_offset;
            loaded.snapshot.extra_cursors = current.extra_cursors;
        }
        apply_restored_tab(&mut editor.tabs[index], loaded);
        editor.session_restore.pending.remove(&record.id);
        if active {
            editor.discard_history_transaction();
            hydrate_active_tab_state(editor);
            editor.invalidate_active_file_panel_caches();
        }
        cx.notify();
    });
    persist_session(editor);
}

/// Rebuilds the pane tree from a session record. Returns false when nothing
/// usable was restored (caller falls back to the legacy flat list).
fn restore_pane_layout(
    editor: &mut EditorView,
    record: &PaneLayoutRecord,
    focused_pane_leaf: Option<usize>,
    next_tab_id: Option<usize>,
) -> bool {
    let mut panes: Vec<Pane> = Vec::new();
    let Some(layout) = build_pane_layout(editor.ui_language, record, &mut panes) else {
        return false;
    };
    if panes.is_empty() {
        return false;
    }

    let leaf_ids = layout.leaf_ids();
    let focused_id = focused_pane_leaf
        .and_then(|index| leaf_ids.get(index).copied())
        .unwrap_or_else(|| layout.first_leaf());

    editor.next_pane_id = panes.iter().map(|pane| pane.id).max().unwrap_or(0) + 1;
    editor.pane_layout = layout;
    editor.panes = panes;
    editor.pane_bounds.clear();

    // Move the focused pane's restored tab state into the root fields
    // (discarding the default welcome state EditorView::new installed).
    let focused_index = editor
        .pane_index(focused_id)
        .expect("focused pane exists in freshly-built layout");
    let mut state = std::mem::replace(
        &mut editor.panes[focused_index].state,
        PaneState::detached(),
    );
    editor.swap_pane_state(&mut state);
    editor.panes[focused_index].state = PaneState::detached();
    editor.focused_pane_id = focused_id;
    editor.swapped_pane_id = None;

    editor.next_tab_id = next_tab_id.unwrap_or_else(|| {
        let inferred = editor
            .all_tabs()
            .filter_map(|tab| {
                tab.id
                    .strip_prefix("tab-")
                    .and_then(|raw| raw.parse::<usize>().ok())
            })
            .max()
            .map(|max| max + 1)
            .unwrap_or(editor.next_tab_id);
        inferred
    });

    // Hydrate every pane's live document state from its active tab snapshot.
    for pane_id in editor.pane_ids() {
        editor.with_pane(pane_id, hydrate_active_tab_state);
    }
    true
}

fn build_pane_layout(
    ui_language: UiLanguage,
    record: &PaneLayoutRecord,
    panes: &mut Vec<Pane>,
) -> Option<PaneLayout> {
    match record {
        PaneLayoutRecord::Leaf {
            tabs,
            active_tab_id,
        } => {
            let mut restored_tabs = Vec::new();
            for tab_record in tabs {
                if let Some(tab) = restore_tab_metadata(tab_record, ui_language) {
                    restored_tabs.push(tab);
                }
            }
            let pane_id = panes.iter().map(|pane| pane.id + 1).max().unwrap_or(0);
            let mut pane = Pane::new(pane_id);
            pane.state.active_tab_id = active_tab_id
                .clone()
                .filter(|id| restored_tabs.iter().any(|tab| &tab.id == id))
                .or_else(|| restored_tabs.first().map(|tab| tab.id.clone()));
            pane.state.tabs = restored_tabs;
            panes.push(pane);
            Some(PaneLayout::Leaf(pane_id))
        }
        PaneLayoutRecord::Split {
            axis,
            flexes,
            children,
        } => {
            let axis = match axis.as_str() {
                "vertical" => SplitAxis::Vertical,
                _ => SplitAxis::Horizontal,
            };
            let mut built = Vec::new();
            for child in children {
                if let Some(child_layout) = build_pane_layout(ui_language, child, panes) {
                    built.push(child_layout);
                }
            }
            match built.len() {
                0 => None,
                1 => Some(built.pop().expect("one child")),
                _ => {
                    let flexes = if flexes.len() == built.len() {
                        flexes.clone()
                    } else {
                        vec![1.0; built.len()]
                    };
                    Some(PaneLayout::Split {
                        axis,
                        children: built,
                        flexes,
                    })
                }
            }
        }
    }
}

/// Loads the (root-swapped-in) pane's active tab snapshot into the live
/// document fields; mirrors the legacy restore tail.
fn hydrate_active_tab_state(editor: &mut EditorView) {
    let Some(index) = editor.active_tab_index() else {
        return;
    };
    let active_tab = editor.tabs[index].clone();
    editor.current_file_path = active_tab.path.clone();
    editor.editor_left_aligned = active_tab.editor_left_aligned;
    editor.reading_mode = active_tab.reading_mode && !active_tab.source_mode;
    editor.source_mode = active_tab.source_mode;
    editor.source_word_wrap = active_tab.source_word_wrap;
    // Keep the file-kind cache in sync so the status bar (language label)
    // and file-kind-dependent paths work right after session restore.
    editor.active_file_kind_cache = active_tab.file_kind.clone();
    if active_tab.kind == TabKind::Editor {
        if active_tab
            .file_kind
            .as_ref()
            .is_some_and(FileKind::supports_text_buffer)
        {
            editor.buffer = Buffer::from_text(&active_tab.snapshot.text);
            editor.cursor.offset = active_tab.snapshot.cursor_offset.min(editor.buffer.len());
            editor.cursor.anchor = active_tab.snapshot.cursor_anchor.min(editor.buffer.len());
            editor.extra_cursors = active_tab
                .snapshot
                .extra_cursors
                .iter()
                .map(|(offset, anchor)| Cursor {
                    offset: (*offset).min(editor.buffer.len()),
                    anchor: (*anchor).min(editor.buffer.len()),
                })
                .collect();
        } else {
            editor.buffer = Buffer::from_text("");
            editor.cursor.offset = 0;
            editor.cursor.anchor = 0;
            editor.extra_cursors.clear();
        }
        editor.scroll_offset = active_tab.snapshot.scroll_offset;
        editor.extra_marked_ranges.clear();
        editor.reparse();
    }
}

fn session_tab_to_record(editor: &EditorView, tab: &OpenTab) -> TabSessionRecord {
    let Some(pending) = editor.session_restore.pending.get(&tab.id) else {
        return tab_to_record(tab);
    };
    pending_tab_to_record(tab, pending)
}

fn pending_tab_to_record(tab: &OpenTab, pending: &PendingTabRestore) -> TabSessionRecord {
    let mut record = pending.record.clone();
    record.title = tab.title.clone();
    record.is_preview = tab.is_preview;
    record.is_pinned = tab.is_pinned;
    record.view.editor_left_aligned = tab.editor_left_aligned;
    record.view.reading_mode = tab.reading_mode && !tab.source_mode;
    record.view.source_mode = tab.source_mode;
    record.view.source_word_wrap = tab.source_word_wrap;
    if tab.path.is_some() {
        return tab_to_record(tab);
    }
    let text_changed = tab.snapshot.text != pending.initial_text;
    let len = pending.initial_text.len();
    let view_changed = tab.snapshot.cursor_offset != pending.record.view.cursor_offset.min(len)
        || tab.snapshot.cursor_anchor != pending.record.view.cursor_anchor.min(len)
        || tab.snapshot.scroll_offset != pending.record.view.scroll_offset
        || !tab.snapshot.extra_cursors.iter().copied().eq(pending
            .record
            .view
            .extra_cursors
            .iter()
            .map(|(offset, anchor)| ((*offset).min(len), (*anchor).min(len))));
    if text_changed {
        record.recovery.text = Some(tab.snapshot.text.clone());
    }
    if text_changed || view_changed {
        record.view = tab_to_record(tab).view;
    }
    record
}

fn tab_to_record(tab: &OpenTab) -> TabSessionRecord {
    TabSessionRecord {
        id: tab.id.clone(),
        kind: serialize_tab_kind(tab.kind),
        title: tab.title.clone(),
        path: tab.path.clone(),
        file_kind: tab.file_kind.as_ref().map(serialize_file_kind),
        agent_cli: tab.kind.terminal_agent().map(serialize_agent_cli),
        is_preview: tab.is_preview,
        is_pinned: tab.is_pinned,
        view: TabViewState {
            cursor_offset: tab.snapshot.cursor_offset,
            cursor_anchor: tab.snapshot.cursor_anchor,
            scroll_offset: tab.snapshot.scroll_offset,
            editor_left_aligned: tab.editor_left_aligned,
            reading_mode: tab.reading_mode && !tab.source_mode,
            source_mode: tab.source_mode,
            source_word_wrap: tab.source_word_wrap,
            extra_cursors: tab.snapshot.extra_cursors.clone(),
        },
        recovery: tab.recovery.clone(),
        history: TabHistoryIndex {
            undo_log_path: tab.history.journal_path.clone(),
            source_identity: tab.history.source_identity.clone(),
            current_index: Some(tab.history.current_index()),
            entry_count: tab.history.entry_count(),
        },
    }
}

fn restore_tab_metadata(record: &TabSessionRecord, ui_language: UiLanguage) -> Option<OpenTab> {
    if record.kind == "terminal" {
        let agent_cli = record
            .agent_cli
            .as_deref()
            .and_then(deserialize_agent_cli)
            .unwrap_or(ChatAgentCli::Terminal);
        if agent_cli != ChatAgentCli::Terminal {
            return None;
        }
        return Some(terminal_tab(record, agent_cli));
    }
    let kind = deserialize_tab_kind(&record.kind)?;
    if kind == TabKind::Settings {
        return Some(settings_tab(record.id.clone()));
    }

    let saved_kind = record.file_kind.as_deref().and_then(deserialize_file_kind);
    // Until the source is verified, no save/autosave/watcher path may target it.
    // Recovery drafts remain editable; unknown source contents are non-text placeholders.
    let file_kind = if record.recovery.text.is_none()
        && (record.path.is_some() || record.file_kind.as_deref() == Some("whiteboard"))
    {
        Some(FileKind::Binary)
    } else {
        saved_kind
            .filter(|kind| record.recovery.text.is_none() || kind.supports_text_buffer())
            .or(Some(FileKind::Markdown))
    };
    let text = record.recovery.text.clone().unwrap_or_else(|| {
        if record.id == WELCOME_TAB_ID {
            default_document_text(ui_language)
        } else {
            String::new()
        }
    });
    let snapshot = snapshot_for_record(record, text);
    let is_dirty = record.recovery.text.is_some();
    let mut recovery = record.recovery.clone();
    if recovery.source_path.is_none() {
        recovery.source_path = record.path.clone();
    }
    let history = history_store::TabHistory::new(
        record.history.source_identity.clone(),
        record.history.undo_log_path.clone(),
    );
    let path = None;

    Some(OpenTab {
        id: record.id.clone(),
        title: record.title.clone(),
        kind,
        path,
        file_kind,
        editor_left_aligned: record.view.editor_left_aligned,
        reading_mode: record.view.reading_mode && !record.view.source_mode,
        source_mode: record.view.source_mode,
        source_word_wrap: record.view.source_word_wrap,
        snapshot,
        image_view: ImageViewState::default(),
        is_preview: record.is_preview,
        is_dirty,
        is_deleted: false,
        is_pinned: record.is_pinned,
        recovery,
        history,
    })
}

// Unit tests use a synchronous adapter; production restores only through fs_task.
#[cfg(test)]
fn restore_tab_from_record(record: &TabSessionRecord, ui_language: UiLanguage) -> Option<OpenTab> {
    let mut tab = restore_tab_metadata(record, ui_language)?;
    if tab.kind == TabKind::Editor {
        match load_restored_tab(record, ui_language) {
            Ok(loaded) => apply_restored_tab(&mut tab, loaded),
            Err(_) if record.recovery.text.is_some() => {}
            Err(_) => return None,
        }
    }
    Some(tab)
}

fn snapshot_for_record(record: &TabSessionRecord, text: String) -> EditorSnapshot {
    EditorSnapshot {
        cursor_offset: record.view.cursor_offset.min(text.len()),
        cursor_anchor: record.view.cursor_anchor.min(text.len()),
        scroll_offset: record.view.scroll_offset,
        extra_cursors: record
            .view
            .extra_cursors
            .iter()
            .map(|(offset, anchor)| ((*offset).min(text.len()), (*anchor).min(text.len())))
            .collect(),
        text,
    }
}

struct RestoredTabData {
    path: Option<PathBuf>,
    snapshot: EditorSnapshot,
    file_kind: FileKind,
    history: history_store::TabHistory,
    is_dirty: bool,
    recovery: TabRecoveryState,
}

fn load_restored_tab(
    record: &TabSessionRecord,
    ui_language: UiLanguage,
) -> io::Result<RestoredTabData> {
    // Old canvas sessions did not store their document in the text snapshot.
    // Without a source or recovery text, an empty buffer is not recovered data.
    if record.file_kind.as_deref() == Some("whiteboard")
        && record.path.is_none()
        && record.recovery.text.is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session tab has no recoverable contents",
        ));
    }
    let saved_kind = record.file_kind.as_deref().and_then(deserialize_file_kind);
    let bytes = record.path.as_deref().map(fs::read).transpose()?;
    let mut file_kind = bytes
        .as_deref()
        .zip(record.path.as_deref())
        .map(|(bytes, path)| classify_file_kind_for_bytes(path, bytes))
        .or(saved_kind.clone())
        .unwrap_or(FileKind::Markdown);
    let (path, snapshot, is_dirty) =
        resolve_snapshot(record, Some(&file_kind), ui_language, bytes.as_deref()).ok_or_else(
            || {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session tab has no recoverable contents",
                )
            },
        )?;
    if is_dirty && !file_kind.supports_text_buffer() {
        file_kind = saved_kind
            .filter(FileKind::supports_text_buffer)
            .unwrap_or(FileKind::Markdown);
    }
    let mut history = history_store::load_tab_history_from_path(
        &record.id,
        path.as_deref(),
        record.history.undo_log_path.as_deref(),
        &snapshot,
    );
    if let Some(identity) = record.history.source_identity.clone() {
        history.source_identity = Some(identity);
    }
    if !is_dirty && path.is_some() {
        history.saved_text = Some(snapshot.text.clone());
    }
    let recovery = if is_dirty {
        let mut recovery = record.recovery.clone();
        if recovery.source_path.is_none() {
            recovery.source_path = record.path.clone();
        }
        recovery
    } else {
        TabRecoveryState::default()
    };
    Ok(RestoredTabData {
        path,
        snapshot,
        file_kind,
        history,
        is_dirty,
        recovery,
    })
}

fn restore_can_apply(tab: &OpenTab, current_text: &str, initial_text: &str) -> bool {
    tab.path.is_none()
        && current_text == initial_text
        && tab.history.undo_stack.is_empty()
        && tab.history.redo_stack.is_empty()
}

fn apply_restored_tab(tab: &mut OpenTab, loaded: RestoredTabData) {
    tab.path = loaded.path;
    tab.snapshot = loaded.snapshot;
    tab.file_kind = Some(loaded.file_kind);
    tab.history = loaded.history;
    tab.is_dirty = loaded.is_dirty;
    tab.recovery = loaded.recovery;
}

fn resolve_snapshot(
    record: &TabSessionRecord,
    file_kind: Option<&FileKind>,
    ui_language: UiLanguage,
    bytes: Option<&[u8]>,
) -> Option<(Option<PathBuf>, EditorSnapshot, bool)> {
    let recovery_text = record.recovery.text.clone();
    let has_recovery_text = recovery_text.is_some();

    match record.path.as_ref() {
        Some(path) if bytes.is_some() => {
            let (mut snapshot, resolved_kind) = load_file_snapshot(path, bytes?);
            if resolved_kind.supports_text_buffer() {
                if let Some(text) = recovery_text {
                    if snapshot.text != text {
                        // A missing baseline is not proof that the source is unchanged.
                        let source_verified = record
                            .recovery
                            .source_content_hash
                            .as_ref()
                            .is_some_and(|expected| {
                                file_ops::content_hash(snapshot.text.as_bytes()) == *expected
                            });
                        snapshot.text = text;
                        snapshot.cursor_offset = record.view.cursor_offset.min(snapshot.text.len());
                        snapshot.cursor_anchor = record.view.cursor_anchor.min(snapshot.text.len());
                        snapshot.scroll_offset = record.view.scroll_offset;
                        snapshot.extra_cursors = record
                            .view
                            .extra_cursors
                            .iter()
                            .map(|(offset, anchor)| {
                                (
                                    (*offset).min(snapshot.text.len()),
                                    (*anchor).min(snapshot.text.len()),
                                )
                            })
                            .collect();
                        return Some((source_verified.then(|| path.clone()), snapshot, true));
                    }
                }
                snapshot.cursor_offset = record.view.cursor_offset.min(snapshot.text.len());
                snapshot.cursor_anchor = record.view.cursor_anchor.min(snapshot.text.len());
                snapshot.scroll_offset = record.view.scroll_offset;
                snapshot.extra_cursors = record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| {
                        (
                            (*offset).min(snapshot.text.len()),
                            (*anchor).min(snapshot.text.len()),
                        )
                    })
                    .collect();
                Some((Some(path.clone()), snapshot, false))
            } else if let Some(text) = recovery_text {
                // An external replacement with an image/binary must not discard a text draft.
                Some((None, snapshot_for_record(record, text), true))
            } else {
                snapshot.scroll_offset = record.view.scroll_offset;
                Some((Some(path.clone()), snapshot, false))
            }
        }
        Some(_) => recovery_text.map(|text| {
            let snapshot = EditorSnapshot {
                cursor_offset: record.view.cursor_offset.min(text.len()),
                cursor_anchor: record.view.cursor_anchor.min(text.len()),
                scroll_offset: record.view.scroll_offset,
                extra_cursors: record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| ((*offset).min(text.len()), (*anchor).min(text.len())))
                    .collect(),
                text,
            };
            (None, snapshot, true)
        }),
        None if record.id == super::WELCOME_TAB_ID
            && file_kind.is_some_and(FileKind::supports_text_buffer) =>
        {
            let text = recovery_text.unwrap_or_else(|| super::default_document_text(ui_language));
            let snapshot = EditorSnapshot {
                cursor_offset: record.view.cursor_offset.min(text.len()),
                cursor_anchor: record.view.cursor_anchor.min(text.len()),
                scroll_offset: record.view.scroll_offset,
                extra_cursors: record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| ((*offset).min(text.len()), (*anchor).min(text.len())))
                    .collect(),
                text,
            };
            Some((None, snapshot, has_recovery_text))
        }
        None if file_kind.is_some_and(FileKind::supports_text_buffer) => Some({
            let text = recovery_text.unwrap_or_default();
            let snapshot = EditorSnapshot {
                cursor_offset: record.view.cursor_offset.min(text.len()),
                cursor_anchor: record.view.cursor_anchor.min(text.len()),
                scroll_offset: record.view.scroll_offset,
                extra_cursors: record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| ((*offset).min(text.len()), (*anchor).min(text.len())))
                    .collect(),
                text,
            };
            (None, snapshot, has_recovery_text)
        }),
        None => None,
    }
}

fn load_file_snapshot(path: &Path, bytes: &[u8]) -> (EditorSnapshot, FileKind) {
    let file_kind = classify_file_kind_for_bytes(path, &bytes);
    let text = if file_kind.supports_text_buffer() {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        String::new()
    };
    let first_line_end = text.find('\n').unwrap_or(text.len());
    (
        EditorSnapshot {
            text,
            cursor_offset: first_line_end,
            cursor_anchor: first_line_end,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        },
        file_kind,
    )
}

fn serialize_tab_kind(kind: TabKind) -> String {
    match kind {
        TabKind::Editor => "editor",
        TabKind::Settings => "settings",
        TabKind::Terminal(_) => "terminal",
    }
    .to_string()
}

fn deserialize_tab_kind(value: &str) -> Option<TabKind> {
    match value {
        "editor" => Some(TabKind::Editor),
        "settings" => Some(TabKind::Settings),
        _ => None,
    }
}

#[cfg(test)]
#[test]
fn legacy_diff_tabs_do_not_restore_as_editable_documents() {
    assert_eq!(deserialize_tab_kind("diff"), None);
    assert_eq!(deserialize_tab_kind("editor"), Some(TabKind::Editor));
    assert_eq!(deserialize_tab_kind("settings"), Some(TabKind::Settings));
}

fn serialize_agent_cli(agent_cli: ChatAgentCli) -> String {
    match agent_cli {
        ChatAgentCli::Terminal => "terminal",
        ChatAgentCli::GithubCopilot => "github_copilot",
        ChatAgentCli::OpenAICodex => "openai_codex",
        ChatAgentCli::ClaudeCode => "claude_code",
        ChatAgentCli::GeminiCli => "gemini_cli",
    }
    .to_string()
}

fn deserialize_agent_cli(value: &str) -> Option<ChatAgentCli> {
    match value {
        "terminal" => Some(ChatAgentCli::Terminal),
        "github_copilot" => Some(ChatAgentCli::GithubCopilot),
        "openai_codex" => Some(ChatAgentCli::OpenAICodex),
        "claude_code" => Some(ChatAgentCli::ClaudeCode),
        "gemini_cli" => Some(ChatAgentCli::GeminiCli),
        _ => None,
    }
}

/// Rebuilds a terminal tab from its session record. The PTY session itself is
/// respawned lazily when the tab first renders.
fn terminal_tab(record: &TabSessionRecord, agent_cli: ChatAgentCli) -> OpenTab {
    OpenTab {
        id: record.id.clone(),
        title: record.title.clone(),
        kind: TabKind::Terminal(agent_cli),
        path: None,
        file_kind: None,
        editor_left_aligned: false,
        reading_mode: false,
        source_mode: false,
        source_word_wrap: true,
        snapshot: EditorSnapshot {
            text: String::new(),
            cursor_offset: 0,
            cursor_anchor: 0,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        },
        image_view: ImageViewState::default(),
        is_preview: false,
        is_dirty: false,
        is_deleted: false,
        is_pinned: record.is_pinned,
        recovery: TabRecoveryState::default(),
        history: history_store::TabHistory::default(),
    }
}

fn serialize_file_kind(kind: &FileKind) -> String {
    match kind {
        FileKind::Markdown => "markdown".to_string(),
        FileKind::Text => "text".to_string(),
        FileKind::Image => "image".to_string(),
        FileKind::Binary => "binary".to_string(),
        FileKind::Code { language } => format!("code:{language}"),
    }
}

fn deserialize_file_kind(value: &str) -> Option<FileKind> {
    match value {
        "markdown" => Some(FileKind::Markdown),
        "text" => Some(FileKind::Text),
        "image" => Some(FileKind::Image),
        "binary" => Some(FileKind::Binary),
        "whiteboard" => Some(FileKind::Text),
        _ => value.strip_prefix("code:").map(|language| FileKind::Code {
            language: language.to_string(),
        }),
    }
}

fn settings_tab(id: String) -> OpenTab {
    OpenTab {
        id,
        title: crate::i18n::translate(
            crate::i18n::detect_system_language(),
            crate::i18n::TextKey::SettingsTitle,
        )
        .to_string(),
        kind: TabKind::Settings,
        path: None,
        file_kind: None,
        editor_left_aligned: false,
        reading_mode: false,
        source_mode: false,
        source_word_wrap: true,
        snapshot: EditorSnapshot {
            text: String::new(),
            cursor_offset: 0,
            cursor_anchor: 0,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        },
        image_view: ImageViewState::default(),
        is_preview: false,
        is_dirty: false,
        is_deleted: false,
        is_pinned: false,
        recovery: TabRecoveryState::default(),
        history: history_store::TabHistory::default(),
    }
}

fn infer_next_tab_id(tabs: &[OpenTab], fallback: usize) -> usize {
    tabs.iter()
        .filter_map(|tab| tab.id.strip_prefix("tab-"))
        .filter_map(|suffix| suffix.parse::<usize>().ok())
        .max()
        .map(|suffix| suffix + 1)
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn bookmark_access_is_coalesced_until_success_or_an_explicit_retry() {
        let root = Path::new("/unavailable-share/workspace");
        let mut access = WorkspaceAccess {
            bookmark: "saved-bookmark".into(),
            loading: false,
            resolved: false,
            request_root: None,
        };
        assert_eq!(access.begin(root).as_deref(), Some("saved-bookmark"));
        for _ in 0..12 {
            assert!(access.begin(root).is_none());
        }
        assert_eq!(access.request_root.as_deref(), Some(root));
        access.loading = false;
        access.request_root = None;
        assert!(access.begin(root).is_some());
        access.loading = false;
        access.resolved = true;
        assert!(access.begin(root).is_none());
    }

    #[test]
    fn relocating_a_bookmark_keeps_unknown_source_metadata_and_recovery() {
        let mut record = text_tab_record(
            Some(PathBuf::from("/old-share/workspace/note.md")),
            Some("draft"),
        );
        record.recovery.source_content_hash = Some("saved-source-hash".into());
        record.history.undo_log_path = Some(PathBuf::from("/local-cache/history.undo.json"));
        record.history.source_identity = Some("/old-share/workspace/note.md".into());
        let original_view = record.view.clone();
        relocate_record(
            &mut record,
            Path::new("/old-share/workspace"),
            Path::new("/new-share/workspace"),
        );
        assert_eq!(
            record.path.as_deref(),
            Some(Path::new("/new-share/workspace/note.md"))
        );
        assert_eq!(record.recovery.source_path, record.path);
        assert_eq!(record.recovery.text.as_deref(), Some("draft"));
        assert_eq!(
            record.recovery.source_content_hash.as_deref(),
            Some("saved-source-hash")
        );
        assert_eq!(
            record.history.undo_log_path.as_deref(),
            Some(Path::new("/local-cache/history.undo.json"))
        );
        assert_eq!(
            record.history.source_identity.as_deref(),
            Some("/new-share/workspace/note.md")
        );
        assert_eq!(record.view, original_view);
        assert!(
            restore_tab_metadata(&record, UiLanguage::English)
                .unwrap()
                .path
                .is_none()
        );
    }

    #[test]
    fn restore_scopes_are_lexical_and_group_nested_tabs_under_the_workspace() {
        let roots = vec![PathBuf::from("/arbitrary/sshfs/project")];
        let first = restore_scope_for_path(Path::new("/arbitrary/sshfs/project/docs/a.md"), &roots);
        let second =
            restore_scope_for_path(Path::new("/arbitrary/sshfs/project/other/b.md"), &roots);
        assert_eq!(first, roots[0]);
        assert_eq!(first, second);
        fs_task::register_workspace_root(&roots[0]);
    }

    #[test]
    fn unavailable_source_metadata_preserves_the_record_without_a_write_target() {
        let mut record = text_tab_record(Some(PathBuf::from("/unavailable-share/note.md")), None);
        record.is_pinned = true;
        record.history = TabHistoryIndex {
            undo_log_path: Some(PathBuf::from("/unavailable-share/history.json")),
            source_identity: Some("original".into()),
            current_index: Some(9),
            entry_count: 20,
        };
        let tab = restore_tab_metadata(&record, UiLanguage::English).unwrap();
        assert!(tab.path.is_none());
        assert!(tab.history.saved_text.is_none());
        assert!(!tab.file_kind.as_ref().unwrap().supports_text_buffer());
        let pending = PendingTabRestore {
            record: record.clone(),
            initial_text: String::new(),
            loading: true,
        };
        assert_eq!(pending_tab_to_record(&tab, &pending), record);
    }

    #[test]
    fn unavailable_recovery_stays_editable_and_roundtrips_the_original_source() {
        let mut record = text_tab_record(
            Some(PathBuf::from("/unavailable-share/note.md")),
            Some("draft"),
        );
        record.recovery.source_content_hash = Some("original-source-hash".into());
        let mut tab = restore_tab_metadata(&record, UiLanguage::English).unwrap();
        assert!(tab.is_dirty);
        assert!(tab.path.is_none());
        assert!(tab.file_kind.as_ref().unwrap().supports_text_buffer());
        let pending = PendingTabRestore {
            record: record.clone(),
            initial_text: "draft".into(),
            loading: false,
        };
        assert_eq!(pending_tab_to_record(&tab, &pending), record);
        tab.snapshot.scroll_offset = 99.0;
        assert_eq!(
            pending_tab_to_record(&tab, &pending).view.scroll_offset,
            99.0
        );
        tab.snapshot.text = "edited while unavailable".into();
        // Debounced recovery sees a detached tab, but must not erase the original source identity.
        tab.recovery.source_path = None;
        tab.recovery.source_content_hash = None;
        let saved = pending_tab_to_record(&tab, &pending);
        assert_eq!(saved.path, record.path);
        assert_eq!(saved.history, record.history);
        assert_eq!(saved.recovery.source_path, record.recovery.source_path);
        assert_eq!(
            saved.recovery.source_content_hash,
            record.recovery.source_content_hash
        );
        assert_eq!(
            saved.recovery.text.as_deref(),
            Some("edited while unavailable")
        );
    }

    #[test]
    fn unavailable_tabs_and_empty_leaves_keep_split_layout_and_view_flags() {
        let record = text_tab_record(Some(PathBuf::from("/unavailable-share/note.md")), None);
        let layout = PaneLayoutRecord::Split {
            axis: "vertical".into(),
            flexes: vec![2.0, 1.0],
            children: vec![
                PaneLayoutRecord::Leaf {
                    tabs: vec![record.clone()],
                    active_tab_id: Some(record.id.clone()),
                },
                PaneLayoutRecord::Leaf {
                    tabs: Vec::new(),
                    active_tab_id: None,
                },
            ],
        };
        let mut panes = Vec::new();
        let built = build_pane_layout(UiLanguage::English, &layout, &mut panes).unwrap();
        assert_eq!(built.leaf_ids(), vec![0, 1]);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].state.active_tab_id, Some(record.id));
        assert!(panes[0].state.tabs[0].reading_mode);
        assert!(panes[0].state.tabs[0].editor_left_aligned);
        assert!(panes[1].state.tabs.is_empty());
        match built {
            PaneLayout::Split { axis, flexes, .. } => {
                assert_eq!(axis, SplitAxis::Vertical);
                assert_eq!(flexes, vec![2.0, 1.0]);
            }
            _ => panic!("split was collapsed"),
        }
    }

    #[test]
    fn source_mode_restore_and_roundtrip_prefer_source_over_reading() {
        for reading_mode in [false, true] {
            let mut record = text_tab_record(None, Some("draft"));
            record.view.reading_mode = reading_mode;
            record.view.source_mode = true;

            let tab = restore_tab_from_record(&record, UiLanguage::English).unwrap();
            assert!(tab.source_mode);
            assert!(!tab.reading_mode);

            let encoded = serde_yaml::to_string(&tab_to_record(&tab)).unwrap();
            let decoded = serde_yaml::from_str(&encoded).unwrap();
            let restored = restore_tab_from_record(&decoded, UiLanguage::English).unwrap();
            assert!(restored.source_mode);
            assert!(!restored.reading_mode);
        }
    }

    #[test]
    fn source_word_wrap_roundtrip_is_independent_of_mode_alignment_and_file_kind() {
        for source_mode in [false, true] {
            for source_word_wrap in [false, true] {
                for editor_left_aligned in [false, true] {
                    let mut record = text_tab_record(None, Some("# draft"));
                    record.view.source_mode = source_mode;
                    record.view.source_word_wrap = source_word_wrap;
                    record.view.editor_left_aligned = editor_left_aligned;

                    let tab = restore_tab_from_record(&record, UiLanguage::English).unwrap();
                    assert_eq!(tab.source_word_wrap, source_word_wrap);
                    assert_eq!(tab.editor_left_aligned, editor_left_aligned);
                    assert_eq!(tab.file_kind, Some(FileKind::Markdown));
                    let saved = tab_to_record(&tab);
                    assert_eq!(saved.file_kind.as_deref(), Some("markdown"));
                    let encoded = serde_yaml::to_string(&saved).unwrap();
                    let decoded = serde_yaml::from_str(&encoded).unwrap();
                    let restored = restore_tab_from_record(&decoded, UiLanguage::English).unwrap();
                    assert_eq!(restored.source_mode, source_mode);
                    assert_eq!(restored.source_word_wrap, source_word_wrap);
                    assert_eq!(restored.editor_left_aligned, editor_left_aligned);
                    assert_eq!(restored.file_kind, Some(FileKind::Markdown));
                }
            }
        }
    }

    #[test]
    fn legacy_session_restore_keeps_reading_mode_without_enabling_source() {
        let record: TabSessionRecord = serde_yaml::from_str(
            "id: tab-legacy\nkind: editor\nview:\n  reading_mode: true\nrecovery:\n  text: draft\n",
        )
        .unwrap();

        let tab = restore_tab_from_record(&record, UiLanguage::English).unwrap();
        assert!(tab.reading_mode);
        assert!(!tab.source_mode);
        assert!(!tab_to_record(&tab).view.source_mode);
        assert!(tab.source_word_wrap);
        assert!(tab_to_record(&tab).view.source_word_wrap);
    }

    #[test]
    fn pending_restore_saves_source_word_wrap_without_changing_alignment_or_metadata() {
        let record = text_tab_record(Some(PathBuf::from("/unavailable-share/note.md")), None);
        let mut tab = restore_tab_metadata(&record, UiLanguage::English).unwrap();
        let file_kind = tab.file_kind.clone();
        let pending = PendingTabRestore {
            record: record.clone(),
            initial_text: String::new(),
            loading: true,
        };
        for source_mode in [true, false] {
            tab.source_mode = source_mode;
            tab.reading_mode = false;
            for source_word_wrap in [false, true] {
                tab.source_word_wrap = source_word_wrap;
                let saved = pending_tab_to_record(&tab, &pending);
                assert_eq!(saved.view.source_word_wrap, source_word_wrap);
                assert_eq!(
                    saved.view.editor_left_aligned,
                    record.view.editor_left_aligned
                );
                assert_eq!(saved.path, record.path);
                assert_eq!(saved.file_kind, record.file_kind);
                assert_eq!(saved.recovery, record.recovery);
                assert_eq!(saved.history, record.history);
                assert_eq!(saved.view.cursor_offset, record.view.cursor_offset);
                let restored = restore_tab_metadata(&saved, UiLanguage::English).unwrap();
                assert_eq!(restored.source_word_wrap, source_word_wrap);
                assert_eq!(restored.source_mode, source_mode);
                assert_eq!(restored.file_kind, file_kind);
            }
        }
    }

    #[test]
    fn pending_restore_saves_source_mode_changes_without_losing_source_metadata() {
        let record = text_tab_record(Some(PathBuf::from("/unavailable-share/note.md")), None);
        let mut tab = restore_tab_metadata(&record, UiLanguage::English).unwrap();
        let pending = PendingTabRestore {
            record: record.clone(),
            initial_text: String::new(),
            loading: true,
        };
        tab.source_mode = true;
        // Even invalid live flags must never persist both modes as enabled.
        tab.reading_mode = true;
        let saved = pending_tab_to_record(&tab, &pending);
        assert!(saved.view.source_mode);
        assert!(!saved.view.reading_mode);
        assert_eq!(saved.path, record.path);
        assert_eq!(saved.recovery, record.recovery);
        assert_eq!(saved.history, record.history);
        assert_eq!(saved.view.cursor_offset, record.view.cursor_offset);
        let restored = restore_tab_metadata(&saved, UiLanguage::English).unwrap();
        assert!(restored.source_mode);
        assert!(!restored.reading_mode);

        tab.source_mode = false;
        tab.reading_mode = false;
        let saved = pending_tab_to_record(&tab, &pending);
        assert!(!saved.view.source_mode);
        assert!(!saved.view.reading_mode);
    }

    #[test]
    fn split_pane_restore_keeps_source_word_wrap_and_modes_independent() {
        let mut source = text_tab_record(None, Some("source pane"));
        source.id = "tab-source".into();
        source.view.source_mode = true;
        source.view.source_word_wrap = false;
        source.view.editor_left_aligned = false;
        let mut reading = text_tab_record(None, Some("reading pane"));
        reading.id = "tab-reading".into();
        let layout = PaneLayoutRecord::Split {
            axis: "horizontal".into(),
            flexes: vec![1.0, 2.0],
            children: vec![
                PaneLayoutRecord::Leaf {
                    active_tab_id: Some(source.id.clone()),
                    tabs: vec![source],
                },
                PaneLayoutRecord::Leaf {
                    active_tab_id: Some(reading.id.clone()),
                    tabs: vec![reading],
                },
            ],
        };
        let encoded = serde_yaml::to_string(&layout).unwrap();
        let decoded = serde_yaml::from_str(&encoded).unwrap();
        let mut panes = Vec::new();
        let built = build_pane_layout(UiLanguage::English, &decoded, &mut panes).unwrap();

        assert_eq!(built.leaf_ids(), vec![0, 1]);
        assert_eq!(panes[0].state.active_tab_id.as_deref(), Some("tab-source"));
        assert!(panes[0].state.tabs[0].source_mode);
        assert!(!panes[0].state.tabs[0].reading_mode);
        assert!(!panes[0].state.tabs[0].source_word_wrap);
        assert!(!panes[0].state.tabs[0].editor_left_aligned);
        assert_eq!(panes[1].state.active_tab_id.as_deref(), Some("tab-reading"));
        assert!(!panes[1].state.tabs[0].source_mode);
        assert!(panes[1].state.tabs[0].reading_mode);
        assert!(panes[1].state.tabs[0].source_word_wrap);
        assert!(panes[1].state.tabs[0].editor_left_aligned);
        assert!(tab_to_record(&panes[0].state.tabs[0]).view.source_mode);
        assert!(!tab_to_record(&panes[1].state.tabs[0]).view.source_mode);
        assert!(!tab_to_record(&panes[0].state.tabs[0]).view.source_word_wrap);
        assert!(tab_to_record(&panes[1].state.tabs[0]).view.source_word_wrap);
    }

    #[test]
    fn hydration_never_replaces_an_edit_or_a_save_as() {
        let record = text_tab_record(
            Some(PathBuf::from("/unavailable-share/note.md")),
            Some("draft"),
        );
        let mut tab = restore_tab_metadata(&record, UiLanguage::English).unwrap();
        assert!(restore_can_apply(&tab, "draft", "draft"));
        assert!(!restore_can_apply(&tab, "new work", "draft"));
        tab.path = Some(PathBuf::from("/local/saved-as.md"));
        assert!(!restore_can_apply(&tab, "draft", "draft"));
    }

    #[test]
    fn hydration_preserves_new_undo_or_redo_work_even_if_text_matches() {
        let record = text_tab_record(
            Some(PathBuf::from("/unavailable-share/note.md")),
            Some("draft"),
        );
        let mut tab = restore_tab_metadata(&record, UiLanguage::English).unwrap();
        tab.history.push_undo_snapshot(snapshot("draft", 5));
        assert!(!restore_can_apply(&tab, "draft", "draft"));
    }

    #[test]
    fn a_source_replaced_by_binary_content_never_discards_a_recovery_draft() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("replaced.bin");
        fs::write(&path, [0, 1, 2, 0, 3]).unwrap();
        let mut record = text_tab_record(Some(path.clone()), Some("unsaved draft"));
        record.recovery.source_path = None;
        let loaded = load_restored_tab(&record, UiLanguage::English).unwrap();
        assert!(loaded.path.is_none());
        assert!(loaded.is_dirty);
        assert_eq!(loaded.snapshot.text, "unsaved draft");
        assert!(loaded.file_kind.supports_text_buffer());
        assert_eq!(loaded.recovery.source_path, Some(path));
    }

    #[test]
    fn legacy_whiteboard_restore_keeps_exact_text_and_roundtrips_as_text() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("board.moeka");
        let text = "{ \"elements\": [], \"unknown\": \"保留\" }\r\n";
        fs::write(&path, text).unwrap();
        let mut record = text_tab_record(Some(path.clone()), None);
        record.file_kind = Some("whiteboard".into());
        record.is_pinned = true;
        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();
        assert_eq!(restored.file_kind, Some(FileKind::Text));
        assert_eq!(restored.snapshot.text, text);
        assert_eq!(restored.path, Some(path.clone()));
        assert!(!restored.is_dirty);
        assert!(restored.is_pinned);
        assert_eq!(restored.id, record.id);
        assert_eq!(restored.history.saved_text.as_deref(), Some(text));
        let saved = tab_to_record(&restored);
        assert_eq!(saved.file_kind.as_deref(), Some("text"));
        let encoded = serde_yaml::to_string(&saved).unwrap();
        let decoded = serde_yaml::from_str(&encoded).unwrap();
        let reopened = restore_tab_from_record(&decoded, UiLanguage::English).unwrap();
        assert_eq!(reopened.snapshot.text, text);
        assert_eq!(reopened.file_kind, Some(FileKind::Text));
        assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
    }

    #[test]
    fn legacy_whiteboard_recovery_requires_a_verified_source_hash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("board.moeka");
        let disk = "{ \"elements\": [] }";
        let draft = "unsaved raw text, not necessarily JSON";
        fs::write(&path, disk).unwrap();
        for hash in [
            None,
            Some(file_ops::content_hash(b"old source")),
            Some(file_ops::content_hash(disk.as_bytes())),
        ] {
            let verified =
                hash.as_deref() == Some(file_ops::content_hash(disk.as_bytes()).as_str());
            let mut record = text_tab_record(Some(path.clone()), Some(draft));
            record.file_kind = Some("whiteboard".into());
            record.recovery.source_content_hash = hash;
            let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();
            assert_eq!(restored.file_kind, Some(FileKind::Text));
            assert_eq!(restored.snapshot.text, draft);
            assert_eq!(restored.path, verified.then(|| path.clone()));
            assert!(restored.is_dirty);
            assert_eq!(restored.recovery, record.recovery);
            let saved = tab_to_record(&restored);
            assert_eq!(saved.file_kind.as_deref(), Some("text"));
            let reopened = restore_tab_from_record(&saved, UiLanguage::English).unwrap();
            assert_eq!(reopened.snapshot.text, draft);
            assert_eq!(reopened.path, restored.path);
            assert!(reopened.is_dirty);
            assert_eq!(fs::read_to_string(&path).unwrap(), disk);
        }
    }

    #[test]
    fn legacy_whiteboard_unavailable_sources_preserve_recovery_and_pending_identity() {
        let dir = tempdir().unwrap();
        // Reading a directory fails on every supported platform, even as root.
        for path in [dir.path().join("missing.moeka"), dir.path().to_path_buf()] {
            for recovery in [None, Some("unsaved canvas JSON")] {
                let mut record = text_tab_record(Some(path.clone()), recovery);
                record.file_kind = Some("whiteboard".into());
                record.is_pinned = true;
                let metadata = restore_tab_metadata(&record, UiLanguage::English).unwrap();
                assert!(metadata.path.is_none());
                assert_eq!(metadata.is_dirty, recovery.is_some());
                assert_eq!(
                    metadata.file_kind.as_ref().unwrap().supports_text_buffer(),
                    recovery.is_some()
                );
                let pending = PendingTabRestore {
                    record: record.clone(),
                    initial_text: recovery.unwrap_or_default().into(),
                    loading: false,
                };
                assert_eq!(pending_tab_to_record(&metadata, &pending), record);
                assert!(load_restored_tab(&record, UiLanguage::English).is_err());
                let restored = restore_tab_from_record(&record, UiLanguage::English);
                if let Some(draft) = recovery {
                    let restored = restored.unwrap();
                    assert_eq!(restored.snapshot.text, draft);
                    assert_eq!(restored.recovery.source_path.as_ref(), Some(&path));
                    assert_eq!(restored.file_kind, Some(FileKind::Text));
                } else {
                    assert!(restored.is_none());
                }
            }
        }
    }

    #[test]
    fn legacy_whiteboard_without_a_source_needs_actual_recovery_text() {
        for recovery in [None, Some(""), Some("{\"elements\":[\"draft\"]}")] {
            let mut record = text_tab_record(None, recovery);
            record.file_kind = Some("whiteboard".into());
            let restored = restore_tab_from_record(&record, UiLanguage::English);
            if let Some(text) = recovery {
                let restored = restored.unwrap();
                assert_eq!(restored.snapshot.text, text);
                assert_eq!(restored.file_kind, Some(FileKind::Text));
                assert!(restored.path.is_none());
                assert!(restored.is_dirty);
                let saved = tab_to_record(&restored);
                assert_eq!(saved.file_kind.as_deref(), Some("text"));
                assert_eq!(saved.recovery.text.as_deref(), Some(text));
            } else {
                assert!(restored.is_none());
                assert_eq!(
                    restore_tab_metadata(&record, UiLanguage::English)
                        .unwrap()
                        .file_kind,
                    Some(FileKind::Binary)
                );
            }
        }
    }

    #[test]
    fn legacy_whiteboard_binary_sources_are_not_empty_text_documents() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("board.moeka");
        let bytes = [0, 0xff, 1, 0xfe];
        fs::write(&path, bytes).unwrap();
        for recovery in [None, Some("unsaved text")] {
            let mut record = text_tab_record(Some(path.clone()), recovery);
            record.file_kind = Some("whiteboard".into());
            let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();
            if let Some(text) = recovery {
                assert_eq!(restored.snapshot.text, text);
                assert_eq!(restored.file_kind, Some(FileKind::Text));
                assert!(restored.path.is_none());
                assert!(restored.is_dirty);
            } else {
                assert_eq!(restored.file_kind, Some(FileKind::Binary));
                assert_eq!(restored.path, Some(path.clone()));
                assert!(!restored.is_dirty);
            }
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn legacy_whiteboard_split_panes_preserve_tab_order_and_view_state() {
        let mut first = text_tab_record(None, Some("first draft"));
        first.file_kind = Some("whiteboard".into());
        first.is_pinned = true;
        let mut second = first.clone();
        second.id = "tab-2".into();
        second.is_pinned = false;
        second.recovery.text = Some("second draft".into());
        let mut third = second.clone();
        third.id = "tab-3".into();
        let layout = PaneLayoutRecord::Split {
            axis: "horizontal".into(),
            flexes: vec![1.0, 2.0],
            children: vec![
                PaneLayoutRecord::Leaf {
                    active_tab_id: Some(second.id.clone()),
                    tabs: vec![first.clone(), second.clone()],
                },
                PaneLayoutRecord::Leaf {
                    active_tab_id: Some(third.id.clone()),
                    tabs: vec![third.clone()],
                },
            ],
        };
        let mut panes = Vec::new();
        let built = build_pane_layout(UiLanguage::English, &layout, &mut panes).unwrap();
        assert_eq!(built.leaf_ids(), vec![0, 1]);
        assert_eq!(panes[0].state.active_tab_id.as_deref(), Some("tab-2"));
        assert_eq!(panes[1].state.active_tab_id.as_deref(), Some("tab-3"));
        for (tab, record) in panes
            .iter()
            .flat_map(|pane| &pane.state.tabs)
            .zip([first, second, third])
        {
            assert_eq!(tab.id, record.id);
            assert_eq!(tab.is_pinned, record.is_pinned);
            assert!(tab.is_dirty);
            assert_eq!(tab.file_kind, Some(FileKind::Text));
            assert_eq!(tab.snapshot.text, record.recovery.text.clone().unwrap());
            assert_eq!(tab.snapshot.scroll_offset, record.view.scroll_offset);
            assert_eq!(tab.recovery, record.recovery);
        }
    }

    #[test]
    fn edited_welcome_tab_is_not_discarded_as_a_placeholder() {
        let mut tab = super::super::builtin_welcome_tab(UiLanguage::English);
        assert!(is_placeholder_welcome_tab(&tab));
        tab.is_dirty = true;
        tab.recovery.text = Some("unsaved welcome edit".to_string());
        assert!(!is_placeholder_welcome_tab(&tab));
    }

    #[test]
    fn session_writer_flushes_only_the_latest_pending_snapshot() {
        let (sender, receiver) = mpsc::channel();
        let (ack_sender, ack_receiver) = mpsc::channel();
        let mut first = config_store::SessionsFile::default();
        first.sessions.active_tab_id = Some("old".to_string());
        let mut latest = first.clone();
        latest.sessions.active_tab_id = Some("latest".to_string());
        sender.send(SessionWrite::Save(first)).unwrap();
        sender.send(SessionWrite::Save(latest)).unwrap();
        sender.send(SessionWrite::Flush(ack_sender)).unwrap();
        drop(sender);

        let mut written = Vec::new();
        run_session_writer(receiver, |config| {
            written.push(config.sessions.active_tab_id.clone())
        });
        ack_receiver.recv().unwrap();
        assert_eq!(written, vec![Some("latest".to_string())]);
    }

    fn snapshot(text: &str, cursor_offset: usize) -> EditorSnapshot {
        EditorSnapshot {
            text: text.to_string(),
            cursor_offset,
            cursor_anchor: cursor_offset,
            scroll_offset: cursor_offset as f32,
            extra_cursors: Vec::new(),
        }
    }

    fn text_tab_record(
        source_path: Option<PathBuf>,
        recovery_text: Option<&str>,
    ) -> TabSessionRecord {
        TabSessionRecord {
            id: "tab-1".to_string(),
            kind: "editor".to_string(),
            title: "note.md".to_string(),
            path: source_path.clone(),
            file_kind: Some("markdown".to_string()),
            agent_cli: None,
            is_preview: false,
            is_pinned: false,
            view: TabViewState {
                cursor_offset: 100,
                cursor_anchor: 50,
                scroll_offset: 42.0,
                editor_left_aligned: true,
                reading_mode: true,
                source_mode: false,
                source_word_wrap: true,
                extra_cursors: vec![(80, 70)],
            },
            recovery: TabRecoveryState {
                text: recovery_text.map(str::to_string),
                source_path,
                source_content_hash: None,
            },
            history: TabHistoryIndex::default(),
        }
    }

    #[test]
    fn file_kind_roundtrip_preserves_code_language() {
        let kind = FileKind::Code {
            language: "rust".to_string(),
        };
        let encoded = serialize_file_kind(&kind);
        let decoded = deserialize_file_kind(&encoded).unwrap();
        assert_eq!(decoded, kind);
    }

    #[test]
    fn terminal_tab_roundtrip_preserves_shell_and_pin_state() {
        let record = TabSessionRecord {
            id: "tab-1".to_string(),
            kind: "terminal".to_string(),
            title: ChatAgentCli::Terminal.label().to_string(),
            path: None,
            file_kind: None,
            agent_cli: Some(serialize_agent_cli(ChatAgentCli::Terminal)),
            is_preview: false,
            is_pinned: true,
            view: TabViewState::default(),
            recovery: TabRecoveryState::default(),
            history: TabHistoryIndex::default(),
        };

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();
        assert_eq!(restored.kind, TabKind::Terminal(ChatAgentCli::Terminal));
        assert!(restored.is_pinned);
        let encoded = tab_to_record(&restored);
        assert_eq!(encoded.kind, "terminal");
        assert_eq!(encoded.agent_cli, record.agent_cli);
    }

    #[test]
    fn legacy_agent_terminal_tabs_are_not_restored() {
        for agent in [
            ChatAgentCli::GithubCopilot,
            ChatAgentCli::OpenAICodex,
            ChatAgentCli::ClaudeCode,
            ChatAgentCli::GeminiCli,
        ] {
            let record = TabSessionRecord {
                id: "legacy-agent".to_string(),
                kind: "terminal".to_string(),
                title: agent.label().to_string(),
                path: None,
                file_kind: None,
                agent_cli: Some(serialize_agent_cli(agent)),
                is_preview: false,
                is_pinned: false,
                view: TabViewState::default(),
                recovery: TabRecoveryState::default(),
                history: TabHistoryIndex::default(),
            };
            assert!(restore_tab_from_record(&record, UiLanguage::English).is_none());
        }
    }

    #[test]
    fn infer_next_tab_id_uses_largest_existing_numeric_suffix() {
        let tabs = vec![
            settings_tab("settings-1".to_string()),
            OpenTab {
                id: "tab-9".to_string(),
                title: "A".to_string(),
                kind: TabKind::Editor,
                path: None,
                file_kind: Some(FileKind::Markdown),
                editor_left_aligned: false,
                reading_mode: false,
                source_mode: false,
                source_word_wrap: true,
                snapshot: EditorSnapshot {
                    text: String::new(),
                    cursor_offset: 0,
                    cursor_anchor: 0,
                    scroll_offset: 0.0,
                    extra_cursors: Vec::new(),
                },
                image_view: ImageViewState::default(),
                is_preview: false,
                is_dirty: false,
                is_deleted: false,
                is_pinned: false,
                recovery: TabRecoveryState::default(),
                history: history_store::TabHistory::default(),
            },
        ];

        assert_eq!(infer_next_tab_id(&tabs, 2), 10);
    }

    #[test]
    fn restore_tab_from_record_uses_explicit_history_path() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "current").unwrap();

        let custom_journal_path = dir.path().join("custom.undo.json");
        let mut history = history_store::TabHistory::new(
            Some(source_path.to_string_lossy().into_owned()),
            Some(custom_journal_path.clone()),
        );
        history.push_undo_snapshot(snapshot("before", 6));
        let current = snapshot("current", 7);
        history_store::persist_tab_history("tab-1", Some(&source_path), &mut history, &current)
            .unwrap();

        let record = TabSessionRecord {
            id: "tab-1".to_string(),
            kind: "editor".to_string(),
            title: "note.md".to_string(),
            path: Some(source_path.clone()),
            file_kind: Some("markdown".to_string()),
            agent_cli: None,
            is_preview: false,
            is_pinned: false,
            view: TabViewState {
                cursor_offset: 7,
                cursor_anchor: 7,
                scroll_offset: 7.0,
                editor_left_aligned: false,
                reading_mode: false,
                source_mode: false,
                source_word_wrap: true,
                extra_cursors: Vec::new(),
            },
            recovery: TabRecoveryState::default(),
            history: TabHistoryIndex {
                undo_log_path: Some(custom_journal_path.clone()),
                source_identity: Some(source_path.to_string_lossy().into_owned()),
                current_index: Some(1),
                entry_count: 2,
            },
        };

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.history.journal_path, Some(custom_journal_path));
        assert_eq!(
            restored.history.undo_stack,
            vec![history_store::HistoryEntry::Full(snapshot("before", 6))]
        );
        assert!(restored.history.redo_stack.is_empty());
    }

    #[test]
    fn restores_unsaved_contents_over_saved_file() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "saved").unwrap();
        let mut record = text_tab_record(Some(source_path.clone()), Some("recovered draft"));
        record.recovery.source_content_hash = Some(file_ops::content_hash(b"saved"));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, Some(source_path));
        assert_eq!(restored.snapshot.text, "recovered draft");
        assert_eq!(restored.snapshot.cursor_offset, "recovered draft".len());
        assert_eq!(restored.snapshot.cursor_anchor, "recovered draft".len());
        assert_eq!(
            restored.snapshot.extra_cursors,
            vec![("recovered draft".len(), "recovered draft".len())]
        );
        assert!(restored.is_dirty);
        assert_eq!(restored.recovery.text.as_deref(), Some("recovered draft"));
    }

    #[test]
    fn restores_inline_recovery_without_a_workspace_temp_file() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "saved").unwrap();
        let mut record = text_tab_record(Some(source_path.clone()), Some("inline draft"));
        record.recovery.source_content_hash = Some(file_ops::content_hash(b"saved"));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, Some(source_path));
        assert_eq!(restored.snapshot.text, "inline draft");
        assert!(restored.is_dirty);
        assert_eq!(restored.recovery.text.as_deref(), Some("inline draft"));
    }

    #[test]
    fn recovers_unsaved_contents_when_source_file_was_deleted() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("deleted.md");
        let record = text_tab_record(Some(source_path.clone()), Some("unsaved work"));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, None);
        assert_eq!(restored.snapshot.text, "unsaved work");
        assert!(restored.is_dirty);
        assert_eq!(restored.recovery.source_path, Some(source_path));
        assert_eq!(restored.recovery.text.as_deref(), Some("unsaved work"));
    }

    #[test]
    fn restores_multiple_untitled_tabs_from_session_contents() {
        for index in 0..3 {
            let text = format!("unsaved document {index}");
            let mut record = text_tab_record(None, Some(&text));
            record.id = format!("tab-{index}");

            let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

            assert!(restored.path.is_none());
            assert_eq!(restored.snapshot.text, text);
            assert!(restored.is_dirty);
            assert_eq!(restored.recovery.text.as_deref(), Some(text.as_str()));
        }
    }

    #[test]
    fn falls_back_to_source_without_unsaved_contents() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "saved contents").unwrap();
        let record = text_tab_record(Some(source_path.clone()), None);

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, Some(source_path));
        assert_eq!(restored.snapshot.text, "saved contents");
        assert!(!restored.is_dirty);
        assert_eq!(restored.recovery, TabRecoveryState::default());
    }

    #[test]
    fn matching_recovery_content_restores_clean_tab() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "same contents").unwrap();
        let record = text_tab_record(Some(source_path), Some("same contents"));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert!(!restored.is_dirty);
        assert_eq!(restored.recovery, TabRecoveryState::default());
    }

    #[test]
    fn recovery_without_a_source_baseline_never_gets_an_overwrite_path() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "unverified source").unwrap();
        let record = text_tab_record(Some(source_path.clone()), Some("unsaved draft"));

        let loaded = load_restored_tab(&record, UiLanguage::English).unwrap();

        assert!(loaded.path.is_none());
        assert!(loaded.is_dirty);
        assert!(loaded.history.saved_text.is_none());
        assert_eq!(loaded.snapshot.text, "unsaved draft");
        assert_eq!(loaded.recovery.source_path.as_ref(), Some(&source_path));
        assert_eq!(
            fs::read_to_string(source_path).unwrap(),
            "unverified source"
        );
    }

    #[test]
    fn external_source_change_restores_draft_without_overwrite_path() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "external edit").unwrap();
        let mut record = text_tab_record(Some(source_path.clone()), Some("unsaved draft"));
        record.recovery.source_content_hash = Some(file_ops::content_hash(b"original"));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, None);
        assert_eq!(restored.recovery.source_path, Some(source_path.clone()));
        assert_eq!(restored.snapshot.text, "unsaved draft");
        assert_eq!(fs::read_to_string(source_path).unwrap(), "external edit");
    }

    #[test]
    fn missing_source_without_recovery_keeps_a_safe_metadata_tab() {
        let dir = tempdir().unwrap();
        let record = text_tab_record(Some(dir.path().join("missing.md")), None);
        let tab = restore_tab_metadata(&record, UiLanguage::English).unwrap();
        assert!(tab.path.is_none());
        assert_eq!(tab.title, record.title);
        assert!(load_restored_tab(&record, UiLanguage::English).is_err());
    }

    #[test]
    fn restores_multiple_independent_recovery_records() {
        let dir = tempdir().unwrap();
        let records = (0..8)
            .map(|index| {
                let source = dir.path().join(format!("note-{index}.md"));
                fs::write(&source, format!("saved {index}")).unwrap();
                let draft = format!("draft {index}");
                let mut record = text_tab_record(Some(source), Some(&draft));
                record.id = format!("tab-{index}");
                record
            })
            .collect::<Vec<_>>();

        let restored = records
            .iter()
            .map(|record| restore_tab_from_record(record, UiLanguage::English).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(restored.len(), 8);
        for (index, tab) in restored.iter().enumerate() {
            assert_eq!(tab.snapshot.text, format!("draft {index}"));
            assert!(tab.is_dirty);
        }
    }
}
