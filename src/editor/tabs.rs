use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, SharedString, Window, div, prelude::*, px, svg,
};

use super::*;

const CHAT_PANEL_SVG_PATH: &str = "assets/icons/chat.svg";

#[derive(Clone, Copy)]
enum TabDropTarget<'a> {
    Before(&'a str),
    After(&'a str),
    End,
}

impl EditorView {
    fn ensure_settings_tab_first(&mut self) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.kind == TabKind::Settings)
        {
            if index != 0 {
                let settings_tab = self.tabs.remove(index);
                self.tabs.insert(0, settings_tab);
            }
        }
    }

    pub(crate) fn sidebar_root_name(&self) -> String {
        if self.sidebar_root.as_os_str().is_empty() {
            return self.t(crate::i18n::TextKey::OpenFolder).to_string();
        }

        self.sidebar_root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.sidebar_root.display().to_string())
    }

    pub(crate) fn active_tab_index(&self) -> Option<usize> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().position(|tab| &tab.id == id))
    }

    pub(crate) fn active_tab_is_settings(&self) -> bool {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .is_some_and(|tab| tab.kind == TabKind::Settings)
    }

    pub(crate) fn active_git_diff_state(&self) -> Option<&GitDiffState> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .and_then(|tab| tab.git_diff.as_ref())
    }

    pub(crate) fn request_editor_focus(&mut self, cx: &mut Context<Self>) {
        self.focused_sidebar_path = None;
        self.pending_editor_focus = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;

            let _ = this.update(cx, |this, cx| {
                this.focused_sidebar_path = None;
                this.pending_editor_focus = true;
                cx.notify();
            });
        })
        .detach();
    }

    fn on_tab_bar_drag_space_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_drag_candidate_id.is_some() || self.tab_dragging_id.is_some() {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        self.focused_sidebar_path = None;
        window.prevent_default();
        cx.stop_propagation();

        if event.click_count >= 2 {
            window.titlebar_double_click();
        } else {
            Self::start_native_window_drag(window);
        }

        cx.notify();
    }

    #[cfg(target_os = "macos")]
    fn start_native_window_drag(window: &mut Window) {
        use objc::{class, msg_send, sel, sel_impl};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
                unsafe {
                    let ns_view = appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
                    let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
                    let app: *mut objc::runtime::Object =
                        msg_send![class!(NSApplication), sharedApplication];
                    let event: *mut objc::runtime::Object = msg_send![app, currentEvent];
                    if !ns_window.is_null() && !event.is_null() {
                        let () = msg_send![ns_window, performWindowDragWithEvent: event];
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn start_native_window_drag(window: &mut Window) {
        window.start_window_move();
    }

    fn preview_tab_index(&self) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.kind == TabKind::Editor && tab.is_preview)
    }

    fn tab_drop_changes_order_for(
        current_tab_ids: &[String],
        settings_tab_id: Option<&str>,
        from_id: &str,
        target: TabDropTarget<'_>,
    ) -> bool {
        let Some(from_idx) = current_tab_ids.iter().position(|tab_id| tab_id == from_id) else {
            return false;
        };

        let mut reordered = current_tab_ids.to_vec();
        let moved = reordered.remove(from_idx);

        match target {
            TabDropTarget::Before(to_id) => {
                let Some(to_idx) = reordered.iter().position(|tab_id| tab_id == to_id) else {
                    return false;
                };
                reordered.insert(to_idx, moved);
            }
            TabDropTarget::After(to_id) => {
                let Some(to_idx) = reordered.iter().position(|tab_id| tab_id == to_id) else {
                    return false;
                };
                reordered.insert((to_idx + 1).min(reordered.len()), moved);
            }
            TabDropTarget::End => reordered.push(moved),
        }

        if let Some(settings_tab_id) = settings_tab_id
            && let Some(settings_idx) = reordered
                .iter()
                .position(|tab_id| tab_id == settings_tab_id)
            && settings_idx != 0
        {
            let settings_tab = reordered.remove(settings_idx);
            reordered.insert(0, settings_tab);
        }

        reordered != current_tab_ids
    }

    fn editor_tab_index_for_path(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            tab.kind == TabKind::Editor
                && tab.path.as_ref().is_some_and(|tab_path| tab_path == path)
        })
    }

    fn tab_navigation_target_for_tab(tab: &OpenTab) -> TabNavigationTarget {
        match tab.kind {
            TabKind::Settings => TabNavigationTarget::Settings,
            _ => tab
                .path
                .as_ref()
                .map(|path| TabNavigationTarget::File(path.clone()))
                .unwrap_or_else(|| TabNavigationTarget::LiveTab(tab.id.clone())),
        }
    }

    fn active_tab_navigation_target(&self) -> Option<TabNavigationTarget> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .map(Self::tab_navigation_target_for_tab)
    }

    fn record_active_tab_navigation_target(&mut self) {
        if self.tab_navigation_suspended {
            return;
        }
        let Some(target) = self.active_tab_navigation_target() else {
            return;
        };
        if self.tab_navigation_history.get(self.tab_navigation_index) == Some(&target) {
            return;
        }

        if self.tab_navigation_index + 1 < self.tab_navigation_history.len() {
            self.tab_navigation_history
                .truncate(self.tab_navigation_index + 1);
        }

        if let Some(existing_index) = self
            .tab_navigation_history
            .iter()
            .position(|entry| entry == &target)
        {
            self.tab_navigation_history.remove(existing_index);
            if existing_index < self.tab_navigation_index {
                self.tab_navigation_index = self.tab_navigation_index.saturating_sub(1);
            }
        }

        self.tab_navigation_history.push(target);
        self.tab_navigation_index = self.tab_navigation_history.len().saturating_sub(1);
    }

    fn restore_closed_tab(&mut self, closed_tab: OpenTab, cx: &mut Context<Self>) {
        let tab = OpenTab {
            id: self.next_tab_identity(),
            title: closed_tab.title,
            kind: TabKind::Editor,
            path: closed_tab.path,
            file_kind: closed_tab.file_kind,
            editor_left_aligned: closed_tab.editor_left_aligned,
            snapshot: closed_tab.snapshot,
            image_view: closed_tab.image_view,
            git_diff: None,
            is_preview: false,
            is_dirty: false,
            is_deleted: false,
            is_pinned: false,
            recovery: closed_tab.recovery,
            history: closed_tab.history,
        };
        self.tabs.push(tab);
        let index = self.tabs.len() - 1;
        self.activate_tab_by_index(index, cx);
    }

    fn activate_tab_navigation_target(
        &mut self,
        target: &TabNavigationTarget,
        cx: &mut Context<Self>,
    ) -> bool {
        match target {
            TabNavigationTarget::Settings => {
                self.open_settings_tab(cx);
                true
            }
            TabNavigationTarget::File(path) => {
                if let Some(index) = self.editor_tab_index_for_path(path) {
                    self.activate_tab_by_index(index, cx);
                    return true;
                }

                if let Some(closed_index) = self
                    .recently_closed_tabs
                    .iter()
                    .rposition(|tab| tab.path.as_ref() == Some(path))
                {
                    let closed_tab = self.recently_closed_tabs.remove(closed_index);
                    self.restore_closed_tab(closed_tab, cx);
                    return true;
                }

                self.open_path_in_permanent_tab(path.clone(), true, cx);
                self.editor_tab_index_for_path(path).is_some()
            }
            TabNavigationTarget::LiveTab(tab_id) => {
                if let Some(index) = self.tabs.iter().position(|tab| &tab.id == tab_id) {
                    self.activate_tab_by_index(index, cx);
                    true
                } else {
                    false
                }
            }
        }
    }

    pub(crate) fn can_navigate_tab_history_back(&self) -> bool {
        self.tab_navigation_index > 0
    }

    pub(crate) fn can_navigate_tab_history_forward(&self) -> bool {
        self.tab_navigation_index + 1 < self.tab_navigation_history.len()
    }

    pub(crate) fn navigate_tab_history_back(&mut self, cx: &mut Context<Self>) {
        while self.tab_navigation_index > 0 {
            let target_index = self.tab_navigation_index - 1;
            let target = self.tab_navigation_history[target_index].clone();
            self.tab_navigation_suspended = true;
            let activated = self.activate_tab_navigation_target(&target, cx);
            self.tab_navigation_suspended = false;
            if activated {
                self.tab_navigation_index = target_index;
                return;
            }

            self.tab_navigation_history.remove(target_index);
            self.tab_navigation_index = self.tab_navigation_index.saturating_sub(1);
        }
    }

    pub(crate) fn navigate_tab_history_forward(&mut self, cx: &mut Context<Self>) {
        while self.tab_navigation_index + 1 < self.tab_navigation_history.len() {
            let target_index = self.tab_navigation_index + 1;
            let target = self.tab_navigation_history[target_index].clone();
            self.tab_navigation_suspended = true;
            let activated = self.activate_tab_navigation_target(&target, cx);
            self.tab_navigation_suspended = false;
            if activated {
                self.tab_navigation_index = target_index;
                return;
            }

            self.tab_navigation_history.remove(target_index);
        }
    }

    pub(crate) fn switch_to_existing_editor_tab_for_path(
        &mut self,
        path: &Path,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.editor_tab_index_for_path(path) else {
            return false;
        };
        self.activate_tab_by_index(index, cx);
        if focus_editor {
            self.request_editor_focus(cx);
        }
        true
    }

    fn load_editor_tab_state_for_path(path: &Path) -> Option<(EditorSnapshot, FileKind)> {
        let bytes = std::fs::read(path).ok()?;
        let file_kind = classify_file_kind_for_bytes(path, &bytes);
        let text = if file_kind.supports_text_buffer() {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            String::new()
        };
        let first_line_end = text.find('\n').unwrap_or(text.len());
        Some((
            EditorSnapshot {
                text,
                cursor_offset: first_line_end,
                cursor_anchor: first_line_end,
                scroll_offset: 0.0,
                extra_cursors: Vec::new(),
            },
            file_kind,
        ))
    }

    fn build_file_tab(&mut self, path: &Path, is_preview: bool) -> Option<OpenTab> {
        let (snapshot, file_kind) = Self::load_editor_tab_state_for_path(path)?;
        let id = self.next_tab_identity();
        let history = history_store::load_tab_history(&id, Some(path), &snapshot);
        Some(OpenTab {
            id,
            title: Self::title_for_path(path),
            kind: TabKind::Editor,
            path: Some(path.to_path_buf()),
            file_kind: Some(file_kind),
            editor_left_aligned: false,
            snapshot,
            image_view: ImageViewState::default(),
            git_diff: None,
            is_preview,
            is_dirty: false,
            is_deleted: false,
            is_pinned: false,
            recovery: config_store::TabRecoveryState::default(),
            history,
        })
    }

    pub(crate) fn open_path_in_preview_tab(
        &mut self,
        path: PathBuf,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.editor_tab_index_for_path(&path) {
            self.activate_tab_by_index(index, cx);
            if focus_editor {
                self.request_editor_focus(cx);
            }
            return;
        }

        if let Some(index) = self.preview_tab_index() {
            if self.tabs[index].is_dirty {
                self.tabs[index].is_preview = false;
            }
        }

        let Some(mut tab) = self.build_file_tab(&path, true) else {
            cx.notify();
            return;
        };

        self.persist_active_tab_state();
        if let Some(index) = self.preview_tab_index() {
            tab.id = self.tabs[index].id.clone();
            self.tabs[index] = tab;
            self.activate_tab_by_index_without_persist(index, cx);
        } else {
            let insert_index = self
                .active_tab_index()
                .map(|index| index + 1)
                .unwrap_or(self.tabs.len());
            self.tabs.insert(insert_index, tab);
            self.activate_tab_by_index_without_persist(insert_index, cx);
        }

        if focus_editor {
            self.request_editor_focus(cx);
        }
        session_store::persist_session(self);
    }

    pub(crate) fn open_path_in_permanent_tab(
        &mut self,
        path: PathBuf,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.editor_tab_index_for_path(&path) {
            self.tabs[index].is_preview = false;
            self.activate_tab_by_index(index, cx);
            if focus_editor {
                self.request_editor_focus(cx);
            }
            return;
        }

        let Some(tab) = self.build_file_tab(&path, false) else {
            cx.notify();
            return;
        };

        self.persist_active_tab_state();
        let insert_index = self
            .active_tab_index()
            .map(|index| index + 1)
            .unwrap_or(self.tabs.len());
        self.tabs.insert(insert_index, tab);
        self.activate_tab_by_index_without_persist(insert_index, cx);
        if focus_editor {
            self.request_editor_focus(cx);
        }
        session_store::persist_session(self);
    }

    pub(crate) fn open_external_paths_in_tabs(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        let file_paths: Vec<PathBuf> = paths
            .iter()
            .filter(|path| path.is_file())
            .cloned()
            .collect();
        let last_index = file_paths.len().saturating_sub(1);

        for (index, path) in file_paths.into_iter().enumerate() {
            self.open_path_in_permanent_tab(path, index == last_index, cx);
        }
    }

    fn should_track_in_recently_closed(tab: &OpenTab) -> bool {
        tab.kind == TabKind::Editor && tab.path.is_some() && !tab.is_preview
    }

    pub(crate) fn persist_active_tab_state(&mut self) {
        if let Some(index) = self.active_tab_index() {
            if matches!(self.tabs[index].kind, TabKind::Editor | TabKind::Diff) {
                if self.tabs[index]
                    .file_kind
                    .as_ref()
                    .is_some_and(FileKind::supports_text_buffer)
                {
                    self.tabs[index].snapshot = self.current_snapshot();
                }
                self.tabs[index].editor_left_aligned = self.editor_left_aligned;
                self.current_file_path = self.tabs[index].path.clone();
            }
        }
    }

    pub(crate) fn activate_tab_by_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.persist_active_tab_state();
        self.activate_tab_by_index_without_persist(index, cx);
        session_store::persist_session(self);
    }

    pub(crate) fn activate_tab_by_index_without_persist(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }
        let tab = self.tabs[index].clone();
        self.invalidate_active_file_panel_caches();
        self.active_tab_id = Some(tab.id.clone());
        self.current_file_path = tab.path.clone();
        self.editor_left_aligned = tab.editor_left_aligned;
        if matches!(tab.kind, TabKind::Editor | TabKind::Diff) {
            if tab
                .file_kind
                .as_ref()
                .is_some_and(FileKind::supports_text_buffer)
            {
                self.restore_tab_snapshot_state(&tab.snapshot);
            } else {
                self.buffer = Buffer::from_text("");
                self.cursor.offset = 0;
                self.cursor.anchor = 0;
                self.extra_cursors.clear();
            }
            self.marked_range = None;
            self.close_preview_overlay();
            self.toolbar_hidden_by_scroll = self.scroll_offset > 0.5;
            self.toolbar_corner_hovered = false;
            self.sync_toolbar_opacity(cx);
            if tab.file_kind.as_ref().is_some_and(FileKind::is_image)
                && let Some(path) = tab.path.as_ref()
            {
                self.queue_image_load(path.to_string_lossy().into_owned(), cx);
            }
            self.clamp_scroll();
            self.pause_blinking(cx);
            self.request_editor_focus(cx);
        }
        if let Some(path) = tab.path.clone() {
            let suppress_sidebar_sync = self
                .suppress_next_sidebar_sync_path
                .as_ref()
                .is_some_and(|suppressed| suppressed == &path);
            if suppress_sidebar_sync {
                self.suppress_next_sidebar_sync_path = None;
            } else {
                self.sync_path_in_sidebar_state(path, cx);
            }
        }
        self.record_active_tab_navigation_target();
        cx.notify();
    }

    pub(crate) fn switch_to_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if self.active_tab_id.as_deref() == Some(tab_id) {
            return;
        }
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) {
            self.activate_tab_by_index(index, cx);
        }
    }

    pub(crate) fn close_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            self.process_pending_close_queue(cx);
            return;
        };
        let was_active = self.active_tab_id.as_deref() == Some(tab_id);
        // Save to recently closed (keep up to 20)
        let removed = self.tabs.remove(index);
        if Self::should_track_in_recently_closed(&removed) {
            self.recently_closed_tabs.push(removed);
            if self.recently_closed_tabs.len() > 20 {
                self.recently_closed_tabs.remove(0);
            }
        }
        if self.tabs.is_empty() {
            self.active_tab_id = None;
            self.pending_close_tab_ids.clear();
            cx.notify();
            session_store::persist_session(self);
            return;
        }
        if was_active {
            let next_index = index.min(self.tabs.len().saturating_sub(1));
            self.activate_tab_by_index(next_index, cx);
        } else {
            cx.notify();
        }
        self.process_pending_close_queue(cx);
        session_store::persist_session(self);
    }

    pub(crate) fn process_pending_close_queue(&mut self, cx: &mut Context<Self>) {
        while let Some(tab_id) = self.pending_close_tab_ids.first().cloned() {
            self.pending_close_tab_ids.remove(0);
            if self.tabs.iter().any(|t| t.id == tab_id) {
                self.request_close_tab(&tab_id, cx);
                return;
            }
        }
    }

    pub(crate) fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(tab_id) = self.active_tab_id.clone() {
            self.request_close_tab(&tab_id, cx);
        }
    }

    pub(crate) fn request_close_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) {
            if tab.is_dirty {
                self.show_dialog(
                    Dialog {
                        title: self
                            .t(crate::i18n::TextKey::UnsavedChangesTitle)
                            .to_string(),
                        message: crate::i18n::close_tab_unsaved_message(
                            self.ui_language,
                            &tab.title,
                        ),
                        save_label: Some(self.t(crate::i18n::TextKey::Save).to_string()),
                        confirm_label: self.t(crate::i18n::TextKey::Discard).to_string(),
                        cancel_label: self.t(crate::i18n::TextKey::Cancel).to_string(),
                        is_danger: true,
                        action: DialogAction::CloseTabUnsaved {
                            tab_id: tab_id.to_string(),
                        },
                    },
                    cx,
                );
                return;
            }
        }

        self.close_tab(tab_id, cx);
    }

    pub(crate) fn reopen_closed_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(closed_tab) = self.recently_closed_tabs.pop() {
            self.restore_closed_tab(closed_tab, cx);
            session_store::persist_session(self);
        }
    }

    pub(crate) fn activate_previous_tab(&mut self, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        if let Some(current) = self.active_tab_index() {
            let prev = if current == 0 {
                self.tabs.len() - 1
            } else {
                current - 1
            };
            self.activate_tab_by_index(prev, cx);
        }
    }

    pub(crate) fn activate_next_tab(&mut self, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        if let Some(current) = self.active_tab_index() {
            let next = if current >= self.tabs.len() - 1 {
                0
            } else {
                current + 1
            };
            self.activate_tab_by_index(next, cx);
        }
    }

    pub(crate) fn toggle_pin_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.is_pinned = !tab.is_pinned;
        }
        // Move pinned tabs to the left
        self.tabs.sort_by_key(|t| !t.is_pinned);
        self.ensure_settings_tab_first();
        cx.notify();
    }

    pub(crate) fn promote_tab_to_permanent(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            if tab.is_preview {
                tab.is_preview = false;
                cx.notify();
            }
        }
    }

    pub(crate) fn close_left_tabs(&mut self, from_tab_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == from_tab_id) else {
            return;
        };
        let to_close: Vec<String> = self.tabs[..index]
            .iter()
            .filter(|t| !t.is_pinned)
            .map(|t| t.id.clone())
            .collect();
        self.pending_close_tab_ids = to_close;
        self.process_pending_close_queue(cx);
    }

    pub(crate) fn close_other_tabs(&mut self, current_tab_id: &str, cx: &mut Context<Self>) {
        let to_close: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.id != current_tab_id && !t.is_pinned)
            .map(|t| t.id.clone())
            .collect();
        self.pending_close_tab_ids = to_close;
        self.process_pending_close_queue(cx);
    }

    pub(crate) fn close_right_tabs(&mut self, from_tab_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == from_tab_id) else {
            return;
        };
        let to_close: Vec<String> = self.tabs[(index + 1)..]
            .iter()
            .filter(|t| !t.is_pinned)
            .map(|t| t.id.clone())
            .collect();
        self.pending_close_tab_ids = to_close;
        self.process_pending_close_queue(cx);
    }

    pub(crate) fn close_saved_tabs(&mut self, cx: &mut Context<Self>) {
        let active_id = self.active_tab_id.clone();
        let to_close: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| !t.is_dirty && active_id.as_deref() != Some(t.id.as_str()))
            .map(|t| t.id.clone())
            .collect();
        self.pending_close_tab_ids = to_close;
        self.process_pending_close_queue(cx);
    }

    pub(crate) fn reorder_tab_before(
        &mut self,
        from_id: &str,
        to_id: &str,
        cx: &mut Context<Self>,
    ) {
        if from_id == to_id {
            return;
        }
        let Some(from_idx) = self.tabs.iter().position(|tab| tab.id == from_id) else {
            return;
        };
        let Some(mut to_idx) = self.tabs.iter().position(|tab| tab.id == to_id) else {
            return;
        };
        let moved = self.tabs.remove(from_idx);
        if from_idx < to_idx {
            to_idx = to_idx.saturating_sub(1);
        }
        self.tabs.insert(to_idx, moved);
        self.ensure_settings_tab_first();
        cx.notify();
    }

    pub(crate) fn reorder_tab_after(&mut self, from_id: &str, to_id: &str, cx: &mut Context<Self>) {
        if from_id == to_id {
            return;
        }
        let Some(from_idx) = self.tabs.iter().position(|tab| tab.id == from_id) else {
            return;
        };
        let Some(mut to_idx) = self.tabs.iter().position(|tab| tab.id == to_id) else {
            return;
        };
        let moved = self.tabs.remove(from_idx);
        if from_idx < to_idx {
            to_idx = to_idx.saturating_sub(1);
        }
        self.tabs.insert((to_idx + 1).min(self.tabs.len()), moved);
        self.ensure_settings_tab_first();
        cx.notify();
    }

    pub(crate) fn reorder_tab_to_end(&mut self, from_id: &str, cx: &mut Context<Self>) {
        let Some(from_idx) = self.tabs.iter().position(|tab| tab.id == from_id) else {
            return;
        };
        let moved = self.tabs.remove(from_idx);
        self.tabs.push(moved);
        self.ensure_settings_tab_first();
        cx.notify();
    }

    pub(crate) fn mark_active_tab_dirty(&mut self) {
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].is_preview = false;
            self.tabs[index].is_dirty = true;
        }
    }

    pub(crate) fn title_for_path(path: &Path) -> String {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn untitled_display_index(&self, title: &str) -> Option<usize> {
        let untitled = self.t(crate::i18n::TextKey::Untitled);
        if title == untitled {
            return Some(1);
        }

        title
            .strip_prefix(&format!("{untitled} "))?
            .parse::<usize>()
            .ok()
            .filter(|index| *index > 1)
    }

    fn next_untitled_title(&self) -> String {
        let mut used_indices = HashSet::new();
        for tab in &self.tabs {
            if tab.kind != TabKind::Editor || tab.path.is_some() {
                continue;
            }
            if let Some(index) = self.untitled_display_index(&tab.title) {
                used_indices.insert(index);
            }
        }

        let mut next_index = 1;
        while used_indices.contains(&next_index) {
            next_index += 1;
        }

        crate::i18n::untitled_title(self.ui_language, next_index)
    }

    pub(crate) fn create_scratch_tab(&mut self, cx: &mut Context<Self>) {
        self.persist_active_tab_state();
        let title = self.next_untitled_title();
        let tab_id = self.next_tab_identity();
        let tab = OpenTab {
            id: tab_id.clone(),
            title,
            kind: TabKind::Editor,
            path: None,
            file_kind: Some(FileKind::Markdown),
            editor_left_aligned: false,
            snapshot: EditorSnapshot {
                text: String::new(),
                cursor_offset: 0,
                cursor_anchor: 0,
                scroll_offset: 0.0,
                extra_cursors: Vec::new(),
            },
            image_view: ImageViewState::default(),
            git_diff: None,
            is_preview: false,
            is_dirty: false,
            is_deleted: false,
            is_pinned: false,
            recovery: config_store::TabRecoveryState::default(),
            history: history_store::TabHistory::new(
                Some(history_store::default_source_identity(&tab_id, None)),
                Some(history_store::default_journal_path(&tab_id, None)),
            ),
        };
        self.tabs.push(tab);
        if let Some(index) = self.tabs.len().checked_sub(1) {
            self.activate_tab_by_index(index, cx);
        }
        self.scroll_tab_bar_to_end(cx);
        session_store::persist_session(self);
    }

    fn scroll_tab_bar_to_end(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;

            this.update(cx, |this, _| {
                let offset = this.tab_bar_scroll_handle.offset();
                let max_offset = this.tab_bar_scroll_handle.max_offset();
                let trailing_reserved =
                    this.scaled(this.styles.tab_bar.tab_end_drop_target_width_px);
                let target_x = -((max_offset.width - px(trailing_reserved)).max(px(0.0)));
                this.tab_bar_scroll_handle
                    .set_offset(gpui::point(target_x, offset.y));
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn next_tab_identity(&mut self) -> String {
        let id = format!("tab-{}", self.next_tab_id);
        self.next_tab_id += 1;
        id
    }

    pub(crate) fn open_settings_tab(&mut self, cx: &mut Context<Self>) {
        // Singleton: if a settings tab already exists, switch to it
        if self.tabs.iter().any(|t| t.kind == TabKind::Settings) {
            self.ensure_settings_tab_first();
            self.activate_tab_by_index(0, cx);
            return;
        }
        self.persist_active_tab_state();
        let tab = OpenTab {
            id: self.next_tab_identity(),
            title: self.t(crate::i18n::TextKey::SettingsTitle).to_string(),
            kind: TabKind::Settings,
            path: None,
            file_kind: None,
            editor_left_aligned: false,
            snapshot: EditorSnapshot {
                text: String::new(),
                cursor_offset: 0,
                cursor_anchor: 0,
                scroll_offset: 0.0,
                extra_cursors: Vec::new(),
            },
            image_view: ImageViewState::default(),
            git_diff: None,
            is_preview: false,
            is_dirty: false,
            is_deleted: false,
            is_pinned: false,
            recovery: config_store::TabRecoveryState::default(),
            history: history_store::TabHistory::default(),
        };
        self.tabs.push(tab);
        self.ensure_settings_tab_first();
        self.ensure_font_size_editor(cx);
        self.ensure_ui_font_size_editor(cx);
        if let Some(index) = self.tabs.iter().position(|t| t.kind == TabKind::Settings) {
            self.activate_tab_by_index(index, cx);
        }
        session_store::persist_session(self);
    }

    // ── Tab rendering ───────────────────────────────────────────────

    fn render_tab_title_label(
        &self,
        tab: &OpenTab,
        is_active: bool,
        is_preview: bool,
        is_deleted: bool,
    ) -> AnyElement {
        let ts = &self.styles.tab_bar;
        let text_color = if is_deleted {
            ts.tab_item_deleted_text_color
        } else if is_active {
            ts.tab_item_active_text_color
        } else {
            ts.tab_item_text_color
        };
        let (base, suffix) = split_file_name_for_display(&tab.title);

        div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .flex()
            .gap(px(0.0))
            .items_center()
            .text_size(px(self.scaled(ts.tab_item_font_size_px)))
            .text_color(text_color)
            .when(is_preview && !is_active, |this| this.opacity(0.72))
            .when(is_preview, |this| {
                this.font_family("Helvetica Neue").italic()
            })
            .font_weight(if is_preview {
                FontWeight::NORMAL
            } else if is_active {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .when(is_deleted, |this| this.line_through())
            .child(div().whitespace_nowrap().child(base.to_string()))
            .when_some(suffix.filter(|_| tab.path.is_some()), |this, suffix| {
                this.child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .opacity(ts.tab_item_suffix_opacity)
                        .child(suffix.to_string()),
                )
            })
            .into_any_element()
    }

    fn active_tab_breadcrumb_items(&self) -> Vec<(String, Option<PathBuf>)> {
        let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        else {
            return Vec::new();
        };

        let Some(path) = tab.path.as_ref() else {
            return vec![(tab.title.clone(), None)];
        };

        let Some(root) = self.workspace_root_for_path(path) else {
            return vec![(tab.title.clone(), Some(path.clone()))];
        };

        let root_name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| root.display().to_string());

        let mut segments = vec![(root_name, Some(root.clone()))];
        let mut segment_path = root.clone();
        if let Ok(relative_path) = path.strip_prefix(&root) {
            for component in relative_path.components() {
                if let std::path::Component::Normal(name) = component {
                    let segment = name.to_string_lossy().into_owned();
                    if !segment.is_empty() {
                        segment_path.push(&segment);
                        segments.push((segment, Some(segment_path.clone())));
                    }
                }
            }
        }

        if segments.len() == 1 {
            segments.push((tab.title.clone(), Some(path.clone())));
        }

        segments
    }

    fn diff_path_bar_navigation_labels(&self) -> (&'static str, &'static str, &'static str) {
        let show_only_changes = self.active_git_diff_show_only_changes();
        match self.ui_language {
            crate::i18n::UiLanguage::ChineseSimplified => (
                if show_only_changes {
                    "展开"
                } else {
                    "折叠"
                },
                "上一个",
                "下一个",
            ),
            crate::i18n::UiLanguage::ChineseTraditional => (
                if show_only_changes {
                    "展開"
                } else {
                    "摺疊"
                },
                "上一個",
                "下一個",
            ),
            _ => (
                if show_only_changes {
                    "Expand"
                } else {
                    "Collapse"
                },
                "Prev",
                "Next",
            ),
        }
    }

    pub(crate) fn render_active_tab_path_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let es = &self.styles.editor;
        let gap = self.scaled(es.editor_path_bar_gap_px);
        let separator_size = self.scaled(es.editor_path_bar_separator_size_px);
        let action_gap = self.scaled(es.editor_path_bar_action_gap_px);
        let action_size = self.scaled(es.editor_path_bar_action_btn_size_px);
        let action_icon_size = self.scaled(es.editor_path_bar_action_icon_size_px);
        let action_hover_opacity = es.editor_path_bar_action_hover_opacity;
        let item_padding_x = self.scaled(es.editor_path_bar_item_padding_x_px);
        let item_padding_y = self.scaled(es.editor_path_bar_item_padding_y_px);
        let item_radius = self.scaled(es.editor_path_bar_item_radius_px);
        let item_hover_text_color = es.editor_path_bar_item_hover_text_color;
        let item_opacity = es.editor_path_bar_item_opacity;
        let diff_group_gap = self.scaled(es.editor_path_bar_diff_group_gap_px);
        let diff_navigation_labels = self.diff_path_bar_navigation_labels();
        let diff_label = self
            .active_git_diff_state()
            .map(|diff| (diff.source_label.clone(), diff.target_label.clone()));
        let mut breadcrumb = div()
            .id("editor-path-bar-content")
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(gap));
        let segments = self.active_tab_breadcrumb_items();

        for (index, (segment, segment_path)) in segments.iter().enumerate() {
            if index > 0 {
                breadcrumb = breadcrumb.child(
                    svg()
                        .path("assets/icons/right.svg")
                        .size(px(separator_size))
                        .flex_none()
                        .text_color(es.editor_path_bar_separator_color),
                );
            }

            let is_last = index + 1 == segments.len();
            let segment_path = segment_path.clone();
            if is_last {
                let (base, suffix) = split_file_name_for_display(segment);
                let mut current = div()
                    .cursor(CursorStyle::Arrow)
                    .when_some(segment_path.clone(), |this, reveal_path| {
                        this.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.reveal_path_in_sidebar(reveal_path.clone(), cx);
                            }),
                        )
                    })
                    .flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden();
                current = current.child(
                    div()
                        .px(px(item_padding_x))
                        .py(px(item_padding_y))
                        .rounded(px(item_radius))
                        .text_color(item_hover_text_color)
                        .opacity(item_opacity)
                        .hover(move |style| style.opacity(1.0))
                        .flex()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .items_center()
                        .whitespace_nowrap()
                        .child(div().whitespace_nowrap().child(base.to_string()))
                        .when_some(suffix.clone(), |this, suffix| {
                            this.child(
                                div()
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .opacity(es.editor_path_bar_suffix_opacity)
                                    .child(suffix.to_string()),
                            )
                        }),
                );
                breadcrumb = breadcrumb.child(current);
            } else {
                breadcrumb = breadcrumb.child(
                    div()
                        .cursor(CursorStyle::Arrow)
                        .when_some(segment_path, |this, reveal_path| {
                            this.on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.reveal_path_in_sidebar(reveal_path.clone(), cx);
                                }),
                            )
                        })
                        .flex_none()
                        .whitespace_nowrap()
                        .child(
                            div()
                                .px(px(item_padding_x))
                                .py(px(item_padding_y))
                                .rounded(px(item_radius))
                                .text_color(item_hover_text_color)
                                .opacity(item_opacity)
                                .hover(move |style| style.opacity(1.0))
                                .whitespace_nowrap()
                                .child(segment.clone()),
                        ),
                );
            }
        }

        div()
            .id("editor-path-bar")
            .w_full()
            .h(px(self.scaled(es.editor_path_bar_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px(px(self.scaled(es.editor_path_bar_padding_x_px)))
            .bg(es.editor_path_bar_bg)
            .border_b(px(self.scaled(es.editor_path_bar_border_width_px)))
            .border_color(es.editor_path_bar_border_color)
            .text_size(px(self.scaled(es.editor_path_bar_font_size_px)))
            .text_color(es.editor_path_bar_text_color)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event, window, cx| {
                    this.on_mouse_down(event, window, cx);
                }),
            )
            .child(breadcrumb)
            .when_some(diff_label, |this, (source_label, target_label)| {
                let (toggle_label, previous_label, next_label) = diff_navigation_labels;
                let diff_button =
                    |icon_path: &'static str,
                     label: &'static str,
                     enabled: bool,
                     handler: fn(&mut Self, &mut Context<Self>)| {
                        let mut button = div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(gap))
                            .whitespace_nowrap()
                            .cursor(CursorStyle::Arrow)
                            .opacity(if enabled {
                                item_opacity
                            } else {
                                item_opacity * 0.55
                            })
                            .child(
                                svg()
                                    .path(icon_path)
                                    .size(px(action_icon_size))
                                    .flex_none()
                                    .text_color(es.editor_path_bar_text_color),
                            )
                            .child(label);

                        if enabled {
                            button = button.hover(move |style| style.opacity(1.0)).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    handler(this, cx);
                                }),
                            );
                        }

                        button
                    };

                this.child(
                    div()
                        .pl(px(action_gap))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(diff_group_gap))
                        .text_color(es.editor_path_bar_text_color)
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(gap))
                                .child(diff_button(
                                    "assets/icons/filetree/workspace-expand.svg",
                                    toggle_label,
                                    true,
                                    Self::toggle_git_diff_show_only_changes,
                                ))
                                .child(diff_button(
                                    "assets/icons/arrow-up-circle.svg",
                                    previous_label,
                                    self.can_navigate_previous_git_diff_row(),
                                    Self::navigate_previous_git_diff_row,
                                ))
                                .child(diff_button(
                                    "assets/icons/arrow-down-circle.svg",
                                    next_label,
                                    self.can_navigate_next_git_diff_row(),
                                    Self::navigate_next_git_diff_row,
                                )),
                        )
                        .child(
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .opacity(item_opacity)
                                .child(source_label),
                        )
                        .child(
                            svg()
                                .path("assets/icons/right.svg")
                                .size(px(separator_size))
                                .flex_none()
                                .text_color(es.editor_path_bar_separator_color),
                        )
                        .child(
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .opacity(item_opacity)
                                .child(target_label),
                        ),
                )
            })
            .when(self.active_tab_supports_text_search(), |this| {
                this.child(
                    div()
                        .pl(px(action_gap))
                        .pr(px(self.scaled(es.editor_path_bar_action_padding_right_px)))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .id("editor-path-bar-search")
                                .w(px(action_size))
                                .h(px(action_size))
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .cursor(CursorStyle::Arrow)
                                .opacity(es.editor_path_bar_action_opacity)
                                .hover(move |style| style.opacity(action_hover_opacity))
                                .child(
                                    svg()
                                        .path(FILETREE_SEARCH_SVG_PATH)
                                        .size(px(action_icon_size))
                                        .text_color(es.editor_path_bar_text_color),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        if this.search_visible {
                                            this.dismiss_search(window, cx);
                                        } else {
                                            this.open_search(false, window, cx);
                                        }
                                    }),
                                ),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_tab_file_indicator(&self, tab: &OpenTab, is_active: bool) -> AnyElement {
        let ts = &self.styles.tab_bar;
        let opacity = if is_active {
            ts.tab_item_icon_active_opacity
        } else {
            ts.tab_item_icon_inactive_opacity
        };
        let icon_size = px(self.scaled(ts.tab_item_icon_size_px));

        if tab.kind == TabKind::Diff {
            return div()
                .min_w(px(self.scaled(ts.tab_item_badge_min_width_px)))
                .h(px(self.scaled(ts.tab_item_badge_height_px)))
                .px(px(self.scaled(ts.tab_item_badge_padding_x_px)))
                .rounded(px(self.scaled(ts.tab_item_badge_radius_px)))
                .flex()
                .items_center()
                .justify_center()
                .bg(ts.tab_item_badge_bg)
                .text_size(px(self.scaled(ts.tab_item_badge_font_size_px)))
                .text_color(ts.tab_item_badge_text_color)
                .opacity(opacity)
                .child("DIFF")
                .into_any_element();
        }

        if tab.is_pinned {
            return svg()
                .path("assets/icons/format/pin.svg")
                .size(px(self.scaled(ts.tab_pin_icon_size_px)))
                .text_color(ts.tab_pin_icon_color)
                .opacity(opacity)
                .into_any_element();
        }

        if let Some(path) = tab.path.as_ref()
            && let Some(label) = file_kind_badge_text(path)
        {
            return div()
                .min_w(px(self.scaled(ts.tab_item_badge_min_width_px)))
                .h(px(self.scaled(ts.tab_item_badge_height_px)))
                .px(px(self.scaled(ts.tab_item_badge_padding_x_px)))
                .rounded(px(self.scaled(ts.tab_item_badge_radius_px)))
                .flex()
                .items_center()
                .justify_center()
                .bg(ts.tab_item_badge_bg)
                .text_size(px(self.scaled(ts.tab_item_badge_font_size_px)))
                .text_color(ts.tab_item_badge_text_color)
                .opacity(opacity)
                .child(label)
                .into_any_element();
        }

        if tab
            .path
            .as_ref()
            .is_some_and(|path| !file_kind_has_indicator(path))
        {
            return div()
                .w(icon_size)
                .h(icon_size)
                .flex_none()
                .into_any_element();
        }

        svg()
            .path(MARKDOWN_SVG_PATH)
            .size(icon_size)
            .text_color(ts.tab_item_icon_color)
            .opacity(opacity)
            .into_any_element()
    }

    pub(crate) fn render_tab(&self, tab: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        let is_active = self.active_tab_id.as_deref() == Some(tab.id.as_str());
        let is_dirty = tab.is_dirty;
        let is_deleted = tab.is_deleted;
        let is_preview = tab.is_preview;
        let tab_kind = tab.kind;
        let tab_index = self
            .tabs
            .iter()
            .position(|candidate| candidate.id == tab.id)
            .unwrap_or(0);
        let tab_id = tab.id.clone();
        let close_id = tab.id.clone();
        let middle_close_id = tab.id.clone();
        let context_tab_id = tab.id.clone();
        let drag_tab_id = tab.id.clone();
        let press_tab_id = tab.id.clone();
        let hover_tab_id = tab.id.clone();
        let drop_tab_id = tab.id.clone();
        let drag_title = tab.title.clone();
        let ts = &self.styles.tab_bar;
        let tab_hover_bg = ts.tab_item_hover_bg;
        let close_hover_bg = ts.tab_item_close_hover_bg;
        let drop_indicator_color = ts.tab_drop_indicator_color;
        let drop_indicator_width = ts.tab_drop_indicator_width_px;
        let source_opacity = ts.tab_drag_source_opacity;
        let tab_surface_height = self.tab_bar_height();
        let tab_bg = if is_active {
            ts.tab_item_active_bg
        } else {
            ts.tab_item_bg
        };
        let is_being_dragged = self.tab_dragging_id.as_deref() == Some(tab.id.as_str());
        let current_tab_ids: Vec<String> = self
            .tabs
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        let settings_tab_id = self
            .tabs
            .iter()
            .find(|candidate| candidate.kind == TabKind::Settings)
            .map(|candidate| candidate.id.clone());
        div()
            .id(SharedString::from(format!("tab-{}", tab.id)))
            .group("tab-item")
            .relative()
            .h(px(self.tab_bar_height()))
            .min_w(px(self.scaled(ts.tab_item_min_width_px)))
            .max_w(px(self.scaled(ts.tab_item_max_width_px)))
            .flex_none()
            // Dim the source tab while dragging
            .when(is_being_dragged, |this: gpui::Stateful<gpui::Div>| {
                this.opacity(source_opacity)
            })
            // Drop indicator: show a single insertion line on the side where the tab will land.
            .drag_over::<DraggedTab>({
                let drop_tab_id = drop_tab_id.clone();
                move |style, dragged, _, _| {
                    let drop_target = if tab_index <= dragged.source_index {
                        TabDropTarget::Before(drop_tab_id.as_str())
                    } else {
                        TabDropTarget::After(drop_tab_id.as_str())
                    };
                    if !Self::tab_drop_changes_order_for(
                        &current_tab_ids,
                        settings_tab_id.as_deref(),
                        &dragged.tab_id,
                        drop_target,
                    ) {
                        return style;
                    }
                    if matches!(drop_target, TabDropTarget::Before(_)) {
                        style
                            .border_l(px(drop_indicator_width))
                            .border_r(px(0.0))
                            .border_color(drop_indicator_color)
                    } else {
                        style
                            .border_l(px(0.0))
                            .border_r(px(drop_indicator_width))
                            .border_color(drop_indicator_color)
                    }
                }
            })
            // Handle drop: reorder on the indicated side of the hovered tab.
            .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                let place_after = this
                    .tabs
                    .iter()
                    .position(|candidate| candidate.id == dragged.tab_id)
                    .zip(
                        this.tabs
                            .iter()
                            .position(|candidate| candidate.id == drop_tab_id),
                    )
                    .is_some_and(|(from_idx, to_idx)| from_idx < to_idx);
                let current_tab_ids: Vec<String> = this
                    .tabs
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect();
                let settings_tab_id = this
                    .tabs
                    .iter()
                    .find(|candidate| candidate.kind == TabKind::Settings)
                    .map(|candidate| candidate.id.clone());
                let drop_target = if place_after {
                    TabDropTarget::After(drop_tab_id.as_str())
                } else {
                    TabDropTarget::Before(drop_tab_id.as_str())
                };
                if !Self::tab_drop_changes_order_for(
                    &current_tab_ids,
                    settings_tab_id.as_deref(),
                    &dragged.tab_id,
                    drop_target,
                ) {
                    this.tab_drag_candidate_id = None;
                    this.tab_dragging_id = None;
                    return;
                }
                if place_after {
                    this.reorder_tab_after(&dragged.tab_id, &drop_tab_id, cx);
                } else {
                    this.reorder_tab_before(&dragged.tab_id, &drop_tab_id, cx);
                }
                this.tab_drag_candidate_id = None;
                this.tab_dragging_id = None;
            }))
            .child(
                div()
                    .relative()
                    .h(px(tab_surface_height))
                    .pl(px(self.scaled(ts.tab_item_padding_left_px)))
                    .pr(px(self.scaled(ts.tab_item_padding_right_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ts.tab_item_gap_px)))
                    .bg(tab_bg)
                    .border_r(px(ts.tab_item_border_width_px))
                    .border_color(ts.tab_item_border_color)
                    .hover(move |style| style.bg(tab_hover_bg))
                    .when(true, |this| {
                        this.when(tab_kind == TabKind::Editor, |this| {
                            this.child(self.render_tab_file_indicator(tab, is_active))
                        })
                        .when(tab_kind == TabKind::Diff, |this| {
                            this.child(self.render_tab_file_indicator(tab, is_active))
                        })
                        .when(tab_kind == TabKind::Settings, |this| {
                            this.child(
                                svg()
                                    .path("assets/icons/setting.svg")
                                    .size(px(self.scaled(ts.tab_item_icon_size_px)))
                                    .text_color(ts.tab_item_icon_color)
                                    .opacity(if is_active {
                                        ts.tab_item_icon_active_opacity
                                    } else {
                                        ts.tab_item_icon_inactive_opacity
                                    }),
                            )
                        })
                        .child(self.render_tab_title_label(tab, is_active, is_preview, is_deleted))
                        .child(
                            div()
                                .id(SharedString::from(format!("close-{}", tab.id)))
                                .w(px(self.scaled(ts.tab_item_close_size_px)))
                                .h(px(self.scaled(ts.tab_item_close_size_px)))
                                .rounded_full()
                                .flex()
                                .flex_none()
                                .justify_center()
                                .items_center()
                                .when(is_dirty, |this| {
                                    this.child(
                                        div()
                                            .w(px(self.scaled(ts.tab_item_dirty_dot_size_px)))
                                            .h(px(self.scaled(ts.tab_item_dirty_dot_size_px)))
                                            .rounded_full()
                                            .bg(ts.tab_item_dirty_dot_color),
                                    )
                                })
                                .when(!is_dirty, |this| {
                                    this.text_size(px(self.scaled(ts.tab_item_close_font_size_px)))
                                        .text_color(ts.tab_item_close_text_color)
                                        .opacity(if is_active {
                                            ts.tab_item_close_active_opacity
                                        } else {
                                            ts.tab_item_close_inactive_opacity
                                        })
                                        .group_hover("tab-item", |style| {
                                            style.opacity(ts.tab_item_close_active_opacity)
                                        })
                                        .hover(move |style| style.bg(close_hover_bg))
                                        .child("✕")
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.request_close_tab(&close_id, cx);
                                    }),
                                ),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(ts.tab_bar_border_width_px))
                            .bg(if is_active {
                                gpui::transparent_black()
                            } else {
                                ts.tab_bar_border_color
                            }),
                    ),
            )
            // Drag initiation: create ghost element
            .on_drag(
                DraggedTab {
                    tab_id: drag_tab_id.clone(),
                    title: drag_title,
                    path: tab.path.clone(),
                    source_index: tab_index,
                },
                {
                    let ghost_font_size_px = self.scaled(self.styles.ui_font_size_px);
                    move |dragged, offset, _window, cx| {
                        cx.new(|_cx| DraggedTabView {
                            title: dragged.title.clone(),
                            font_size_px: ghost_font_size_px,
                            cursor_offset: offset,
                        })
                    }
                },
            )
            .on_drag_move(cx.listener(
                move |this, _event: &DragMoveEvent<DraggedTab>, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.tab_drag_candidate_id = Some(drag_tab_id.clone());
                    this.tab_dragging_id = Some(drag_tab_id.clone());
                },
            ))
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                    if event.dragging()
                        && (this.tab_drag_candidate_id.as_deref() == Some(hover_tab_id.as_str())
                            || this.tab_dragging_id.is_some())
                    {
                        this.tab_dragging_id = Some(hover_tab_id.clone());
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    this.tab_drag_candidate_id = Some(press_tab_id.clone());
                    this.switch_to_tab(&tab_id, cx);
                    if event.click_count >= 2 {
                        this.promote_tab_to_permanent(&tab_id, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.open_tab_context_menu(context_tab_id.clone(), event.position, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.request_close_tab(&middle_close_id, cx);
                }),
            )
            .into_any_element()
    }

    fn render_tab_bar_action_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        is_active: bool,
        is_enabled: bool,
        on_right_click: Option<fn(&mut Self, &mut Context<Self>)>,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ts = &self.styles.tab_bar;
        let button_height = (self.tab_bar_height() - self.scaled(6.0)).max(18.0);
        let icon_size = self.scaled(ts.tab_item_icon_size_px);
        let button_padding_x = self.scaled(ts.tab_action_button_padding_x_px);
        let button_opacity = if is_active {
            ts.tab_item_icon_active_opacity
        } else {
            ts.tab_item_icon_inactive_opacity
        };
        let icon_color = if is_active {
            ts.tab_item_active_text_color
        } else {
            ts.tab_item_icon_color
        };

        div()
            .id(id)
            .w(px(icon_size + button_padding_x * 2.0))
            .h(px(button_height))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .opacity(button_opacity)
            .when(is_enabled, |this| {
                this.hover(move |style| style.opacity(ts.tab_item_icon_active_opacity))
            })
            .cursor(CursorStyle::Arrow)
            .child(
                svg()
                    .path(icon_path)
                    .size(px(icon_size))
                    .text_color(icon_color),
            )
            .when(is_enabled, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        on_click(this, cx);
                    }),
                )
            })
            .when_some(on_right_click.filter(|_| is_enabled), |this, handler| {
                this.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        handler(this, cx);
                    }),
                )
            })
            .into_any_element()
    }

    pub(crate) fn render_tab_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let ts = &self.styles.tab_bar;
        let inner_height = (self.tab_bar_height() - ts.tab_bar_border_width_px).max(0.0);
        let end_drop_indicator_color = ts.tab_drop_indicator_color;
        let end_drop_indicator_width = ts.tab_drop_indicator_width_px;
        let current_tab_ids: Vec<String> = self.tabs.iter().map(|tab| tab.id.clone()).collect();
        let settings_tab_id = self
            .tabs
            .iter()
            .find(|tab| tab.kind == TabKind::Settings)
            .map(|tab| tab.id.clone());
        let mut tab_items: Vec<AnyElement> = self
            .tabs
            .iter()
            .map(|tab| self.render_tab(tab, cx))
            .collect();

        let new_hover_color = ts.tab_new_hover_text_color;
        let new_hover_bg = ts.tab_item_hover_bg;
        let end_drop_target_width = self.scaled(ts.tab_end_drop_target_width_px);
        tab_items.push(
            div()
                .h(px(inner_height))
                .w(px(self.scaled(ts.tab_new_width_px)))
                .flex_none()
                .flex()
                .justify_center()
                .items_center()
                .border_r(px(ts.tab_item_border_width_px))
                .border_color(gpui::transparent_black())
                .text_size(px(self.scaled(ts.tab_new_font_size_px)))
                .text_color(ts.tab_new_text_color)
                .hover(move |style| {
                    style
                        .text_color(new_hover_color)
                        .bg(new_hover_bg)
                        .border_color(ts.tab_item_border_color)
                })
                .child("+")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.create_scratch_tab(cx);
                    }),
                )
                .into_any_element(),
        );
        tab_items.push(
            div()
                .id("tab-bar-end-drop-target")
                .h(px(inner_height))
                .w(px(end_drop_target_width))
                .flex_none()
                .drag_over::<DraggedTab>(move |style, dragged, _, _| {
                    if Self::tab_drop_changes_order_for(
                        &current_tab_ids,
                        settings_tab_id.as_deref(),
                        &dragged.tab_id,
                        TabDropTarget::End,
                    ) {
                        style
                            .border_l(px(end_drop_indicator_width))
                            .border_color(end_drop_indicator_color)
                    } else {
                        style
                    }
                })
                .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                    let current_tab_ids: Vec<String> =
                        this.tabs.iter().map(|tab| tab.id.clone()).collect();
                    let settings_tab_id = this
                        .tabs
                        .iter()
                        .find(|tab| tab.kind == TabKind::Settings)
                        .map(|tab| tab.id.clone());
                    if Self::tab_drop_changes_order_for(
                        &current_tab_ids,
                        settings_tab_id.as_deref(),
                        &dragged.tab_id,
                        TabDropTarget::End,
                    ) {
                        this.reorder_tab_to_end(&dragged.tab_id, cx);
                    }
                    this.tab_drag_candidate_id = None;
                    this.tab_dragging_id = None;
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_tab_bar_drag_space_mouse_down),
                )
                .into_any_element(),
        );

        let chat_active = self.is_utility_panel_active(RightUtilityPanel::Chat);
        let info_active = self.is_utility_panel_active(RightUtilityPanel::Info);
        let git_active = self.is_utility_panel_active(RightUtilityPanel::GitHistory);
        let can_nav_back = self.can_navigate_tab_history_back();
        let can_nav_forward = self.can_navigate_tab_history_forward();
        let history_button_size = self.tab_bar_height();
        let history_icon_size = self.scaled(ts.tab_history_btn_icon_size_px);
        let history_button =
            |id: &'static str,
             icon_path: &'static str,
             enabled: bool,
             handler: fn(&mut Self, &mut Context<Self>)| {
                div()
                    .id(id)
                    .w(px(history_button_size))
                    .h(px(inner_height))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(ts.tab_history_btn_bg)
                    .opacity(if enabled {
                        ts.tab_history_btn_enabled_opacity
                    } else {
                        ts.tab_history_btn_disabled_opacity
                    })
                    .cursor(CursorStyle::Arrow)
                    .when(enabled, |this| {
                        this.hover(move |style| style.bg(ts.tab_history_btn_hover_bg))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    handler(this, cx);
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(history_icon_size))
                            .text_color(if enabled {
                                ts.tab_history_btn_icon_color
                            } else {
                                ts.tab_history_btn_icon_disabled_color
                            }),
                    )
            };
        let left_controls = div()
            .id("tab-bar-left-controls")
            .h(px(inner_height))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(0.0))
            .border_r(px(self.scaled(ts.tab_history_group_border_width_px)))
            .border_color(ts.tab_history_group_border_color)
            .child(history_button(
                "tab-bar-history-back",
                "assets/icons/arrow-left.svg",
                can_nav_back,
                Self::navigate_tab_history_back,
            ))
            .child(history_button(
                "tab-bar-history-forward",
                "assets/icons/arrow-right.svg",
                can_nav_forward,
                Self::navigate_tab_history_forward,
            ))
            .into_any_element();
        let leading_left_margin = if self.sidebar_visible {
            0.0
        } else {
            self.scaled(ts.tab_history_hidden_sidebar_margin_px)
        };
        let left_controls_group = div()
            .id("tab-bar-left-group")
            .h(px(inner_height))
            .flex_none()
            .flex()
            .items_center()
            .ml(px(leading_left_margin))
            .child(left_controls);
        let action_buttons = div()
            .id("tab-bar-actions")
            .h(px(inner_height))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(0.0))
            .pr(px(self.scaled(ts.tab_item_padding_right_px)))
            .child(self.render_tab_bar_action_button(
                "tab-bar-chat-toggle",
                CHAT_PANEL_SVG_PATH,
                chat_active,
                true,
                Some(Self::show_chat_panel_bottom),
                cx,
                |this, cx| {
                    this.toggle_chat_panel(cx);
                },
            ))
            .child(self.render_tab_bar_action_button(
                "tab-bar-info-toggle",
                INFO_PANEL_SVG_PATH,
                info_active,
                true,
                None,
                cx,
                |this, cx| {
                    this.toggle_info_panel(cx);
                },
            ))
            .child(self.render_tab_bar_action_button(
                "tab-bar-git-toggle",
                GIT_HISTORY_SVG_PATH,
                git_active,
                true,
                None,
                cx,
                |this, cx| {
                    this.toggle_git_history_panel(cx);
                },
            ));

        div()
            .id("tab-bar")
            .relative()
            .w_full()
            .h(px(self.tab_bar_height()))
            .flex()
            .items_start()
            .bg(ts.tab_bar_bg)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if this.tab_drag_candidate_id.is_some() || this.tab_dragging_id.is_some() {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if event.dragging()
                    && (this.tab_drag_candidate_id.is_some() || this.tab_dragging_id.is_some())
                {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.tab_drag_candidate_id = None;
                    if this.tab_dragging_id.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.tab_drag_candidate_id = None;
                    if this.tab_dragging_id.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(ts.tab_bar_border_width_px))
                    .bg(ts.tab_bar_border_color),
            )
            .child(left_controls_group)
            .child(
                div()
                    .id("tab-bar-scroll")
                    .flex_1()
                    .h_full()
                    .min_w(px(0.0))
                    .flex()
                    .items_start()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_bar_scroll_handle)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::on_tab_bar_drag_space_mouse_down),
                    )
                    .children(tab_items),
            )
            .child(action_buttons)
            .into_any_element()
    }
}
