use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, FontWeight, MouseButton, MouseDownEvent,
    SharedString, Transformation, div, img, prelude::*, px, radians, svg,
};

use super::*;

impl EditorView {
    pub(crate) fn toggle_sidebar_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.expanded_dirs.insert(path.clone()) {
            self.expanded_dirs.remove(&path);
        }
        cx.notify();
    }

    pub(crate) fn collapse_all_dirs(&mut self, cx: &mut Context<Self>) {
        self.expanded_dirs.clear();
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
        cx.notify();
    }

    pub(crate) fn open_sidebar_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !is_markdown_file(&path) {
            cx.notify();
            return;
        }
        self.open_path_in_permanent_tab(path, true, cx);
    }

    pub(crate) fn sorted_dir_entries(path: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(Result::ok).collect();
        entries.sort_by(|left, right| {
            let left_is_dir = left.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let right_is_dir = right.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            right_is_dir.cmp(&left_is_dir).then_with(|| {
                left.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&right.file_name().to_string_lossy().to_lowercase())
            })
        });
        Ok(entries)
    }

    pub(crate) fn collect_sidebar_items(
        &self,
        path: &Path,
        depth: usize,
        items: &mut Vec<SidebarItem>,
    ) {
        let Ok(entries) = Self::sorted_dir_entries(path) else {
            return;
        };

        for entry in entries {
            let entry_path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            if !is_dir && !is_markdown_file(&entry_path) {
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

    pub(crate) fn sidebar_items(&self) -> Vec<SidebarItem> {
        let mut items = Vec::new();
        if self.sidebar_error.is_none() && self.sidebar_root.exists() {
            self.collect_sidebar_items(&self.sidebar_root, 1, &mut items);
        }
        items
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
        let styles = self.styles.text_input.clone();
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
        let styles = self.styles.text_input.clone();
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
        let styles = self.styles.text_input.clone();
        let editor = cx.new(|cx| crate::text_input::TextInput::new(styles, "文件夹名...", cx));
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
                    let new_path = parent.join(trimmed);
                    if let Err(e) = std::fs::File::create(&new_path) {
                        eprintln!("Create file failed: {e}");
                    } else if is_markdown_file(&new_path) {
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
        self.context_menu = None;
        self.start_watching(cx);
        cx.notify();
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
            self.context_menu = None;
            self._watcher_task = None;
            cx.notify();
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
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let shift = ss.sidebar_item_left_shift_px;
        let left_padding = self.scaled(
            (ss.sidebar_item_indent_base_px + depth as f32 * ss.sidebar_item_indent_step_px
                - shift)
                .max(0.0),
        );
        let icon_path = if is_dir {
            FOLDER_CLOSED_SVG_PATH
        } else {
            MARKDOWN_SVG_PATH
        };

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
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                            .text_color(ss.sidebar_item_selected_icon_color),
                    )
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
        let folder_icon = if item.is_expanded {
            FOLDER_OPEN_SVG_PATH
        } else {
            FOLDER_CLOSED_SVG_PATH
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

        let is_drag_over =
            is_dir && self.sidebar_drag_hover_target.as_deref() == Some(path.as_path());
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
                    .child(
                        svg()
                            .path(if item.is_dir {
                                folder_icon
                            } else {
                                MARKDOWN_SVG_PATH
                            })
                            .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(self.scaled(ss.sidebar_item_font_size_px)))
                            .text_color(label_color)
                            .truncate()
                            .child(item.name),
                    ),
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
                    move |_dragged, _offset, _window, cx| {
                        cx.new(|_cx| DraggedSidebarItemsView {
                            label: label.clone(),
                            count,
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
                        this.sidebar_drag_hover_target = None;
                        this.sidebar_drag_expand_task = None;
                        this.move_sidebar_items(paths, drop_path.clone(), cx);
                    },
                ))
            })
            // Drag-move: track hover target for auto-expand
            .on_drag_move(cx.listener({
                let path = path.clone();
                move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                    if is_dir && event.bounds.contains(&event.event.position) {
                        let new_target = Some(path.clone());
                        if this.sidebar_drag_hover_target != new_target {
                            this.sidebar_drag_hover_target = new_target;
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
                                            editor.expanded_dirs.insert(expand_path);
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

    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_root = self.sidebar_root.exists() && self.sidebar_error.is_none();
        let ss = &self.styles.sidebar;
        let show_workspace_bar = true;

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

        let root_name = self.sidebar_root_name().to_uppercase();
        let header_hover_bg = ss.sidebar_header_hover_bg;
        let header = div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_header_height_px)))
            .bg(ss.sidebar_header_bg)
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
                    .text_size(px(self.scaled(ss.sidebar_header_font_size_px)))
                    .text_color(ss.sidebar_header_text_color)
                    .overflow_hidden()
                    .child(root_name),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pick_workspace_folder(cx);
                }),
            );

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

            let items = self.sidebar_items();

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
            for (idx, item) in items.iter().enumerate() {
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
            if !injected {
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
            tree_items.insert(1, header.into_any_element());
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
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.focused_sidebar_path = None;
                                    cx.notify();
                                }),
                            )
                            .drag_over::<DraggedSidebarItems>(move |style, _, _, _| style)
                            .on_drop({
                                let root = self.sidebar_root.clone();
                                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                                    let paths = dragged.paths.clone();
                                    this.sidebar_drag_hover_target = None;
                                    this.sidebar_drag_expand_task = None;
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
                    let sticky_header_hover_bg = ss.sidebar_header_hover_bg;
                    let sticky_root_name = self.sidebar_root_name().to_uppercase();
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(self.scaled(ss.sidebar_top_spacer_height_px)))
                            .w_full()
                            .h(px(self.scaled(ss.sidebar_header_height_px)))
                            .bg(ss.sidebar_bg)
                            .px(px(self.scaled(ss.sidebar_header_padding_right_px)))
                            .pl(px(self.scaled(ss.sidebar_header_padding_left_px)))
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(ss.sidebar_header_gap_px)))
                            .flex_none()
                            .hover(move |s| s.bg(sticky_header_hover_bg))
                            .child(
                                svg()
                                    .path(ROOT_SVG_PATH)
                                    .size(px(self.scaled(ss.sidebar_header_icon_size_px)))
                                    .text_color(ss.sidebar_header_icon_color),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(self.scaled(ss.sidebar_header_font_size_px)))
                                    .text_color(ss.sidebar_header_text_color)
                                    .overflow_hidden()
                                    .child(sticky_root_name),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.pick_workspace_folder(cx);
                                }),
                            ),
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

        div()
            .id("workspace-bar")
            .w_full()
            .h(px(self.scaled(wss.ws_bar_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .px(px(self.scaled(wss.ws_bar_padding_x_px)))
            .gap(px(self.scaled(wss.ws_bar_gap_px)))
            .bg(wss.ws_bar_bg)
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
            .into_any_element()
    }
}
