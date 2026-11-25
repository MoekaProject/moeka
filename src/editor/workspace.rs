use super::*;
use gpui::px;

pub(crate) struct WorkspaceClose {
    root: PathBuf,
    remote: Option<RemoteWorkspaceClose>,
    approved: HashMap<String, String>,
}

struct RemoteWorkspaceClose {
    generation: Option<u64>,
    destination: String,
    root: moeka_fs::path::ResourcePath,
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::AppContext as _;

    #[gpui::test]
    fn each_workspace_view_keeps_its_own_tabs_and_tab_activation_never_switches_views(
        cx: &mut gpui::TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let editor = cx.update(|cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("# Local"), false))
        });
        editor.update(cx, |editor, cx| {
            editor.workspaces.push(directory.path().to_path_buf());
            editor.sidebar_root = directory.path().to_path_buf();
            editor.create_remote_scratch_tab(cx);
            let local_tab = editor.active_tab_id.clone().unwrap();
            let key = PathBuf::from(format!("{REMOTE_VIEW_PREFIX}host\u{0}/work"));
            editor.show_view(Some(key.clone()), cx);
            assert!(editor.remote_workspace_active);
            assert!(
                editor.tabs.is_empty(),
                "a new view starts without the other's tabs"
            );
            editor.create_remote_scratch_tab(cx);
            let remote_tab = editor.active_tab_id.clone().unwrap();
            assert_ne!(remote_tab, local_tab);
            assert!(editor.all_tabs().any(|tab| tab.id == local_tab));
            editor.show_view(None, cx);
            assert!(!editor.remote_workspace_active);
            assert_eq!(editor.active_tab_id.as_deref(), Some(local_tab.as_str()));
            assert!(editor.tabs.iter().all(|tab| tab.id != remote_tab));
            editor.show_view(Some(key), cx);
            assert_eq!(editor.active_tab_id.as_deref(), Some(remote_tab.as_str()));
            editor.activate_tab_by_index(0, cx);
            assert!(
                editor.remote_workspace_active,
                "activating a tab keeps the view"
            );
        });
    }

    #[gpui::test]
    fn workspace_adopts_unscoped_tabs_when_first_root_opens(cx: &mut gpui::TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let editor = cx.update(|cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("# Keep"), false))
        });
        editor.update(cx, |editor, cx| {
            let tab = editor.active_tab_id.clone();
            editor.workspaces.push(directory.path().to_path_buf());
            editor.switch_workspace(directory.path().to_path_buf(), cx);
            assert_eq!(editor.active_tab_id, tab);
            assert_eq!(editor.buffer.text(), "# Keep");
            assert!(editor.workspace_states.is_empty());
            assert_eq!(editor.panes.len(), 1);
        });
    }

    #[gpui::test]
    fn workspaces_keep_live_buffers_panes_and_background_identity(cx: &mut gpui::TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let a = directory.path().join("a");
        let b = directory.path().join("b");
        moeka_fs::local::native::create_dir_all(&a).unwrap();
        moeka_fs::local::native::create_dir_all(&b).unwrap();
        let editor = cx.update(|cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("# A"), false))
        });
        editor.update(cx, |editor, cx| {
            editor.sidebar_root = a.clone();
            editor.workspaces = vec![a.clone(), b.clone()];
            editor.cursor.offset = 2;
            editor.cursor.anchor = 1;
            editor.tabs[0].is_dirty = true;
            let outside_path = b.join("shared.md");
            editor.tabs[0].path = Some(outside_path.clone());
            editor.current_file_path = Some(outside_path.clone());
            let a_pane = editor.focused_pane_id;
            let a_tab = editor.active_tab_id.clone();
            editor.switch_workspace(b.clone(), cx);
            let b_pane = editor.focused_pane_id;
            assert_ne!(a_pane, b_pane);
            assert!(editor.tabs.is_empty());
            assert_eq!(editor.buffer.text(), "");
            assert_eq!(editor.all_tabs().count(), 1);
            editor
                .with_pane_if_alive(a_pane, |editor| {
                    assert_eq!(editor.sidebar_root, a);
                    assert_eq!(editor.buffer.text(), "# A");
                    assert_eq!(editor.active_tab_id, a_tab);
                    editor.buffer.insert(editor.buffer.len(), " changed");
                })
                .unwrap();
            assert_eq!(editor.sidebar_root, b);
            assert_eq!(editor.buffer.text(), "");
            editor.switch_workspace(a.clone(), cx);
            assert_eq!(editor.focused_pane_id, a_pane);
            assert_eq!(editor.buffer.text(), "# A changed");
            assert_eq!(editor.cursor.offset, 2);
            assert_eq!(editor.cursor.anchor, 1);
            assert!(editor.tabs[0].is_dirty);
            assert_eq!(editor.workspace_for_pane(b_pane), Some(b.clone()));
            editor.focus_pane(b_pane, cx);
            assert_eq!(editor.sidebar_root, b);
            assert_eq!(editor.visible_pane_ids(), vec![b_pane]);
            assert!(editor.switch_to_existing_editor_tab_for_path(&outside_path, false, cx));
            assert_eq!(
                editor.sidebar_root, a,
                "the existing tab's workspace owns the document"
            );
            assert_eq!(editor.active_tab_id, a_tab);
        });
    }

    #[gpui::test]
    fn workspace_settings_tabs_are_local_and_cancelled_removal_keeps_tabs(
        cx: &mut gpui::TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let a = directory.path().join("a");
        let b = directory.path().join("b");
        moeka_fs::local::native::create_dir_all(&a).unwrap();
        moeka_fs::local::native::create_dir_all(&b).unwrap();
        let editor = cx.update(|cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("# Draft"), false))
        });
        editor.update(cx, |editor, cx| {
            editor.sidebar_root = a.clone();
            editor.workspaces = vec![a.clone(), b.clone()];
            editor.tabs[0].is_dirty = true;
            let draft = editor.tabs[0].id.clone();
            let mut second = editor.tabs[0].clone();
            second.id = "workspace-second-draft".into();
            editor.tabs.push(second);
            editor.open_settings_tab(cx);
            let settings_a = editor.active_tab_id.clone();
            editor.switch_workspace(b.clone(), cx);
            editor.open_settings_tab(cx);
            assert_ne!(editor.active_tab_id, settings_a);
            assert_eq!(
                editor
                    .all_tabs()
                    .filter(|tab| tab.kind == TabKind::Settings)
                    .count(),
                2
            );
            editor.close_workspace(&a, cx);
            assert!(editor.dialog.is_some());
            assert!(editor.all_tabs().any(|tab| tab.id == draft));
            editor.confirm_dialog(cx);
            assert!(editor.dialog.is_some());
            assert!(editor.all_tabs().any(|tab| tab.id == draft && tab.is_dirty));
            editor.dismiss_dialog(cx);
            assert!(editor.workspace_close.is_none());
            assert_eq!(editor.workspaces.len(), 2);
            assert!(editor.all_tabs().any(|tab| tab.id == draft && tab.is_dirty));
            editor.close_workspace(&a, cx);
            editor.confirm_dialog(cx);
            editor.confirm_dialog(cx);
            assert_eq!(editor.workspaces, vec![b.clone()]);
            assert_eq!(editor.sidebar_root, b);
            assert!(!editor.workspace_states.contains_key(&a));
            assert!(!editor.all_tabs().any(|tab| tab.id == draft));
            editor.close_workspace(&b, cx);
            assert!(editor.workspaces.is_empty());
            assert!(editor.sidebar_root.as_os_str().is_empty());
        });
    }
}

