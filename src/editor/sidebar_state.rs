use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use gpui::{Context, Pixels, px};

use super::{profile, *};

fn sidebar_git_repo_root_for_path(sidebar_root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(sidebar_root)
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

fn collect_sidebar_git_statuses_for_root(sidebar_root: &Path) -> HashMap<PathBuf, SidebarGitStatus> {
    let Some(repo_root) = sidebar_git_repo_root_for_path(sidebar_root) else {
        return HashMap::new();
    };

    let normalized_sidebar_root = EditorView::normalize_sidebar_path(sidebar_root);

    let relative_root = normalized_sidebar_root
        .strip_prefix(&repo_root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.to_string_lossy().replace('\\', "/"));

    let mut args = vec![
        "status".to_string(),
        "--porcelain=v1".to_string(),
        "--untracked-files=all".to_string(),
    ];
    if let Some(relative_root) = relative_root.as_deref() {
        args.push("--".to_string());
        args.push(relative_root.to_string());
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
            let Some((relative_path, status)) = EditorView::parse_sidebar_git_status_line(line)
            else {
                continue;
            };

            let relative_to_sidebar = if let Some(relative_root) = relative_root.as_deref() {
                if relative_path == relative_root {
                    ""
                } else if let Some(stripped) = relative_path
                    .strip_prefix(relative_root)
                    .and_then(|path| path.strip_prefix('/'))
                {
                    stripped
                } else {
                    continue;
                }
            } else {
                relative_path.as_str()
            };

            let absolute_path = EditorView::normalize_sidebar_path(
                &normalized_sidebar_root.join(relative_to_sidebar),
            );
            if !absolute_path.starts_with(&normalized_sidebar_root) {
                continue;
            }

            let mut current = Some(absolute_path);
            while let Some(path) = current {
                if !path.starts_with(&normalized_sidebar_root) {
                    break;
                }

                let merged = EditorView::merge_sidebar_git_status(statuses.get(&path).copied(), status);
                statuses.insert(path.clone(), merged);

                if path == normalized_sidebar_root {
                    break;
                }
                current = path.parent().map(Path::to_path_buf);
            }
        }
    }

    statuses
}

impl EditorView {
    pub(super) fn sidebar_item_height_px(&self) -> f32 {
        self.scaled(self.styles.sidebar.sidebar_item_height_px)
    }

    pub(super) fn visible_starred_sidebar_files(&mut self) -> Vec<PathBuf> {
        let mut changed = self.app_settings.prune_missing_starred_files();
        let root = Self::normalize_sidebar_path(&self.sidebar_root);
        let mut visible = Vec::new();
        let mut deduped = Vec::with_capacity(self.app_settings.starred_files.len());

        for path in self.app_settings.starred_files.iter().cloned() {
            let normalized = Self::normalize_sidebar_path(&path);
            if deduped.iter().any(|candidate| candidate == &normalized) {
                changed = true;
                continue;
            }
            if !normalized.exists() || !normalized.is_file() {
                changed = true;
                continue;
            }
            if !root.as_os_str().is_empty() && normalized.starts_with(&root) {
                visible.push(normalized.clone());
            }
            deduped.push(normalized);
        }

        if deduped != self.app_settings.starred_files {
            self.app_settings.starred_files = deduped;
            changed = true;
        }

        if changed {
            self.app_settings.save();
        }

        visible
    }

    pub(super) fn sidebar_starred_section_height(&self, has_root: bool) -> f32 {
        if !has_root {
            return 0.0;
        }

        let header_height = self.scaled(self.styles.sidebar.sidebar_header_height_px);
        if self.sidebar_starred_collapsed {
            return header_height;
        }

        header_height + self.sidebar_visible_starred_count() as f32 * self.sidebar_item_height_px()
    }

    pub(super) fn sidebar_visible_starred_count(&self) -> usize {
        let root = Self::normalize_sidebar_path(&self.sidebar_root);
        let mut visible = std::collections::HashSet::new();
        self.app_settings
            .starred_files
            .iter()
            .filter_map(|path| {
                let normalized = Self::normalize_sidebar_path(path);
                (normalized.exists()
                    && normalized.is_file()
                    && !root.as_os_str().is_empty()
                    && normalized.starts_with(&root))
                .then_some(normalized)
            })
            .for_each(|path| {
                visible.insert(path);
            });
        visible.len()
    }

    pub(super) fn sidebar_scroll_prefix_height(&self, has_root: bool) -> f32 {
        let sidebar_styles = &self.styles.sidebar;
        let logo_height = self.scaled(
            sidebar_styles.sidebar_logo_size_px + 2.0 * sidebar_styles.sidebar_logo_padding_y_px,
        );
        if has_root {
            logo_height
                + self.sidebar_starred_section_height(has_root)
                + self.scaled(sidebar_styles.sidebar_header_height_px)
        } else {
            logo_height
        }
    }

