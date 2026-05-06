use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use gpui::{Context, Pixels, px};

use super::{profile, *};

impl EditorView {
    pub(super) fn sidebar_item_height_px(&self) -> f32 {
        self.scaled(self.styles.sidebar.sidebar_item_height_px)
    }

    pub(super) fn sidebar_scroll_prefix_height(&self, has_root: bool) -> f32 {
        let sidebar_styles = &self.styles.sidebar;
        let logo_height = self.scaled(
            sidebar_styles.sidebar_logo_size_px + 2.0 * sidebar_styles.sidebar_logo_padding_y_px,
        );
        if has_root {
            logo_height + self.scaled(sidebar_styles.sidebar_header_height_px)
        } else {
            logo_height
        }
    }

    fn ensure_sidebar_item_visible(&self, index: usize) -> bool {
        let bounds = self.sidebar_scroll_handle.bounds();
        let viewport_height = f32::from(bounds.size.height);
        if viewport_height <= 0.0 {
            return false;
        }

        let offset = self.sidebar_scroll_handle.offset();
        let scroll_y = -f32::from(offset.y);
        let item_height = self.sidebar_item_height_px();
        let item_top = self.sidebar_scroll_prefix_height(true) + index as f32 * item_height;
        let item_bottom = item_top + item_height;

        let target_top = if item_top < scroll_y {
            Some(item_top)
        } else if item_bottom > scroll_y + viewport_height {
            Some((item_bottom - viewport_height).max(0.0))
        } else {
            None
        };

        let Some(target_top) = target_top else {
            return false;
        };

        self.sidebar_scroll_handle
            .set_offset(gpui::point(offset.x, px(-target_top)));
        true
    }

    pub(super) fn sidebar_drag_position_key(position: gpui::Point<Pixels>) -> (f32, f32) {
        (position.x.into(), position.y.into())
    }

    pub(super) fn sidebar_drag_has_child_target(&self, root: &Path) -> bool {
        self.sidebar_drag_expand_hover_dir
            .as_ref()
            .is_some_and(|target| target != root)
            || self
                .sidebar_drag_hover_target
                .as_ref()
                .is_some_and(|target| target != root)
    }

    pub(super) fn sidebar_drag_child_handled_same_event(
        &mut self,
        position: gpui::Point<Pixels>,
        root: &Path,
    ) -> bool {
        let handled = self.sidebar_drag_has_child_target(root)
            && self.sidebar_drag_child_hover_position
                == Some(Self::sidebar_drag_position_key(position));
        self.sidebar_drag_child_hover_position = None;
        handled
    }

    pub(super) fn clear_sidebar_drag_hover(&mut self, cx: &mut Context<Self>) {
        self.update_sidebar_drag_expand_hover_dir(None, cx);
        self.update_sidebar_drag_target(None, cx);
    }