pub(crate) struct WorkspaceState {
    pub needs_hydration: bool,
    pub layout: PaneLayout,
    pub focused_pane: PaneId,
    pub expanded_dirs: HashSet<PathBuf>,
    pub selected_paths: HashSet<PathBuf>,
    pub focused_path: Option<PathBuf>,
    pub file_list_dir: PathBuf,
    pub tree_scroll: f32,
    pub file_list_scroll: f32,
    pub history: Vec<TabNavigationTarget>,
    pub history_index: usize,
    pub closed_tabs: Vec<OpenTab>,
    pub tree: WorkspaceTreeState,
}

pub(crate) struct WorkspaceTreeState {
    scroll: ScrollHandle,
    file_list_scroll: ScrollHandle,
    items: Rc<Vec<SidebarItem>>,
    items_dirty: bool,
    children: HashMap<PathBuf, Vec<sidebar_tree::SidebarDirectoryChild>>,
    selection_anchor: Option<PathBuf>,
    error: Option<sidebar_tree::SidebarFsError>,
    filter: sidebar_filter::SidebarFilter,
    file_list_items: Vec<FileListItem>,
    file_list_loaded_dir: Option<PathBuf>,
}

impl Default for WorkspaceTreeState {
    fn default() -> Self {
        Self {
            scroll: ScrollHandle::new(),
            file_list_scroll: ScrollHandle::new(),
            items: Rc::new(Vec::new()),
            items_dirty: true,
            children: HashMap::new(),
            selection_anchor: None,
            error: None,
            filter: Default::default(),
            file_list_items: Vec::new(),
            file_list_loaded_dir: None,
        }
    }
}