    fn ensure_sidebar_item_visible(&self, index: usize, item_count: usize) -> bool {
        if item_count == 0 {
            return false;
        }

        let bounds = self.sidebar_scroll_handle.bounds();
        let viewport_height = f32::from(bounds.size.height);
        let viewport_height = if viewport_height > 0.0 {
            viewport_height
        } else {
            self.last_window_height.max(self.sidebar_item_height_px())
        };

        let offset = self.sidebar_scroll_handle.offset();
        let scroll_y = -f32::from(offset.y);
        let item_height = self.sidebar_item_height_px();
        let prefix_height = self.sidebar_scroll_prefix_height(true);
        let item_top = prefix_height + index as f32 * item_height;
        let item_bottom = item_top + item_height;
        let bottom_spacer_height = self.scaled(self.styles.sidebar.sidebar_bottom_spacer_height_px);
        let content_height = prefix_height + item_count as f32 * item_height + bottom_spacer_height;
        let max_scroll_top = (content_height - viewport_height).max(0.0);
        let desired_top = ((item_top + item_bottom) * 0.5 - viewport_height * 0.5)
            .max(0.0)
            .min(max_scroll_top);

        if (desired_top - scroll_y).abs() < 0.5 {
            return false;
        }

        self.sidebar_scroll_handle
            .set_offset(gpui::point(offset.x, px(-desired_top)));
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

    fn schedule_sidebar_git_status_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.sidebar_git_status_refresh_due() || self.sidebar_git_statuses_task.is_some() {
            return;
        }

        let requested_sidebar_root = self.sidebar_root.clone();
        let epoch = self.sidebar_git_statuses_refresh_epoch;
        let delay = remaining_refresh_delay(
            self.sidebar_git_statuses_scanned_at,
            SIDEBAR_GIT_STATUS_REFRESH_MIN_INTERVAL_MS,
        );
        let background_sidebar_root = requested_sidebar_root.clone();
        let task = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }

            let statuses = cx
                .background_executor()
                .spawn(async move { collect_sidebar_git_statuses_for_root(&background_sidebar_root) })
                .await;

            this.update(cx, |this, cx| {
                this.sidebar_git_statuses_task = None;

                if this.sidebar_root != requested_sidebar_root
                    || this.sidebar_git_statuses_refresh_epoch != epoch
                {
                    if this.sidebar_git_status_refresh_due() {
                        this.schedule_sidebar_git_status_refresh(cx);
                    }
                    return;
                }

                if profile::enabled() {
                    let mut profile_scope = profile::ProfileScope::new("sidebar.git_status_refresh");
                    profile_scope.set_note(format!(
                        "repo_root={} entries={} dirty_was={}",
                        requested_sidebar_root.display(),
                        statuses.len(),
                        this.sidebar_git_statuses_dirty
                    ));
                }

                this.sidebar_git_statuses = statuses;
                this.sidebar_git_statuses_dirty = false;
                this.sidebar_git_statuses_scanned_at = Some(SystemTime::now());
                cx.notify();
            })
            .ok();
        });
        self.sidebar_git_statuses_task = Some(task);
    }

    pub(super) fn refresh_sidebar_git_statuses_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.sidebar_git_status_refresh_due() {
            return;
        }

        self.schedule_sidebar_git_status_refresh(cx);
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
        let item_count = items.len();
        if let Some(index) = items.iter().position(|item| item.path == path) {
            self.selected_sidebar_paths.clear();
            self.selected_sidebar_paths.insert(path.clone());
            self.sidebar_selection_anchor = Some(path.clone());

            let sidebar_is_available = show_sidebar || self.sidebar_visible;
            if sidebar_is_available {
                self.focused_sidebar_path = Some(path.clone());
                let immediate_scrolled = self.ensure_sidebar_item_visible(index, item_count);
                let needs_deferred_reveal =
                    switched_workspace || expanded_changed || immediate_scrolled;
                let reveal_path = self.focused_sidebar_path.clone();
                cx.spawn(async move |this, cx| {
                    let retry_count = if needs_deferred_reveal { 6 } else { 4 };
                    for _ in 0..retry_count {
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;

                        let done = this
                            .update(cx, |editor, cx| {
                                let Some(path) = reveal_path.as_ref() else {
                                    return true;
                                };

                                let items = editor.sidebar_items();
                                let item_count = items.len();
                                let Some(index) = items.iter().position(|item| &item.path == path)
                                else {
                                    return false;
                                };

                                let bounds = editor.sidebar_scroll_handle.bounds();
                                let viewport_height = f32::from(bounds.size.height);
                                if viewport_height <= 0.0 {
                                    return false;
                                }

                                if editor.ensure_sidebar_item_visible(index, item_count) {
                                    cx.notify();
                                }
                                true
                            })
                            .unwrap_or(true);

                        if done {
                            break;
                        }
                    }
                })
                .detach();
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
