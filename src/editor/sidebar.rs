use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, FontWeight, MouseButton, MouseDownEvent,
    SharedString, Transformation, div, img, prelude::*, px, radians, svg,
};

use super::*;

impl EditorView {
    fn update_sidebar_drag_target(
        &mut self,
        new_target: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let previous_target = self.sidebar_drag_hover_target.clone();
        let previous_auto_expanded = self.sidebar_drag_auto_expanded_target.clone();
        self.sidebar_drag_hover_target = new_target.clone();

        if previous_auto_expanded.as_ref() != new_target.as_ref() {
            if let Some(path) = previous_auto_expanded {
                if self.expanded_dirs.remove(&path) {
                    self.invalidate_sidebar_items_cache();
                }
            }
            self.sidebar_drag_auto_expanded_target = None;
        }

        if previous_target != new_target {
            cx.notify();
        }
    }

    fn clear_sidebar_drag_state(&mut self, cx: &mut Context<Self>) {
        self.sidebar_drag_expand_task = None;
        self.update_sidebar_drag_target(None, cx);
    }

    pub(crate) fn invalidate_sidebar_items_cache(&mut self) {
        self.sidebar_items_dirty = true;
    }

    fn reset_sidebar_render_limit(&mut self) {
        self.sidebar_render_limit = 200;
    }

    fn maybe_extend_sidebar_items(&mut self, total_items: usize, cx: &mut Context<Self>) {
        if self.sidebar_edit.is_some() || total_items <= self.sidebar_render_limit {
            return;
        }

        let bottom_item = self.sidebar_scroll_handle.bottom_item();
        if bottom_item + 12 >= self.sidebar_render_limit.saturating_sub(1) {
            self.sidebar_render_limit = (self.sidebar_render_limit + 200).min(total_items);
            cx.notify();
        }
    }