impl WorkspaceTreeState {
    pub(super) fn finish_filter_request(&mut self, request: &Arc<std::sync::atomic::AtomicBool>) {
        self.filter.finish_request(request);
    }
}

impl WorkspaceState {
    pub fn new(root: &Path, layout: PaneLayout, focused_pane: PaneId) -> Self {
        Self {
            needs_hydration: false,
            layout,
            focused_pane,
            expanded_dirs: HashSet::from([root.to_path_buf()]),
            selected_paths: HashSet::new(),
            focused_path: None,
            file_list_dir: root.to_path_buf(),
            tree_scroll: 0.0,
            file_list_scroll: 0.0,
            history: Vec::new(),
            history_index: 0,
            closed_tabs: Vec::new(),
            tree: WorkspaceTreeState::default(),
        }
    }
}

/// `workspace_states` also holds the tab views of SSH workspaces, under keys
/// that cannot be local paths.
const REMOTE_VIEW_PREFIX: &str = "\u{0}ssh:";

pub(crate) fn remote_view_key(destination: &str, root: &moeka_fs::path::ResourcePath) -> PathBuf {
    PathBuf::from(format!(
        "{REMOTE_VIEW_PREFIX}{destination}\u{0}{}",
        root.path
    ))
}

pub(crate) fn is_remote_view_key(key: &Path) -> bool {
    key.to_str()
        .is_some_and(|key| key.starts_with(REMOTE_VIEW_PREFIX))
}

impl EditorView {
    /// The key of the tab view currently on screen: a local root, or an SSH view key.
    pub(crate) fn live_view_key(&self) -> PathBuf {
        self.live_view
            .clone()
            .unwrap_or_else(|| self.active_workspace_root().to_path_buf())
    }

    pub(crate) fn remote_view_key_of_current(&self) -> Option<PathBuf> {
        self.remote_workspace
            .as_ref()
            .map(|workspace| remote_view_key(&workspace.destination, &workspace.root))
    }

    /// Puts the live panes, history and closed-tab list aside under the live key.
    fn stash_live_view(&mut self) {
        debug_assert!(self.swapped_pane_id.is_none());
        self.persist_active_tab_state();
        let key = self
            .live_view
            .clone()
            .unwrap_or_else(|| self.sidebar_root.clone());
        let mut state = WorkspaceState::new(&key, self.pane_layout.clone(), self.focused_pane_id);
        state.history = std::mem::take(&mut self.tab_navigation_history);
        state.history_index = self.tab_navigation_index;
        state.closed_tabs = std::mem::take(&mut self.recently_closed_tabs);
        self.workspace_states.insert(key, state);
    }

    /// Brings the view stored under `key` (or a fresh one) onto the screen.
    fn restore_view(&mut self, key: &Path, cx: &mut Context<Self>) {
        let state = self.workspace_states.remove(key).unwrap_or_else(|| {
            let id = self.next_pane_id;
            self.next_pane_id += 1;
            self.panes.push(Pane::new(id));
            WorkspaceState::new(key, PaneLayout::Leaf(id), id)
        });
        self.pane_layout = state.layout;
        self.focus_pane(state.focused_pane, cx);
        if state.needs_hydration {
            session_store::hydrate_visible_workspace(self);
            self.invalidate_active_file_panel_caches();
        }
        self.tab_navigation_history = state.history;
        self.tab_navigation_index = state.history_index;
        self.recently_closed_tabs = state.closed_tabs;
        self.pane_bounds.clear();
        self.pending_terminal_focus = self.active_terminal_tab_id();
        session_store::schedule_visible_workspace_restores(self, cx);
    }