    pub(super) fn update_sidebar_drag_autoscroll(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_drag_scroll_task = None;

        let bounds = self.sidebar_scroll_handle.bounds();
        if !bounds.contains(&position) {
            return;
        }

        let panel_height = bounds.size.height;
        if panel_height <= px(0.) {
            return;
        }

        let event_offset = position.y - bounds.origin.y;
        let hovered_region_offset = event_offset / panel_height;
        let vertical_scroll_offset = if hovered_region_offset <= 0.05 {
            8.0
        } else if hovered_region_offset <= 0.15 {
            5.0
        } else if hovered_region_offset >= 0.95 {
            -8.0
        } else if hovered_region_offset >= 0.85 {
            -5.0
        } else {
            return;
        };

        let adjustment = px(vertical_scroll_offset);
        self.sidebar_drag_scroll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let did_scroll = this
                    .update(cx, |editor, cx| {
                        editor.sidebar_drag_scroll_task.as_ref()?;

                        let offset = editor.sidebar_scroll_handle.offset();
                        let max_offset = editor.sidebar_scroll_handle.max_offset();
                        let next_y = (offset.y + adjustment).clamp(-max_offset.height, px(0.));
                        if next_y == offset.y {
                            return None;
                        }

                        editor
                            .sidebar_scroll_handle
                            .set_offset(gpui::point(offset.x, next_y));
                        let total_items = editor.sidebar_items().len();
                        editor.maybe_extend_sidebar_items(total_items, cx);
                        cx.notify();
                        Some(())
                    })
                    .ok()
                    .flatten()
                    .is_some();

                if !did_scroll {
                    return;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        }));
    }

    pub(crate) fn workspace_root_for_path(&self, path: &Path) -> Option<PathBuf> {
        self.workspaces
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
            .or_else(|| {
                path.starts_with(&self.sidebar_root)
                    .then(|| self.sidebar_root.clone())
            })
    }

    pub(crate) fn relative_path_for_path(&self, path: &Path) -> Option<String> {
        let root = self.workspace_root_for_path(path)?;
        let relative = path.strip_prefix(&root).ok()?;
        if relative.as_os_str().is_empty() {
            Some(".".to_string())
        } else {
            Some(relative.to_string_lossy().into_owned())
        }
    }

    fn sidebar_git_status_refresh_due(&self) -> bool {
        self.sidebar_git_statuses_dirty || self.sidebar_git_statuses_scanned_at.is_none()
    }

    fn sidebar_git_repo_root(&self) -> Option<PathBuf> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.sidebar_root)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        Some(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }

    fn merge_sidebar_git_status(
        current: Option<SidebarGitStatus>,
        next: SidebarGitStatus,
    ) -> SidebarGitStatus {
        match (current, next) {
            (Some(SidebarGitStatus::Modified), _) | (_, SidebarGitStatus::Modified) => {
                SidebarGitStatus::Modified
            }
            _ => SidebarGitStatus::Added,
        }
    }

    fn parse_sidebar_git_status_line(line: &str) -> Option<(String, SidebarGitStatus)> {
        if line.len() < 4 {
            return None;
        }

        let status_code = &line[..2];
        let raw_path = line[3..].trim();
        let normalized_path = raw_path
            .rsplit(" -> ")
            .next()
            .unwrap_or(raw_path)
            .trim_matches('"')
            .replace('\\', "/");
        let status = if status_code == "??" || status_code.contains('A') {
            SidebarGitStatus::Added
        } else if status_code.contains('M')
            || status_code.contains('D')
            || status_code.contains('R')
            || status_code.contains('C')
            || status_code.contains('U')
        {
            SidebarGitStatus::Modified
        } else {
            return None;
        };

        Some((normalized_path, status))
    }

    pub(super) fn refresh_sidebar_git_statuses_if_needed(&mut self) {
        if !self.sidebar_git_status_refresh_due() {
            return;
        }

        let mut profile_scope = profile::ProfileScope::new("sidebar.git_status_refresh");

        let Some(repo_root) = self.sidebar_git_repo_root() else {
            self.sidebar_git_statuses.clear();
            self.sidebar_git_statuses_dirty = false;
            self.sidebar_git_statuses_scanned_at = Some(SystemTime::now());
            profile_scope.set_note("repo_root=none entries=0");
            return;
        };

        let relative_root = self
            .sidebar_root
            .strip_prefix(&repo_root)
            .ok()
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| path.to_string_lossy().replace('\\', "/"));

        let mut args = vec![
            "status".to_string(),
            "--porcelain=v1".to_string(),
            "--untracked-files=all".to_string(),
        ];
        if let Some(relative_root) = relative_root {
            args.push("--".to_string());
            args.push(relative_root);
        }

        let mut statuses = HashMap::new();
        if let Ok(output) = Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .args(&args)
            .output()
            && output.status.success()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let Some((relative_path, status)) = Self::parse_sidebar_git_status_line(line)
                else {
                    continue;
                };

                let absolute_path = repo_root.join(relative_path);
                if !absolute_path.starts_with(&self.sidebar_root) {
                    continue;
                }

                let mut current = Some(absolute_path);
                while let Some(path) = current {
                    if !path.starts_with(&self.sidebar_root) {
                        break;
                    }

                    let merged =
                        Self::merge_sidebar_git_status(statuses.get(&path).copied(), status);
                    statuses.insert(path.clone(), merged);

                    if path == self.sidebar_root {
                        break;
                    }
                    current = path.parent().map(Path::to_path_buf);
                }
            }
        }

        if profile::enabled() {
            profile_scope.set_note(format!(
                "repo_root={} entries={} dirty_was={}",
                repo_root.display(),
                statuses.len(),
                self.sidebar_git_statuses_dirty
            ));
        }

        self.sidebar_git_statuses = statuses;
        self.sidebar_git_statuses_dirty = false;
        self.sidebar_git_statuses_scanned_at = Some(SystemTime::now());
    }

    pub(super) fn sidebar_git_status_color(
        &self,
        status: Option<SidebarGitStatus>,
    ) -> Option<gpui::Hsla> {
        match status {
            Some(SidebarGitStatus::Added) => {
                Some(self.styles.sidebar.sidebar_item_git_added_text_color)
            }
            Some(SidebarGitStatus::Modified) => {
                Some(self.styles.sidebar.sidebar_item_git_modified_text_color)
            }
            None => None,
        }
    }

    pub(super) fn update_sidebar_drag_target(
        &mut self,
        new_target: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let previous_target = self.sidebar_drag_hover_target.clone();
        self.sidebar_drag_hover_target = new_target.clone();

        if previous_target != new_target {
            cx.notify();
        }
    }

    pub(super) fn update_sidebar_drag_expand_hover_dir(
        &mut self,
        new_dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_drag_expand_hover_dir == new_dir {
            return;
        }

        self.sidebar_drag_expand_hover_dir = new_dir.clone();
        self.sidebar_drag_expand_task = None;

        let Some(expand_path) = new_dir else {
            return;
        };

        if self.expanded_dirs.contains(&expand_path) {
            return;
        }

        self.sidebar_drag_expand_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            this.update(cx, |editor, cx| {
                if editor.sidebar_drag_expand_hover_dir.as_ref() == Some(&expand_path)
                    && !editor.expanded_dirs.contains(&expand_path)
                {
                    editor.expanded_dirs.insert(expand_path.clone());
                    editor
                        .sidebar_drag_auto_expanded_dirs
                        .insert(expand_path.clone());
                    editor.invalidate_sidebar_items_cache();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn collapse_sidebar_drag_auto_expanded_dirs(&mut self) -> bool {
        let auto_expanded_dirs: Vec<PathBuf> =
            self.sidebar_drag_auto_expanded_dirs.drain().collect();
        let mut collapsed_any = false;
        for path in auto_expanded_dirs {
            if self.expanded_dirs.remove(&path) {
                collapsed_any = true;
            }
        }
        collapsed_any
    }

    pub(crate) fn clear_sidebar_drag_state(&mut self, cx: &mut Context<Self>) {
        self.sidebar_drag_child_hover_position = None;
        self.sidebar_drag_scroll_task = None;
        self.clear_sidebar_drag_hover(cx);
        if self.collapse_sidebar_drag_auto_expanded_dirs() {
            self.invalidate_sidebar_items_cache();
            cx.notify();
        }
    }

    fn reveal_path_in_sidebar_internal(
        &mut self,
        path: PathBuf,
        show_sidebar: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.workspace_root_for_path(&path) else {
            return;
        };

        let switched_workspace = self.sidebar_root != root;
        let preserve_hidden_sidebar = !show_sidebar && !self.sidebar_visible;

        if switched_workspace {
            self.switch_workspace(root.clone(), cx);
            if preserve_hidden_sidebar {
                self.sidebar_visible = false;
                self.sidebar_resizing = false;
                self.set_sidebar_width_immediately(0.0);
                session_store::persist_session(self);
            }
        } else if show_sidebar && !self.sidebar_visible {
            self.sidebar_visible = true;
            self.sync_sidebar_width_animation(cx);
        }

        let mut expanded_changed = switched_workspace;
        expanded_changed |= self.expanded_dirs.insert(root.clone());
        let mut ancestor = path.parent();
        while let Some(dir) = ancestor {
            if !dir.starts_with(&root) {
                break;
            }
            expanded_changed |= self.expanded_dirs.insert(dir.to_path_buf());
            if dir == root {
                break;
            }
            ancestor = dir.parent();
        }

        self.invalidate_sidebar_items_cache();
        let items = self.sidebar_items();
        if let Some(index) = items.iter().position(|item| item.path == path) {
            self.selected_sidebar_paths.clear();
            self.selected_sidebar_paths.insert(path.clone());
            self.sidebar_selection_anchor = Some(path.clone());

            let sidebar_is_available = show_sidebar || self.sidebar_visible;
            if sidebar_is_available {
                self.focused_sidebar_path = Some(path.clone());
                let scrolled = self.ensure_sidebar_item_visible(index);
                if switched_workspace || expanded_changed || scrolled {
                    let reveal_path = self.focused_sidebar_path.clone();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(1))
                            .await;
                        this.update(cx, |editor, cx| {
                            if let Some(path) = reveal_path.as_ref() {
                                let items = editor.sidebar_items();
                                if let Some(index) = items.iter().position(|item| &item.path == path)
                                    && editor.ensure_sidebar_item_visible(index)
                                {
                                    cx.notify();
                                }
                            }
                        })
                        .ok();
                    })
                    .detach();
                }
            } else {
                self.focused_sidebar_path = None;
            }
        }

        cx.notify();
    }

    pub(crate) fn reveal_path_in_sidebar(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.reveal_path_in_sidebar_internal(path, true, cx);
    }

    pub(crate) fn sync_path_in_sidebar_state(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.reveal_path_in_sidebar_internal(path, false, cx);
    }
}