    fn sidebar_edit_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        let sidebar_styles = &self.styles.sidebar;
        styles.text_input_height_px = sidebar_styles.sidebar_edit_input_height_px;
        styles.text_input_padding_left_px = sidebar_styles.sidebar_edit_input_padding_x_px;
        styles.text_input_padding_right_px = sidebar_styles.sidebar_edit_input_padding_x_px;
        styles.text_input_font_size_px = sidebar_styles.sidebar_edit_input_font_size_px;
        styles.text_input_bg = sidebar_styles.sidebar_edit_input_bg;
        styles.text_input_focus_bg = sidebar_styles.sidebar_edit_input_bg;
        styles.text_input_border_width_px = sidebar_styles.sidebar_edit_input_border_width_px;
        styles.text_input_border_color = sidebar_styles.sidebar_edit_input_border_color;
        styles.text_input_focus_border_color = sidebar_styles.sidebar_edit_input_border_color;
        styles.text_input_radius_px = sidebar_styles.sidebar_edit_input_radius_px;
        styles.text_input_text_color = sidebar_styles.sidebar_edit_input_text_color;
        styles.text_input_placeholder_color = sidebar_styles.sidebar_edit_input_placeholder_color;
        styles.text_input_cursor_height_ratio =
            sidebar_styles.sidebar_edit_input_cursor_height_ratio;
        styles
    }

    fn compare_sidebar_entries(
        &self,
        left: &std::fs::DirEntry,
        right: &std::fs::DirEntry,
    ) -> std::cmp::Ordering {
        let left_is_dir = left.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let right_is_dir = right.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        right_is_dir
            .cmp(&left_is_dir)
            .then_with(|| match self.sidebar_sort_mode {
                SidebarSortMode::NameAsc => left
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&right.file_name().to_string_lossy().to_lowercase()),
                SidebarSortMode::NameDesc => right
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&left.file_name().to_string_lossy().to_lowercase()),
                SidebarSortMode::Type => {
                    let left_ext = left
                        .path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let right_ext = right
                        .path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    left_ext.cmp(&right_ext).then_with(|| {
                        left.file_name()
                            .to_string_lossy()
                            .to_lowercase()
                            .cmp(&right.file_name().to_string_lossy().to_lowercase())
                    })
                }
                SidebarSortMode::CreatedDesc | SidebarSortMode::CreatedAsc => {
                    let fallback = SystemTime::UNIX_EPOCH;
                    let left_created = left
                        .metadata()
                        .and_then(|meta| meta.created().or_else(|_| meta.modified()))
                        .unwrap_or(fallback);
                    let right_created = right
                        .metadata()
                        .and_then(|meta| meta.created().or_else(|_| meta.modified()))
                        .unwrap_or(fallback);
                    let order = left_created.cmp(&right_created);
                    let order = if self.sidebar_sort_mode == SidebarSortMode::CreatedDesc {
                        order.reverse()
                    } else {
                        order
                    };
                    order.then_with(|| {
                        left.file_name()
                            .to_string_lossy()
                            .to_lowercase()
                            .cmp(&right.file_name().to_string_lossy().to_lowercase())
                    })
                }
            })
    }

    pub(crate) fn toggle_sidebar_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.expanded_dirs.insert(path.clone()) {
            self.expanded_dirs.remove(&path);
        }
        self.invalidate_sidebar_items_cache();
        self.reset_sidebar_render_limit();
        cx.notify();
    }

    pub(crate) fn collapse_all_dirs(&mut self, cx: &mut Context<Self>) {
        self.expanded_dirs.clear();
        self.invalidate_sidebar_items_cache();
        self.reset_sidebar_render_limit();
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn expand_all_dirs(&mut self, cx: &mut Context<Self>) {
        fn collect_dirs(dir: &Path, out: &mut HashSet<PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && !path
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                    {
                        out.insert(path.clone());
                        collect_dirs(&path, out);
                    }
                }
            }
        }
        let root = self.sidebar_root.clone();
        collect_dirs(&root, &mut self.expanded_dirs);
        self.invalidate_sidebar_items_cache();
        self.reset_sidebar_render_limit();
        self.context_menu = None;
        cx.notify();
    }

    fn is_sidebar_item_selected(&self, item_path: &Path) -> bool {
        self.selected_sidebar_paths.contains(item_path)
    }

    /// Core multi-select logic: called on click with modifier keys.
    /// - No modifier: clear selection, select only this item, set anchor
    /// - Cmd (platform): toggle this item in selection, update anchor
    /// - Shift: range-select from anchor to this item
    /// - Cmd+Shift: add range to existing selection
    fn sidebar_select_item(&mut self, path: &Path, shift: bool, cmd: bool) {
        let items = self.sidebar_items();
        let path_buf = path.to_path_buf();

        if shift {
            // Range select from anchor
            let anchor = self
                .sidebar_selection_anchor
                .clone()
                .unwrap_or_else(|| path_buf.clone());
            let anchor_idx = items.iter().position(|i| i.path == anchor);
            let target_idx = items.iter().position(|i| i.path == path_buf);
            if let (Some(a), Some(t)) = (anchor_idx, target_idx) {
                let (start, end) = if a <= t { (a, t) } else { (t, a) };
                if !cmd {
                    self.selected_sidebar_paths.clear();
                }
                for i in start..=end {
                    self.selected_sidebar_paths.insert(items[i].path.clone());
                }
            } else {
                if !cmd {
                    self.selected_sidebar_paths.clear();
                }
                self.selected_sidebar_paths.insert(path_buf.clone());
                self.sidebar_selection_anchor = Some(path_buf.clone());
            }
        } else if cmd {
            // Toggle single item
            if self.selected_sidebar_paths.contains(&path_buf) {
                self.selected_sidebar_paths.remove(&path_buf);
            } else {
                self.selected_sidebar_paths.insert(path_buf.clone());
            }
            self.sidebar_selection_anchor = Some(path_buf.clone());
        } else {
            // Normal click: single select
            self.selected_sidebar_paths.clear();
            self.selected_sidebar_paths.insert(path_buf.clone());
            self.sidebar_selection_anchor = Some(path_buf.clone());
        }

        self.focused_sidebar_path = Some(path_buf);
    }

    /// Filter out nested paths for drag operations.
    /// If both /a and /a/b are selected, only /a is kept.
    fn filter_nested_paths(paths: &HashSet<PathBuf>) -> Vec<PathBuf> {
        let mut sorted: Vec<PathBuf> = paths.iter().cloned().collect();
        sorted.sort();
        let mut result = Vec::new();
        for path in &sorted {
            let dominated = result
                .iter()
                .any(|ancestor: &PathBuf| path.starts_with(ancestor));
            if !dominated {
                result.push(path.clone());
            }
        }
        result
    }

    /// Move sidebar items to target directory (for drag-and-drop).
    pub(crate) fn move_sidebar_items(
        &mut self,
        paths: Vec<PathBuf>,
        target_dir: PathBuf,
        cx: &mut Context<Self>,
    ) {
        for src in &paths {
            let Some(name) = src.file_name() else {
                continue;
            };
            let dst = target_dir.join(name);
            if src == &dst || dst.starts_with(src) {
                continue; // skip self or descendant
            }
            if let Err(e) = std::fs::rename(src, &dst) {
                eprintln!("Move failed: {e}");
                continue;
            }
            // Update open tabs
            for tab in &mut self.tabs {
                if let Some(ref tab_path) = tab.path {
                    if tab_path.starts_with(src) {
                        let relative = tab_path.strip_prefix(src).unwrap_or(tab_path.as_path());
                        let new_tab_path = dst.join(relative);
                        tab.title = Self::title_for_path(&new_tab_path);
                        tab.path = Some(new_tab_path);
                    }
                }
            }
            // Update selection
            let old_selected: Vec<PathBuf> = self
                .selected_sidebar_paths
                .iter()
                .filter(|p| p.starts_with(src))
                .cloned()
                .collect();
            for old in old_selected {
                self.selected_sidebar_paths.remove(&old);
                let relative = old.strip_prefix(src).unwrap_or(old.as_path());
                self.selected_sidebar_paths.insert(dst.join(relative));
            }
            if self
                .current_file_path
                .as_ref()
                .is_some_and(|p| p.starts_with(src))
            {
                if let Some(ref p) = self.current_file_path {
                    let relative = p.strip_prefix(src).unwrap_or(p.as_path());
                    self.current_file_path = Some(dst.join(relative));
                }
            }
        }
        self.context_menu = None;
        self.invalidate_sidebar_items_cache();
        self.clear_sidebar_drag_state(cx);
        cx.notify();
    }

    pub(crate) fn open_sidebar_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.open_path_in_permanent_tab(path, true, cx);
    }

    pub(crate) fn sorted_dir_entries(
        &self,
        path: &Path,
    ) -> std::io::Result<Vec<std::fs::DirEntry>> {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(Result::ok).collect();
        entries.sort_by(|left, right| self.compare_sidebar_entries(left, right));
        Ok(entries)
    }

    pub(crate) fn collect_sidebar_items(
        &self,
        path: &Path,
        depth: usize,
        items: &mut Vec<SidebarItem>,
    ) {
        let Ok(entries) = self.sorted_dir_entries(path) else {
            return;
        };

        for entry in entries {
            let entry_path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            if should_skip_path(&entry_path, is_dir) {
                continue;
            }
            let is_expanded = is_dir && self.expanded_dirs.contains(&entry_path);
            items.push(SidebarItem {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry_path.clone(),
                depth,
                is_dir,
                is_expanded,
            });
            if is_expanded {
                self.collect_sidebar_items(&entry_path, depth + 1, items);
            }
        }
    }

    pub(crate) fn sidebar_items(&mut self) -> Vec<SidebarItem> {
        if self.sidebar_items_dirty {
            let mut items = Vec::new();
            if self.sidebar_error.is_none() && self.sidebar_root.exists() {
                self.collect_sidebar_items(&self.sidebar_root, 1, &mut items);
            }
            self.sidebar_items_cache = items;
            self.sidebar_items_dirty = false;
        }

        self.sidebar_items_cache.clone()
    }

    // ── Sidebar edit operations ─────────────────────────────────────

    pub(crate) fn start_rename(
        &mut self,
        path: PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let current_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let styles = self.sidebar_edit_text_input_styles();
        let editor = cx.new(|cx| {
            let mut ti = crate::text_input::TextInput::new(styles, "名称...", cx);
            ti.set_text(&current_name, cx);
            // Select filename without extension
            let dot_pos = current_name.rfind('.').unwrap_or(current_name.len());
            ti.set_selection(0, dot_pos, cx);
            ti
        });
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        let focus_handle = editor.read(cx).focus_handle.clone();
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.cancel_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(SidebarEditMode::Rename { path, editor });
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn start_create_file(
        &mut self,
        parent: PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.expanded_dirs.insert(parent.clone());
        self.invalidate_sidebar_items_cache();
        let styles = self.sidebar_edit_text_input_styles();
        let editor = cx.new(|cx| crate::text_input::TextInput::new(styles, "文件名...", cx));
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        let focus_handle = editor.read(cx).focus_handle.clone();
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.cancel_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(SidebarEditMode::CreateFile { parent, editor });
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn start_create_dir(
        &mut self,
        parent: PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.expanded_dirs.insert(parent.clone());
        self.invalidate_sidebar_items_cache();
        let styles = self.sidebar_edit_text_input_styles();
        let editor = cx.new(|cx| crate::text_input::TextInput::new(styles, "目录名...", cx));
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        let focus_handle = editor.read(cx).focus_handle.clone();
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.cancel_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(SidebarEditMode::CreateDir { parent, editor });
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn commit_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.sidebar_edit.take() else {
            return;
        };
        self.sidebar_edit_blur_sub = None;
        let value = edit.text(cx);
        match &edit {
            SidebarEditMode::Rename { path, .. } => {
                let trimmed = value.trim();
                if !trimmed.is_empty()
                    && trimmed != path.file_name().and_then(|n| n.to_str()).unwrap_or("")
                {
                    let new_path = path.with_file_name(trimmed);
                    if let Err(e) = std::fs::rename(path, &new_path) {
                        eprintln!("Rename failed: {e}");
                    } else {
                        self.invalidate_sidebar_items_cache();
                        for tab in &mut self.tabs {
                            if tab.path.as_deref() == Some(path.as_path()) {
                                tab.path = Some(new_path.clone());
                                tab.title = Self::title_for_path(&new_path);
                            }
                        }
                        if self.selected_sidebar_paths.contains(path) {
                            self.selected_sidebar_paths.remove(path);
                            self.selected_sidebar_paths.insert(new_path.clone());
                        }
                        if self.current_file_path.as_deref() == Some(path.as_path()) {
                            self.current_file_path = Some(new_path);
                        }
                    }
                }
            }
            SidebarEditMode::CreateFile { parent, .. } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let file_name = if Path::new(trimmed)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| !ext.is_empty())
                    {
                        trimmed.to_string()
                    } else {
                        format!("{trimmed}.md")
                    };
                    let new_path = parent.join(file_name);
                    if let Err(e) = std::fs::File::create(&new_path) {
                        eprintln!("Create file failed: {e}");
                    } else {
                        self.invalidate_sidebar_items_cache();
                        self.open_sidebar_file(new_path, cx);
                        return;
                    }
                }
            }
            SidebarEditMode::CreateDir { parent, .. } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let new_path = parent.join(trimmed);
                    if let Err(e) = std::fs::create_dir(&new_path) {
                        eprintln!("Create folder failed: {e}");
                    } else {
                        self.expanded_dirs.insert(new_path);
                        self.invalidate_sidebar_items_cache();
                    }
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn cancel_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_edit.take().is_some() {
            self.sidebar_edit_blur_sub = None;
            cx.notify();
        }
    }

    pub(crate) fn delete_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(e) = result {
            eprintln!("Delete failed: {e}");
        } else {
            self.invalidate_sidebar_items_cache();
            self.tabs
                .retain(|tab| tab.path.as_ref().is_none_or(|p| !p.starts_with(&path)));
            if self.tabs.is_empty() {
                self.create_scratch_tab(cx);
                return;
            }
            let active_still_exists = self
                .active_tab_id
                .as_ref()
                .is_some_and(|id| self.tabs.iter().any(|t| &t.id == id));
            if !active_still_exists {
                self.activate_tab_by_index(0, cx);
            }
        }
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn paste_into_dir(&mut self, target_dir: PathBuf, cx: &mut Context<Self>) {
        let Some(clipboard) = self.file_clipboard.take() else {
            return;
        };
        for src in &clipboard.paths {
            let Some(name) = src.file_name() else {
                continue;
            };
            let dst = target_dir.join(name);
            if clipboard.is_cut {
                if let Err(e) = std::fs::rename(src, &dst) {
                    eprintln!("Move failed: {e}");
                } else {
                    for tab in &mut self.tabs {
                        if tab.path.as_deref() == Some(src.as_path()) {
                            tab.path = Some(dst.clone());
                            tab.title = Self::title_for_path(&dst);
                        }
                    }
                }
            } else {
                if src.is_file() {
                    if let Err(e) = std::fs::copy(src, &dst) {
                        eprintln!("Copy failed: {e}");
                    }
                }
            }
        }
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn reveal_in_finder(&self, path: &Path) {
        if let Err(e) = std::process::Command::new("open")
            .arg("--reveal")
            .arg(path)
            .spawn()
        {
            eprintln!("failed to reveal in Finder: {e}");
        }
    }

    pub(crate) fn pick_workspace_folder(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_folder_blocking() })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    if !editor.workspaces.contains(&path) {
                        editor.workspaces.push(path.clone());
                    }
                    editor.switch_workspace(path, cx);
                    session_store::persist_session(editor);
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn switch_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.sidebar_root = path.clone();
        self.sidebar_visible = true;
        self.sidebar_resizing = false;
        self.expanded_dirs.clear();
        self.expanded_dirs.insert(path);
        self.selected_sidebar_paths.clear();
        self.focused_sidebar_path = None;
        self.sidebar_selection_anchor = None;
        self.sidebar_error = None;
        self.invalidate_sidebar_items_cache();
        self.reset_sidebar_render_limit();
        self.context_menu = None;
        self.start_watching(cx);
        cx.notify();
        session_store::persist_session(self);
    }

    pub(crate) fn close_workspace(&mut self, path: &Path, cx: &mut Context<Self>) {
        // Find the index of the workspace being closed so we can switch to the next one
        let closing_idx = self.workspaces.iter().position(|w| w == path);
        self.workspaces.retain(|w| w != path);
        if self.workspaces.is_empty() {
            self.sidebar_root = PathBuf::new();
            self.sidebar_visible = false;
            self.sidebar_resizing = false;
            self.expanded_dirs.clear();
            self.selected_sidebar_paths.clear();
            self.focused_sidebar_path = None;
            self.sidebar_selection_anchor = None;
            self.sidebar_error = None;
            self.invalidate_sidebar_items_cache();
            self.context_menu = None;
            self._watcher_task = None;
            cx.notify();
            session_store::persist_session(self);
            return;
        }
        if self.sidebar_root == path {
            // Switch to the next workspace (or last if we were at the end)
            let next_idx = closing_idx
                .unwrap_or(0)
                .min(self.workspaces.len().saturating_sub(1));
            let next = self.workspaces[next_idx].clone();
            self.switch_workspace(next, cx);
        } else {
            self.context_menu = None;
            cx.notify();
        }
        session_store::persist_session(self);
    }

    pub(crate) fn open_workspace_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Workspace,
        });
        cx.notify();
    }

    pub(crate) fn open_sidebar_sort_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::SidebarSort,
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    pub(crate) fn open_sidebar_create_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::SidebarCreate,
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    fn render_sidebar_name_label(&self, name: &str, color: gpui::Hsla) -> AnyElement {
        let ss = &self.styles.sidebar;
        let (base, suffix) = split_file_name_for_display(name);
        div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .flex()
            .gap(px(0.0))
            .items_center()
            .text_size(px(self.scaled(ss.sidebar_item_font_size_px)))
            .text_color(color)
            .child(div().whitespace_nowrap().child(base.to_string()))
            .when_some(suffix, |this, suffix| {
                this.child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .opacity(ss.sidebar_item_suffix_opacity)
                        .child(suffix.to_string()),
                )
            })
            .into_any_element()
    }

    fn render_sidebar_edit_indicator(
        &self,
        name: &str,
        is_dir: bool,
        color: gpui::Hsla,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        if is_dir {
            return svg()
                .path(FOLDER_CLOSED_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .text_color(color)
                .into_any_element();
        }

        if let Some(label) = file_kind_badge_text_for_name(name) {
            return div()
                .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
                .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
                .px(px(self.scaled(ss.sidebar_item_badge_padding_x_px)))
                .rounded(px(self.scaled(ss.sidebar_item_badge_radius_px)))
                .flex()
                .items_center()
                .justify_center()
                .bg(ss.sidebar_item_badge_bg)
                .text_size(px(self.scaled(ss.sidebar_item_badge_font_size_px)))
                .text_color(ss.sidebar_item_badge_text_color)
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(label)
                .into_any_element();
        }

        if !file_kind_has_indicator_for_name(name) {
            return svg()
                .path(MARKDOWN_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .text_color(color)
                .into_any_element();
        }

        svg()
            .path(MARKDOWN_SVG_PATH)
            .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
            .text_color(color)
            .into_any_element()
    }

    fn render_sidebar_header_action_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &MouseDownEvent, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let hover_opacity = ss.sidebar_header_action_hover_opacity;
        div()
            .id(id)
            .w(px(self.scaled(ss.sidebar_header_action_btn_size_px)))
            .h(px(self.scaled(ss.sidebar_header_action_btn_size_px)))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::Arrow)
            .opacity(ss.sidebar_header_action_opacity)
            .hover(move |style| style.opacity(hover_opacity))
            .child(
                svg()
                    .path(icon_path)
                    .size(px(self.scaled(ss.sidebar_header_action_icon_size_px)))
                    .text_color(ss.sidebar_header_icon_color),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    handler(this, event, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_sidebar_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = &self.styles.sidebar;
        let root_name = self.sidebar_root_name().to_uppercase();
        let header_hover_bg = ss.sidebar_header_hover_bg;
        let header_drag_over_bg = ss.sidebar_item_selected_bg;
        let root_drag_over = self
            .sidebar_drag_hover_target
            .as_ref()
            .is_some_and(|target| target == &self.sidebar_root);
        div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_header_height_px)))
            .bg(if root_drag_over { header_drag_over_bg } else { ss.sidebar_header_bg })
            .px(px(self.scaled(ss.sidebar_header_padding_right_px)))
            .pl(px(self.scaled(ss.sidebar_header_padding_left_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_header_gap_px)))
            .flex_none()
            .hover(move |s| s.bg(header_hover_bg))
            .child(
                svg()
                    .path(ROOT_SVG_PATH)
                    .size(px(self.scaled(ss.sidebar_header_icon_size_px)))
                    .text_color(ss.sidebar_header_icon_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(self.scaled(ss.sidebar_header_font_size_px)))
                    .text_color(ss.sidebar_header_text_color)
                    .overflow_hidden()
                    .truncate()
                    .child(root_name),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_header_action_gap_px)))
                    .child(self.render_sidebar_header_action_button(
                        "sidebar-header-search",
                        FILETREE_SEARCH_SVG_PATH,
                        cx,
                        |this, _event, window, cx| {
                            this.toggle_workspace_search(window, cx);
                        },
                    ))
                    .child(self.render_sidebar_header_action_button(
                        "sidebar-header-sort",
                        FILETREE_SORT_SVG_PATH,
                        cx,
                        |this, event, _window, cx| {
                            this.open_sidebar_sort_menu(event.position, cx);
                        },
                    ))
                    .child(self.render_sidebar_header_action_button(
                        "sidebar-header-create",
                        FILETREE_ADD_SVG_PATH,
                        cx,
                        |this, event, _window, cx| {
                            this.open_sidebar_create_menu(event.position, cx);
                        },
                    )),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pick_workspace_folder(cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.open_sidebar_context_menu(
                        this.sidebar_root.clone(),
                        true,
                        event.position,
                        cx,
                    );
                }),
            )
            .drag_over::<DraggedSidebarItems>(move |style, _, _, _| style.bg(header_drag_over_bg))
            .on_drop({
                let root = self.sidebar_root.clone();
                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                    let paths = dragged.paths.clone();
                    this.clear_sidebar_drag_state(cx);
                    this.move_sidebar_items(paths, root.clone(), cx);
                })
            })
            .on_drag_move({
                let root = self.sidebar_root.clone();
                cx.listener(move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                    if event.bounds.contains(&event.event.position) {
                        this.update_sidebar_drag_target(Some(root.clone()), cx);
                    }
                })
            })
            .into_any_element()
    }

    // ── Sidebar keyboard navigation ─────────────────────────────────

    pub(crate) fn sidebar_nav_up(&mut self, cx: &mut Context<Self>) {
        let items = self.sidebar_items();
        if items.is_empty() {
            return;
        }
        let new_path = if let Some(ref path) = self.focused_sidebar_path {
            if let Some(idx) = items.iter().position(|i| &i.path == path) {
                if idx > 0 {
                    Some(items[idx - 1].path.clone())
                } else {
                    Some(items[0].path.clone())
                }
            } else {
                Some(items[0].path.clone())
            }
        } else {
            items.last().map(|i| i.path.clone())
        };
        self.focused_sidebar_path = new_path;
        cx.notify();
    }

    pub(crate) fn sidebar_nav_down(&mut self, cx: &mut Context<Self>) {
        let items = self.sidebar_items();
        if items.is_empty() {
            return;
        }
        let new_path = if let Some(ref path) = self.focused_sidebar_path {
            if let Some(idx) = items.iter().position(|i| &i.path == path) {
                Some(items[(idx + 1).min(items.len() - 1)].path.clone())
            } else {
                Some(items[0].path.clone())
            }
        } else {
            Some(items[0].path.clone())
        };
        self.focused_sidebar_path = new_path;
        cx.notify();
    }

    pub(crate) fn sidebar_nav_left(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(p) => p,
            None => return,
        };
        if path.is_dir() && self.expanded_dirs.contains(&path) {
            self.expanded_dirs.remove(&path);
            self.invalidate_sidebar_items_cache();
            cx.notify();
        } else if let Some(parent) = path.parent() {
            if parent != self.sidebar_root && parent.starts_with(&self.sidebar_root) {
                self.focused_sidebar_path = Some(parent.to_path_buf());
                cx.notify();
            }
        }
    }

    pub(crate) fn sidebar_nav_right(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(p) => p,
            None => return,
        };
        if path.is_dir() {
            if !self.expanded_dirs.contains(&path) {
                self.expanded_dirs.insert(path.clone());
                self.invalidate_sidebar_items_cache();
                cx.notify();
            } else {
                // Already expanded: move focus to first child
                let items = self.sidebar_items();
                if let Some(child) = items.iter().find(|i| i.path.parent() == Some(&path)) {
                    self.focused_sidebar_path = Some(child.path.clone());
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn sidebar_confirm_focused(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(p) => p,
            None => return,
        };
        if self.sidebar_edit.is_some() {
            self.commit_sidebar_edit(cx);
        } else {
            // Set single selection on confirm
            self.sidebar_select_item(&path, false, false);
            if path.is_dir() {
                self.toggle_sidebar_directory(path, cx);
            } else {
                self.open_sidebar_file(path, cx);
            }
        }
    }

    pub(crate) fn sidebar_delete_focused(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(p) => p,
            None => return,
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.show_dialog(
            Dialog {
                title: "删除".to_string(),
                message: format!("确定要删除 \"{}\" 吗？此操作不可撤销。", name),
                save_label: None,
                confirm_label: "删除".to_string(),
                cancel_label: "取消".to_string(),
                is_danger: true,
                action: DialogAction::DeletePath { path },
            },
            cx,
        );
    }

    pub(crate) fn render_sidebar_edit_row(
        &self,
        editor: &gpui::Entity<crate::text_input::TextInput>,
        depth: usize,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let shift = ss.sidebar_item_left_shift_px;
        let left_padding = self.scaled(
            (ss.sidebar_item_indent_base_px + depth as f32 * ss.sidebar_item_indent_step_px
                - shift)
                .max(0.0),
        );
        let current_name = editor.read(cx).text().to_string();

        div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_item_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .bg(ss.sidebar_item_selected_bg)
            .child(
                div()
                    .w_full()
                    .h_full()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_item_gap_px)))
                    .child(
                        div()
                            .w(px(self.scaled(ss.sidebar_item_fold_container_width_px)))
                            .h(px(self.scaled(ss.sidebar_item_fold_container_height_px)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .child(if is_dir {
                                svg()
                                    .path(FILETREE_FOLD_SVG_PATH)
                                    .size(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .text_color(ss.sidebar_item_fold_icon_color)
                                    .into_any_element()
                            } else {
                                div()
                                    .w(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .into_any_element()
                            }),
                    )
                    .child(self.render_sidebar_edit_indicator(
                        &current_name,
                        is_dir,
                        ss.sidebar_item_selected_icon_color,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(editor.clone())),
            )
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {})
            .into_any_element()
    }

    pub(crate) fn render_sidebar_item(
        &self,
        item: SidebarItem,
        active_guide_levels: &HashSet<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(SidebarEditMode::Rename {
            path: ref rp,
            ref editor,
        }) = self.sidebar_edit
        {
            if *rp == item.path {
                return self.render_sidebar_edit_row(editor, item.depth, item.is_dir, cx);
            }
        }

        let is_selected = self.is_sidebar_item_selected(&item.path);
        let path = item.path.clone();
        let is_dir = item.is_dir;
        let ss = &self.styles.sidebar;
        let shift = ss.sidebar_item_left_shift_px;
        let left_padding = self.scaled(
            (ss.sidebar_item_indent_base_px + item.depth as f32 * ss.sidebar_item_indent_step_px
                - shift)
                .max(0.0),
        );
        let row_bg = if is_selected {
            ss.sidebar_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let icon_color = if is_selected {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        };
        let label_color = if is_selected {
            ss.sidebar_item_selected_text_color
        } else {
            ss.sidebar_item_text_color
        };
        let is_focused = self.focused_sidebar_path.as_deref() == Some(path.as_path());
        let focus_border_color = ss.sidebar_item_focus_border_color;
        let active_border_color = ss.sidebar_item_active_border_color;
        let item_hover_bg = ss.sidebar_item_hover_bg;

        // Build indent guide lines (dashed vertical strips for levels with siblings below)
        let guide_color = ss.sidebar_indent_guide_color;
        let guide_width = ss.sidebar_indent_guide_width_px;
        let dash_len = self.scaled(ss.sidebar_indent_guide_dash_length_px);
        let gap_len = self.scaled(ss.sidebar_indent_guide_gap_length_px);
        let indent_base = ss.sidebar_item_indent_base_px;
        let indent_step = ss.sidebar_item_indent_step_px;
        let item_height = self.scaled(ss.sidebar_item_height_px);
        let fold_w = self.scaled(ss.sidebar_item_fold_container_width_px);
        let guides: Vec<gpui::AnyElement> = (1..=item.depth)
            .filter(|level| active_guide_levels.contains(level))
            .flat_map(|level| {
                let guide_x = self
                    .scaled((indent_base + level as f32 * indent_step - shift).max(0.0))
                    + fold_w / 2.0
                    - guide_width / 2.0;
                let segment_step = dash_len + gap_len;
                let count = (item_height / segment_step).ceil() as usize;
                (0..count).map(move |i| {
                    let y = i as f32 * segment_step;
                    let h = dash_len.min(item_height - y);
                    div()
                        .absolute()
                        .left(px(guide_x))
                        .top(px(y))
                        .w(px(guide_width))
                        .h(px(h))
                        .bg(guide_color)
                        .into_any_element()
                })
            })
            .collect();

        let is_drag_over = self
            .sidebar_drag_hover_target
            .as_ref()
            .is_some_and(|target| target == &path || path.starts_with(target));
        let drag_over_bg = ss.sidebar_item_selected_bg;

        // Prepare drag payload: if item is selected, drag all; otherwise drag just this one
        let drag_paths: Vec<PathBuf> = if self.selected_sidebar_paths.contains(&path) {
            Self::filter_nested_paths(&self.selected_sidebar_paths)
        } else {
            vec![path.clone()]
        };
        let drag_label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let drag_count = drag_paths.len();
        let indicator = if item.is_dir {
            svg()
                .path(if item.is_expanded {
                    FOLDER_OPEN_SVG_PATH
                } else {
                    FOLDER_CLOSED_SVG_PATH
                })
                .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .text_color(icon_color)
                .into_any_element()
        } else if let Some(label) = file_kind_badge_text(&item.path) {
            div()
                .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
                .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
                .px(px(self.scaled(ss.sidebar_item_badge_padding_x_px)))
                .rounded(px(self.scaled(ss.sidebar_item_badge_radius_px)))
                .flex()
                .items_center()
                .justify_center()
                .bg(ss.sidebar_item_badge_bg)
                .text_size(px(self.scaled(ss.sidebar_item_badge_font_size_px)))
                .text_color(ss.sidebar_item_badge_text_color)
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(label)
                .into_any_element()
        } else if !file_kind_has_indicator(&item.path) {
            div()
                .w(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .h(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .flex_none()
                .into_any_element()
        } else {
            svg()
                .path(MARKDOWN_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .text_color(icon_color)
                .into_any_element()
        };

        div()
            .id(SharedString::from(format!("sidebar-{}", path.display())))
            .w_full()
            .h(px(item_height))
            .flex_none()
            .relative()
            .flex()
            .items_center()
            .bg(if is_drag_over { drag_over_bg } else { row_bg })
            .border_1()
            .border_color(if is_selected {
                active_border_color
            } else if is_focused {
                focus_border_color
            } else {
                gpui::transparent_black()
            })
            .when(!self.sidebar_resizing && !self.outline_resizing, |this| {
                this.hover(move |style| style.bg(item_hover_bg))
            })
            .children(guides)
            .child(
                div()
                    .w_full()
                    .h_full()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_item_gap_px)))
                    .child(
                        div()
                            .w(px(self.scaled(ss.sidebar_item_fold_container_width_px)))
                            .h(px(self.scaled(ss.sidebar_item_fold_container_height_px)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .child(if item.is_dir {
                                svg()
                                    .path(FILETREE_FOLD_SVG_PATH)
                                    .size(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .text_color(ss.sidebar_item_fold_icon_color)
                                    .when(item.is_expanded, |s| {
                                        s.with_transformation(Transformation::rotate(radians(
                                            std::f32::consts::FRAC_PI_2,
                                        )))
                                    })
                                    .into_any_element()
                            } else {
                                div()
                                    .w(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .into_any_element()
                            }),
                    )
                    .child(indicator)
                    .child(self.render_sidebar_name_label(&item.name, label_color)),
            )
            // Multi-select click with modifier keys
            .on_click({
                let path = path.clone();
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    let mods = event.modifiers();
                    let has_modifier = mods.platform || mods.shift;
                    this.sidebar_select_item(&path, mods.shift, mods.platform);
                    // Only open/toggle on plain click (no modifier)
                    if !has_modifier {
                        if path.is_dir() {
                            this.toggle_sidebar_directory(path.clone(), cx);
                        } else if event.click_count() >= 2 {
                            this.open_path_in_permanent_tab(path.clone(), true, cx);
                            this.move_cursor_to_first_line_end();
                        } else {
                            if !this.switch_to_existing_editor_tab_for_path(&path, true, cx) {
                                this.open_path_in_preview_tab(path.clone(), true, cx);
                            }
                        }
                    }
                    window.prevent_default();
                    cx.notify();
                })
            })
            // Drag initiation (disabled during resize)
            .when(!self.sidebar_resizing && !self.outline_resizing, |el| {
                el.on_drag(DraggedSidebarItems { paths: drag_paths }, {
                    let label = drag_label;
                    let count = drag_count;
                    let ghost_font_size_px = self.scaled(self.styles.ui_font_size_px);
                    move |_dragged, _offset, _window, cx| {
                        cx.new(|_cx| DraggedSidebarItemsView {
                            label: label.clone(),
                            count,
                            font_size_px: ghost_font_size_px,
                        })
                    }
                })
            })
            // Drop target: only directories accept drops
            .when(is_dir, |el| {
                let drop_path = path.clone();
                el.drag_over::<DraggedSidebarItems>(move |style, _dragged, _, _| {
                    style.bg(drag_over_bg)
                })
                .on_drop(cx.listener(
                    move |this, dragged: &DraggedSidebarItems, _, cx| {
                        let paths = dragged.paths.clone();
                        this.clear_sidebar_drag_state(cx);
                        this.move_sidebar_items(paths, drop_path.clone(), cx);
                    },
                ))
            })
            // Drag-move: track hover target for auto-expand
            .on_drag_move(cx.listener({
                let path = path.clone();
                move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }

                    if !is_dir {
                        this.update_sidebar_drag_target(None, cx);
                        return;
                    }

                    if is_dir {
                        let new_target = Some(path.clone());
                        if this.sidebar_drag_hover_target != new_target {
                            this.update_sidebar_drag_target(new_target, cx);
                            // Start auto-expand timer
                            let expand_path = path.clone();
                            this.sidebar_drag_expand_task =
                                Some(cx.spawn(async move |this, cx| {
                                    cx.background_executor()
                                        .timer(Duration::from_millis(500))
                                        .await;
                                    this.update(cx, |editor, cx| {
                                        if editor.sidebar_drag_hover_target.as_ref()
                                            == Some(&expand_path)
                                        {
                                            editor.expanded_dirs.insert(expand_path.clone());
                                            editor.sidebar_drag_auto_expanded_target =
                                                Some(expand_path);
                                            editor.invalidate_sidebar_items_cache();
                                            cx.notify();
                                        }
                                    })
                                    .ok();
                                }));
                            cx.notify();
                        }
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    // If right-clicking on an unselected item, select only that item
                    if !this.selected_sidebar_paths.contains(&path) {
                        this.sidebar_select_item(&path, false, false);
                    }
                    this.focused_sidebar_path = Some(path.clone());
                    this.open_sidebar_context_menu(path.clone(), is_dir, event.position, cx);
                }),
            )
            .into_any_element()
    }

    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let has_root = self.sidebar_root.exists() && self.sidebar_error.is_none();
        let show_workspace_bar = true;
        let items = if has_root {
            Some(self.sidebar_items())
        } else {
            None
        };
        let ss = self.styles.sidebar.clone();

        // Fixed header section (spacer + logo + root name)
        let top_spacer = div()
            .id("sidebar-top-spacer")
            .w_full()
            .h(px(self.scaled(ss.sidebar_top_spacer_height_px)))
            .flex_none()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseDownEvent, window, _cx| {
                    if event.click_count >= 2 {
                        window.zoom_window();
                    } else {
                        super::perform_window_drag(window);
                    }
                }),
            );

        let header = self.render_sidebar_header(cx);

        // Scrollable content section
        let mut tree_items: Vec<AnyElement> = Vec::new();

        if let Some(error) = &self.sidebar_error {
            tree_items.push(
                div()
                    .w_full()
                    .px(px(self.scaled(ss.sidebar_error_padding_x_px)))
                    .py(px(self.scaled(ss.sidebar_error_padding_y_px)))
                    .text_size(px(self.scaled(ss.sidebar_error_font_size_px)))
                    .text_color(ss.sidebar_error_text_color)
                    .child(error.clone())
                    .into_any_element(),
            );
        } else if !has_root {
            let empty_hover_bg = ss.sidebar_empty_btn_hover_bg;
            tree_items.push(
                div()
                    .w_full()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px(px(self.scaled(ss.sidebar_empty_padding_x_px)))
                    .child(
                        div()
                            .px(px(self.scaled(ss.sidebar_empty_btn_padding_x_px)))
                            .py(px(self.scaled(ss.sidebar_empty_btn_padding_y_px)))
                            .rounded_full()
                            .border(px(ss.sidebar_empty_btn_border_width_px))
                            .border_dashed()
                            .border_color(ss.sidebar_empty_btn_border_color)
                            .text_size(px(self.scaled(ss.sidebar_empty_btn_font_size_px)))
                            .text_color(ss.sidebar_empty_btn_text_color)
                            .hover(move |s| s.bg(empty_hover_bg))
                            .cursor(CursorStyle::Arrow)
                            .child("打开目录...")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.pick_workspace_folder(cx);
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        } else {
            let create_parent = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { parent, .. })
                | Some(SidebarEditMode::CreateDir { parent, .. }) => Some(parent.clone()),
                _ => None,
            };
            let create_depth = create_parent.as_ref().map(|p| {
                p.strip_prefix(&self.sidebar_root)
                    .map(|rel| rel.components().count() + 1)
                    .unwrap_or(1)
            });
            let create_editor = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { editor, .. })
                | Some(SidebarEditMode::CreateDir { editor, .. }) => Some(editor.clone()),
                _ => None,
            };
            let create_is_dir =
                matches!(&self.sidebar_edit, Some(SidebarEditMode::CreateDir { .. }));

            let items = items.unwrap_or_default();
            let render_all_items = self.sidebar_edit.is_some();
            let item_limit = if render_all_items {
                items.len()
            } else {
                self.sidebar_render_limit.min(items.len())
            };

            // Pre-compute which indent levels have a sibling below for each item.
            // A guide at level L is shown for item[i] if there exists a later item[j] (j > i)
            // with depth == L (a sibling at that level), meaning the vertical line continues.
            // We walk backwards to efficiently compute this.
            let mut next_at_depth: HashMap<usize, usize> = HashMap::new();
            let mut item_guide_levels: Vec<HashSet<usize>> = vec![HashSet::new(); items.len()];
            for i in (0..items.len()).rev() {
                let depth = items[i].depth;
                next_at_depth.insert(depth, i);
                // For item[i], collect all levels L in 1..depth where
                // the next item at level L exists after i
                let mut levels = HashSet::new();
                for level in 1..=depth {
                    if let Some(&next_idx) = next_at_depth.get(&level) {
                        if next_idx > i {
                            levels.insert(level);
                        }
                    }
                }
                item_guide_levels[i] = levels;
            }

            let mut injected = false;
            if create_parent.as_ref() == Some(&self.sidebar_root) {
                if let Some(ref editor) = create_editor {
                    tree_items.push(self.render_sidebar_edit_row(editor, 1, create_is_dir, cx));
                    injected = true;
                }
            }
            for (idx, item) in items.iter().take(item_limit).enumerate() {
                tree_items.push(self.render_sidebar_item(
                    item.clone(),
                    &item_guide_levels[idx],
                    cx,
                ));
                if let Some(ref parent) = create_parent {
                    if !injected && item.is_dir && &item.path == parent {
                        if let Some(ref editor) = create_editor {
                            let depth = create_depth.unwrap_or(1);
                            tree_items.push(self.render_sidebar_edit_row(
                                editor,
                                depth,
                                create_is_dir,
                                cx,
                            ));
                            injected = true;
                        }
                    }
                }
            }
            if render_all_items && !injected {
                if let (Some(depth), Some(editor)) = (create_depth, &create_editor) {
                    tree_items.push(self.render_sidebar_edit_row(editor, depth, create_is_dir, cx));
                }
            }
        }

        // Logo goes inside scrollable area so it scrolls away
        tree_items.insert(
            0,
            div()
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .py(px(self.scaled(ss.sidebar_logo_padding_y_px)))
                .child(
                    img(helpers::resolve_asset_image_path(LOGO_SVG_PATH))
                        .w(px(self.scaled(ss.sidebar_logo_size_px)))
                        .h(px(self.scaled(ss.sidebar_logo_size_px))),
                )
                .into_any_element(),
        );

        // Header inside scrollable area (right after logo, before items)
        if has_root {
            tree_items.insert(1, header);
        }

        tree_items.push(
            div()
                .w_full()
                .h(px(self.scaled(ss.sidebar_bottom_spacer_height_px)))
                .flex_none()
                .into_any_element(),
        );

        // Determine if header should be shown as sticky overlay.
        // The logo area height is how far the header is from the top of scroll content.
        let logo_area_height =
            self.scaled(ss.sidebar_logo_size_px + 2.0 * ss.sidebar_logo_padding_y_px);
        let scroll_y = -f32::from(self.sidebar_scroll_handle.offset().y);
        let header_is_sticky = has_root && scroll_y >= logo_area_height;

        div()
            .id("sidebar")
            .w(px(self.sidebar_width()))
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
            .when(
                self.workspace_search_visible,
                |this: gpui::Stateful<gpui::Div>| this.child(self.render_workspace_search(cx)),
            )
            .when(
                !self.workspace_search_visible,
                |this: gpui::Stateful<gpui::Div>| {
                    this.child(
                        div()
                            .id("sidebar-tree")
                            .flex_1()
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                let total_items = this.sidebar_items().len();
                                this.maybe_extend_sidebar_items(total_items, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.focused_sidebar_path = None;
                                    this.clear_sidebar_drag_state(cx);
                                    cx.notify();
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                {
                                    let root = self.sidebar_root.clone();
                                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        this.open_sidebar_context_menu(
                                            root.clone(),
                                            true,
                                            event.position,
                                            cx,
                                        );
                                    })
                                },
                            )
                            .drag_over::<DraggedSidebarItems>(move |style, _, _, _| style)
                            .on_drag_move({
                                let root = self.sidebar_root.clone();
                                cx.listener(
                                    move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                                        if event.bounds.contains(&event.event.position) {
                                            this.update_sidebar_drag_target(Some(root.clone()), cx);
                                        }
                                    },
                                )
                            })
                            .on_drop({
                                let root = self.sidebar_root.clone();
                                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                                    let paths = dragged.paths.clone();
                                    this.clear_sidebar_drag_state(cx);
                                    this.move_sidebar_items(paths, root.clone(), cx);
                                })
                            })
                            .children(tree_items),
                    )
                },
            )
            .when(
                header_is_sticky && !self.workspace_search_visible,
                |this: gpui::Stateful<gpui::Div>| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(self.scaled(ss.sidebar_top_spacer_height_px)))
                            .w_full()
                            .bg(ss.sidebar_bg)
                            .child(self.render_sidebar_header(cx)),
                    )
                },
            )
            .when(
                show_workspace_bar && !self.workspace_search_visible,
                |this: gpui::Stateful<gpui::Div>| this.child(self.render_workspace_bar(cx)),
            )
            .into_any_element()
    }

    fn render_workspace_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let wss = &self.styles.workspace_switcher;
        let ws_name = self.sidebar_root_name().to_uppercase();
        let hover_bg = wss.ws_bar_hover_bg;
        let root_drag_over = self
            .sidebar_drag_hover_target
            .as_ref()
            .is_some_and(|target| target == &self.sidebar_root);

        div()
            .id("workspace-bar")
            .w_full()
            .h(px(self.scaled(wss.ws_bar_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .px(px(self.scaled(wss.ws_bar_padding_x_px)))
            .gap(px(self.scaled(wss.ws_bar_gap_px)))
            .bg(if root_drag_over {
                hover_bg
            } else {
                wss.ws_bar_bg
            })
            .border_t(px(1.0))
            .border_color(wss.ws_bar_border_color)
            .cursor(CursorStyle::Arrow)
            .hover(move |s| s.bg(hover_bg))
            .child(
                svg()
                    .path(WORKSPACE_EXPAND_SVG_PATH)
                    .size(px(self.scaled(wss.ws_bar_icon_size_px)))
                    .text_color(wss.ws_bar_icon_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_size(px(self.scaled(wss.ws_bar_font_size_px)))
                    .text_color(wss.ws_bar_text_color)
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(ws_name),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let pos = event.position;
                    // Menu should appear above the bar, so offset upward
                    let menu_pos = gpui::point(pos.x, pos.y - gpui::px(20.0));
                    this.open_workspace_menu(menu_pos, cx);
                }),
            )
            .drag_over::<DraggedSidebarItems>(move |style, _, _, _| {
                style.bg(hover_bg)
            })
            .on_drag_move({
                let root = self.sidebar_root.clone();
                cx.listener(move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                    if event.bounds.contains(&event.event.position) {
                        this.update_sidebar_drag_target(Some(root.clone()), cx);
                    }
                })
            })
            .on_drop({
                let root = self.sidebar_root.clone();
                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                    let paths = dragged.paths.clone();
                    this.clear_sidebar_drag_state(cx);
                    this.move_sidebar_items(paths, root.clone(), cx);
                })
            })
            .into_any_element()
    }
}