    /// Shows a workspace's own tab view: `None` for the local workspace, or the
    /// SSH workspace whose view key is given. Tabs never move between views.
    pub(crate) fn show_view(&mut self, target: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(key) = &target {
            // Make the connection serving this view the current one.
            let parked = self.parked_remote_workspaces.iter().position(|workspace| {
                remote_view_key(&workspace.destination, &workspace.root) == *key
            });
            if let Some(index) = parked {
                self.switch_remote_workspace(index, cx);
            }
        }
        if self.live_view == target {
            self.remote_workspace_active = target.is_some();
            return;
        }
        let adopt_unscoped = target.as_ref().is_some_and(|key| {
            self.live_view.is_none()
                && self.sidebar_root.as_os_str().is_empty()
                && !self.workspace_states.contains_key(key)
        });
        if adopt_unscoped {
            // Tabs opened before any workspace existed belong to the first one.
            self.live_view = target;
        } else {
            self.stash_live_view();
            self.live_view = target.clone();
            let key = target.unwrap_or_else(|| self.sidebar_root.clone());
            self.restore_view(&key, cx);
        }
        self.remote_workspace_active = self.live_view.is_some();
        session_store::persist_session(self);
        cx.notify();
    }

    /// Drops a view that no longer has a workspace (its tabs are already closed).
    pub(crate) fn discard_view(&mut self, key: &Path) {
        if let Some(state) = self.workspace_states.remove(key) {
            let removed = state.layout.leaf_ids();
            self.panes.retain(|pane| !removed.contains(&pane.id));
            self.pane_bounds.retain(|id, _| !removed.contains(id));
        }
    }

    /// Activates the workspace or view a pane belongs to.
    pub(crate) fn switch_to_workspace_key(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if is_remote_view_key(&root) {
            self.show_view(Some(root), cx);
        } else {
            self.switch_workspace(root, cx);
        }
    }

    pub(crate) fn workspace_for_document_path(&self, path: &Path) -> Option<PathBuf> {
        self.pane_id_for_editor_path(path)
            .and_then(|pane| self.workspace_for_pane(pane))
            .or_else(|| self.workspace_root_for_path(path))
    }

    pub(super) fn activate_starred_workspace(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(root) = self.workspace_for_document_path(path) {
            self.switch_workspace(root, cx);
        }
    }

    pub(crate) fn reject_workspace_root_mutation(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> bool {
        if paths
            .iter()
            .any(|path| self.workspaces.iter().any(|root| root.starts_with(path)))
        {
            self.notify_error(
                crate::i18n::runtime::RuntimeText::WorkspaceRootProtected.to_string(),
                cx,
            );
            true
        } else {
            false
        }
    }
    pub(crate) fn request_close_workspace(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.workspace_close.is_some() || !self.workspaces.iter().any(|path| path == root) {
            return;
        }
        self.workspace_close = Some(WorkspaceClose {
            root: root.to_path_buf(),
            remote: None,
            approved: HashMap::new(),
        });
        self.continue_workspace_close(cx);
    }

    pub(super) fn request_close_remote_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.remote_workspace.as_ref() else {
            return;
        };
        self.workspace_close = Some(WorkspaceClose {
            root: PathBuf::new(),
            remote: Some(RemoteWorkspaceClose {
                generation: Some(self.remote_generation),
                destination: workspace.destination.clone(),
                root: workspace.root.clone(),
            }),
            approved: HashMap::new(),
        });
        self.continue_workspace_close(cx);
    }

    pub(super) fn request_close_pending_remote_workspace(
        &mut self,
        destination: String,
        root: moeka_fs::path::ResourcePath,
        cx: &mut Context<Self>,
    ) {
        self.workspace_close = Some(WorkspaceClose {
            root: PathBuf::new(),
            remote: Some(RemoteWorkspaceClose {
                generation: None,
                destination,
                root,
            }),
            approved: HashMap::new(),
        });
        self.continue_workspace_close(cx);
    }

    fn pending_workspace_close_tabs(&self) -> Vec<String> {
        let Some(close) = &self.workspace_close else {
            return Vec::new();
        };
        if let Some(target) = &close.remote {
            self.all_tabs()
                .filter(|tab| {
                    tab.remote.as_ref().is_some_and(|remote| {
                        remote.root == target.root && remote.destination == target.destination
                    })
                })
                .map(|tab| tab.id.clone())
                .collect()
        } else {
            self.workspace_tab_ids(&close.root)
        }
    }

    fn workspace_close_text(&mut self, tab: &str) -> Option<String> {
        let pane = self.pane_id_for_tab(tab)?;
        self.with_pane(pane, |this| {
            if this.active_tab_id.as_deref() == Some(tab) {
                Some(this.buffer.text().to_string())
            } else {
                this.tabs
                    .iter()
                    .find(|item| item.id == tab)
                    .map(|item| item.snapshot.text.clone())
            }
        })
    }

    pub(crate) fn approve_workspace_tab_close(
        &mut self,
        tab: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(_root) = self
            .workspace_close
            .as_ref()
            .map(|close| close.root.clone())
        else {
            return false;
        };
        if !self
            .pending_workspace_close_tabs()
            .iter()
            .any(|id| id == tab)
        {
            return false;
        }
        if let Some(text) = self.workspace_close_text(tab) {
            self.workspace_close
                .as_mut()
                .unwrap()
                .approved
                .insert(tab.to_string(), text);
        }
        self.close_tab_return_to = None;
        self.continue_workspace_close(cx);
        true
    }

    fn continue_workspace_close(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self
            .workspace_close
            .as_ref()
            .map(|close| close.root.clone())
        else {
            return;
        };
        let remote = self.workspace_close.as_ref().and_then(|close| {
            close.remote.as_ref().map(|target| {
                (
                    target.generation,
                    target.destination.clone(),
                    target.root.clone(),
                )
            })
        });
        if remote
            .as_ref()
            .and_then(|(generation, ..)| *generation)
            .is_some_and(|generation| generation != self.remote_generation)
        {
            self.workspace_close = None;
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "wrong_connection"),
                cx,
            );
            return;
        }
        let ids = self.pending_workspace_close_tabs();
        if self.all_tabs().any(|tab| {
            ids.contains(&tab.id)
                && tab
                    .path
                    .as_deref()
                    .is_some_and(|path| !file_ops::DocumentSave::auto_save_allowed(path))
        }) {
            self.workspace_close = None;
            self.notify_error(
                crate::i18n::runtime::RuntimeText::AutoSavePaused.to_string(),
                cx,
            );
            return;
        }
        for id in &ids {
            let needs_confirmation = self
                .all_tabs()
                .find(|tab| &tab.id == id)
                .is_some_and(|tab| {
                    tab.is_dirty
                        || tab.kind.is_terminal() && self.terminal_tab_has_running_command(id)
                });
            if needs_confirmation {
                let text = self.workspace_close_text(id);
                let approved = self.workspace_close.as_ref().unwrap().approved.get(id);
                if text.as_ref() != approved {
                    self.request_close_tab(id, cx);
                    return;
                }
            }
        }
        self.workspace_close = None;
        self.close_tab_return_to = None;
        for id in ids {
            self.close_tab(&id, cx);
        }
        if let Some((generation, destination, remote_root)) = remote {
            let key = remote_view_key(&destination, &remote_root);
            if generation.is_some() {
                self.disconnect_remote(cx);
                self.discard_view(&key);
            } else {
                self.forget_pending_ssh_workspace_root(&destination, &remote_root, cx);
            }
        } else {
            self.finish_close_workspace(&root, cx);
        }
    }

    pub(crate) fn visible_pane_ids(&self) -> Vec<PaneId> {
        self.pane_layout.leaf_ids()
    }

    pub(crate) fn active_workspace_root(&self) -> &Path {
        self.swapped_workspace_root
            .as_deref()
            .unwrap_or(&self.sidebar_root)
    }

    pub(crate) fn workspace_for_pane(&self, pane: PaneId) -> Option<PathBuf> {
        if self.pane_layout.contains(pane) {
            return Some(self.live_view_key());
        }
        self.workspace_states
            .iter()
            .find(|(_, state)| state.layout.contains(pane))
            .map(|(root, _)| root.clone())
    }

    pub(crate) fn workspace_tab_ids(&self, root: &Path) -> Vec<String> {
        self.all_tabs()
            .filter(|tab| {
                self.pane_id_for_tab(&tab.id)
                    .and_then(|pane| self.workspace_for_pane(pane))
                    .as_deref()
                    == Some(root)
            })
            .map(|tab| tab.id.clone())
            .collect()
    }

    pub(super) fn park_workspace(&mut self) {
        debug_assert!(self.swapped_pane_id.is_none());
        self.persist_active_tab_state();
        self.sidebar_filter.suspend();
        let state = WorkspaceState {
            needs_hydration: false,
            layout: self.pane_layout.clone(),
            focused_pane: self.focused_pane_id,
            expanded_dirs: self.expanded_dirs.clone(),
            selected_paths: self.selected_sidebar_paths.clone(),
            focused_path: self.focused_sidebar_path.clone(),
            file_list_dir: self.file_list_dir.clone(),
            tree_scroll: self.pending_sidebar_scroll_top.get().map_or_else(
                || f32::from(self.sidebar_scroll_handle.offset().y),
                |top| -top,
            ),
            file_list_scroll: self.pending_file_list_scroll_top.get().map_or_else(
                || f32::from(self.file_list_scroll_handle.offset().y),
                |top| -top,
            ),
            history: std::mem::take(&mut self.tab_navigation_history),
            history_index: self.tab_navigation_index,
            closed_tabs: std::mem::take(&mut self.recently_closed_tabs),
            tree: WorkspaceTreeState {
                scroll: std::mem::replace(&mut self.sidebar_scroll_handle, ScrollHandle::new()),
                file_list_scroll: std::mem::replace(
                    &mut self.file_list_scroll_handle,
                    ScrollHandle::new(),
                ),
                items: std::mem::take(&mut self.sidebar_items_cache),
                items_dirty: self.sidebar_items_dirty,
                children: std::mem::take(&mut self.sidebar_directory_children),
                selection_anchor: self.sidebar_selection_anchor.take(),
                error: self.sidebar_error.take(),
                filter: std::mem::take(&mut self.sidebar_filter),
                file_list_items: std::mem::take(&mut self.file_list_items),
                file_list_loaded_dir: self.file_list_loaded_dir.take(),
            },
        };
        self.workspace_states
            .insert(self.sidebar_root.clone(), state);
        self.pending_sidebar_scroll_top = Rc::new(Cell::new(None));
        self.pending_file_list_scroll_top = Rc::new(Cell::new(None));
    }

    pub(super) fn activate_workspace_state(&mut self, cx: &mut Context<Self>) {
        let root = self.sidebar_root.clone();
        let state = self.workspace_states.remove(&root).unwrap_or_else(|| {
            let id = self.next_pane_id;
            self.next_pane_id += 1;
            self.panes.push(Pane::new(id));
            WorkspaceState::new(&root, PaneLayout::Leaf(id), id)
        });
        self.pane_layout = state.layout;
        self.focus_pane(state.focused_pane, cx);
        if state.needs_hydration {
            session_store::hydrate_visible_workspace(self);
            self.invalidate_active_file_panel_caches();
        }
        self.expanded_dirs = state.expanded_dirs;
        self.expanded_dirs.insert(root);
        self.selected_sidebar_paths = state.selected_paths;
        self.focused_sidebar_path = state.focused_path;
        self.file_list_dir = state.file_list_dir;
        self.tab_navigation_history = state.history;
        self.tab_navigation_index = state.history_index;
        self.recently_closed_tabs = state.closed_tabs;
        self.sidebar_scroll_handle = state.tree.scroll;
        self.file_list_scroll_handle = state.tree.file_list_scroll;
        self.sidebar_items_cache = state.tree.items;
        self.sidebar_items_dirty = state.tree.items_dirty;
        self.sidebar_directory_children = state.tree.children;
        self.sidebar_selection_anchor = state.tree.selection_anchor;
        self.sidebar_error = state.tree.error;
        self.sidebar_filter = state.tree.filter;
        self.file_list_items = state.tree.file_list_items;
        self.file_list_loaded_dir = state.tree.file_list_loaded_dir;
        self.sidebar_guide_cache = None;
        self.sidebar_tree_motion = None;
        self.pending_sidebar_reveal_path = None;
        self.pending_sidebar_scroll_anchor = None;
        self.file_list_pending_reveal_path = None;
        self.file_list_pending_reveal_centered = false;
        self.sidebar_scroll_handle
            .set_offset(gpui::point(px(0.0), px(state.tree_scroll)));
        self.file_list_scroll_handle
            .set_offset(gpui::point(px(0.0), px(state.file_list_scroll)));
        self.pending_sidebar_scroll_top
            .set(Some(-state.tree_scroll));
        self.pending_file_list_scroll_top
            .set(Some(-state.file_list_scroll));
        self.pane_bounds.clear();
        self.pending_terminal_focus = self.active_terminal_tab_id();
        // Refresh in the background while the previous accepted rows stay visible.
        self.sidebar_directory_loading.clear();
        self.sidebar_directory_epoch = self.sidebar_directory_epoch.wrapping_add(1);
        self.sidebar_directory_refresh_pending = true;
        self.file_list_epoch = self.file_list_epoch.wrapping_add(1);
        self.file_list_task = None;
        self.file_list_detail_task = None;
        self.file_list_dirty = true;
        self.sync_sidebar_filter_input(cx);
        session_store::schedule_visible_workspace_restores(self, cx);
    }
}
