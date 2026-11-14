use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{Context, Pixels, px};

use super::*;

#[derive(Default)]
struct SettingsWrites {
    pending: Option<(u64, AppSettings)>,
    running: bool,
}

impl gpui::Global for SettingsWrites {}

fn visible_starred_paths(stars: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    stars
        .iter()
        .map(|path| EditorView::normalize_sidebar_path(path))
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn minimal_scroll_top_for_item(
    scroll_top: f32,
    viewport_height: f32,
    item_top: f32,
    item_bottom: f32,
    max_scroll_top: f32,
) -> Option<f32> {
    let visible_bottom = scroll_top + viewport_height;
    let desired_top = if item_top < scroll_top {
        item_top
    } else if item_bottom > visible_bottom {
        item_bottom - viewport_height
    } else {
        return None;
    }
    .clamp(0.0, max_scroll_top.max(0.0));

    ((desired_top - scroll_top).abs() >= 0.5).then_some(desired_top)
}

fn scroll_top_for_sidebar_anchor(
    anchor: &PendingSidebarScrollAnchor,
    items: &[SidebarItem],
    prefix_height: f32,
    item_height: f32,
    max_scroll_top: f32,
) -> f32 {
    anchor
        .candidates
        .iter()
        .find_map(|(path, relative_y)| {
            items
                .iter()
                .position(|item| &item.path == path)
                .map(|index| prefix_height + index as f32 * item_height - relative_y)
        })
        .unwrap_or(anchor.fallback_scroll_top)
        .clamp(0.0, max_scroll_top.max(0.0))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        PendingSidebarScrollAnchor, SidebarItem, minimal_scroll_top_for_item,
        scroll_top_for_sidebar_anchor,
    };

    fn item(path: &str) -> SidebarItem {
        SidebarItem {
            path: PathBuf::from(path),
            name: path.to_string(),
            depth: 1,
            is_dir: false,
            is_expanded: false,
        }
    }

    #[test]
    fn offline_starred_rows_remain_visible_and_deduplicated_without_metadata() {
        let root = PathBuf::from("offline-workspace");
        let star = root.join("unavailable.md");
        let stars = vec![star.clone(), star.clone(), PathBuf::from("other/note.md")];
        assert_eq!(
            super::visible_starred_paths(&stars),
            vec![star, PathBuf::from("other/note.md")]
        );
        assert!(super::visible_starred_paths(&[]).is_empty());
        assert_eq!(stars.len(), 3);
    }

    #[test]
    fn file_tree_scrolls_only_enough_to_reveal_an_offscreen_item() {
        assert_eq!(
            minimal_scroll_top_for_item(100.0, 200.0, 120.0, 144.0, 500.0),
            None
        );
        assert_eq!(
            minimal_scroll_top_for_item(100.0, 200.0, 76.0, 100.0, 500.0),
            Some(76.0)
        );
        assert_eq!(
            minimal_scroll_top_for_item(100.0, 200.0, 300.0, 324.0, 500.0),
            Some(124.0)
        );
    }

    #[test]
    fn file_tree_refresh_keeps_a_path_anchor_stable_across_insert_and_delete() {
        let anchor = PendingSidebarScrollAnchor {
            candidates: vec![
                (PathBuf::from("b"), 0.0),
                (PathBuf::from("c"), 24.0),
                (PathBuf::from("a"), -24.0),
            ],
            fallback_scroll_top: 24.0,
        };

        let inserted = [item("new"), item("a"), item("b"), item("c")];
        assert_eq!(
            scroll_top_for_sidebar_anchor(&anchor, &inserted, 0.0, 24.0, 200.0),
            48.0
        );

        let anchor_deleted = [item("a"), item("c")];
        assert_eq!(
            scroll_top_for_sidebar_anchor(&anchor, &anchor_deleted, 0.0, 24.0, 200.0),
            0.0
        );
    }
}

impl EditorView {
    /// Coalesce scoped settings changes and keep their writes off the UI thread.
    pub(super) fn save_settings_in_background(&self, cx: &mut Context<Self>) {
        let settings = self.app_settings.clone();
        let generation = config_store::register_settings_write(&settings);
        let writes = cx.default_global::<SettingsWrites>();
        writes.pending = Some((generation, settings));
        if writes.running {
            return;
        }
        writes.running = true;
        let path = config_paths::settings_file_path();
        cx.spawn(async move |this, cx| {
            loop {
                let settings = cx
                    .update(|cx| {
                        let writes = cx.global_mut::<SettingsWrites>();
                        let pending = writes.pending.take();
                        if pending.is_none() {
                            writes.running = false;
                        }
                        pending
                    })
                    .ok()
                    .flatten();
                let Some((generation, settings)) = settings else {
                    break;
                };
                let result =
                    crate::platform::fs_task::run_write("Save settings", path.clone(), move || {
                        config_store::save_registered_settings(generation, settings)
                    })
                    .await;
                if let Err(error) = result {
                    let _ = cx.update(|cx| {
                        let writes = cx.global_mut::<SettingsWrites>();
                        writes.pending = None;
                        writes.running = false;
                    });
                    let message = error.to_string();
                    if this
                        .update(cx, |this, cx| this.notify_diagnostic(message.clone(), cx))
                        .is_err()
                    {
                        log_error!("{message}");
                    }
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn sidebar_item_height_px(&self) -> f32 {
        self.scaled(self.styles.sidebar.sidebar_item_height_px)
    }

    pub(super) fn visible_starred_sidebar_files(&self) -> Vec<PathBuf> {
        visible_starred_paths(&self.app_settings.starred_files)
            .into_iter()
            .filter(|path| self.sidebar_cached_path_is_dir(path) != Some(true))
            .filter(|path| {
                !self.sidebar_filter_active()
                    || path.file_name().is_some_and(|name| {
                        !self
                            .sidebar_filter_match_ranges(&name.to_string_lossy())
                            .is_empty()
                    })
            })
            .collect()
    }

    pub(super) fn sidebar_starred_section_height(&self, has_root: bool) -> f32 {
        if !has_root && self.sidebar_visible_starred_count() == 0 {
            return 0.0;
        }
        self.scaled(self.styles.sidebar.sidebar_header_height_px)
            + self.sidebar_starred_list_height()
    }

    pub(super) fn sidebar_visible_starred_count(&self) -> usize {
        self.visible_starred_sidebar_files().len()
    }

    pub(super) fn sidebar_starred_list_height(&self) -> f32 {
        if self.sidebar_starred_collapsed && self.sidebar_starred_motion.is_none() {
            return 0.0;
        }
        let row_height = self.sidebar_item_height_px();
        let content = self.sidebar_visible_starred_count() as f32 * row_height
            + self
                .starred_reveal()
                .map_or(0.0, |reveal| reveal.offset(row_height));
        let available = (self.last_window_height
            - self.tab_bar_height()
            - self.scaled(self.styles.sidebar.sidebar_header_height_px) * 2.0
            - row_height * (1.0 + self.styles.sidebar.sidebar_min_tree_rows))
            .max(0.0);
        content.max(0.0).min(
            available
                * self
                    .styles
                    .sidebar
                    .sidebar_root_region_max_fraction
                    .clamp(0.0, 0.5),
        )
    }

    pub(super) fn ensure_starred_path_visible(&self, path: &Path) {
        let paths = self.visible_starred_sidebar_files();
        let Some(index) = paths.iter().position(|item| item == path) else {
            return;
        };
        let height = self.sidebar_starred_list_height();
        let row = self.sidebar_item_height_px();
        let offset = self.sidebar_starred_scroll_handle.offset();
        if let Some(top) = minimal_scroll_top_for_item(
            -f32::from(offset.y),
            height,
            index as f32 * row,
            (index + 1) as f32 * row,
            (paths.len() as f32 * row - height).max(0.0),
        ) {
            self.sidebar_starred_scroll_handle
                .set_offset(gpui::point(offset.x, px(-top)));
        }
    }

    pub(super) fn ensure_sidebar_item_minimally_visible(
        &self,
        index: usize,
        item_count: usize,
    ) -> bool {
        if item_count == 0 {
            return false;
        }

        let bounds = self.sidebar_scroll_handle.bounds();
        let viewport_height = f32::from(bounds.size.height);
        if viewport_height <= 0.0 {
            return false;
        }

        let offset = self.sidebar_scroll_handle.offset();
        let scroll_top = -f32::from(offset.y);
        let item_height = self.sidebar_item_height_px();
        let prefix_height = 0.0;
        let item_top = prefix_height + index as f32 * item_height;
        let item_bottom = item_top + item_height;
        let bottom_spacer_height = self.scaled(self.styles.sidebar.sidebar_bottom_spacer_height_px);
        let content_height = prefix_height + item_count as f32 * item_height + bottom_spacer_height;
        let max_scroll_top = (content_height - viewport_height).max(0.0);
        let Some(desired_top) = minimal_scroll_top_for_item(
            scroll_top,
            viewport_height,
            item_top,
            item_bottom,
            max_scroll_top,
        ) else {
            return false;
        };
        self.sidebar_scroll_handle
            .set_offset(gpui::point(offset.x, px(-desired_top)));
        true
    }

    /// Scrolls the file tree so `index` sits vertically centered, regardless of
    /// whether it is already visible. Used by explicit "reveal in tree".
    pub(super) fn center_sidebar_item_in_view(&self, index: usize, item_count: usize) -> bool {
        if item_count == 0 {
            return false;
        }

        let bounds = self.sidebar_scroll_handle.bounds();
        let viewport_height = f32::from(bounds.size.height);
        if viewport_height <= 0.0 {
            return false;
        }

        let offset = self.sidebar_scroll_handle.offset();
        let item_height = self.sidebar_item_height_px();
        let prefix_height = 0.0;
        let item_top = prefix_height + index as f32 * item_height;
        let bottom_spacer_height = self.scaled(self.styles.sidebar.sidebar_bottom_spacer_height_px);
        let content_height = prefix_height + item_count as f32 * item_height + bottom_spacer_height;
        let max_scroll_top = (content_height - viewport_height).max(0.0);
        let desired_top =
            (item_top + item_height * 0.5 - viewport_height * 0.5).clamp(0.0, max_scroll_top);
        self.sidebar_scroll_handle
            .set_offset(gpui::point(offset.x, px(-desired_top)));
        true
    }

    pub(super) fn resolve_pending_sidebar_scroll_anchor(&mut self, items: &[SidebarItem]) {
        let Some(anchor) = self.pending_sidebar_scroll_anchor.take() else {
            return;
        };
        let viewport_height = f32::from(self.sidebar_scroll_handle.bounds().size.height);
        if viewport_height <= 0.0 {
            self.pending_sidebar_scroll_anchor = Some(anchor);
            return;
        }

        let item_height = self.sidebar_item_height_px();
        let prefix_height = 0.0;
        let bottom_spacer_height = self.scaled(self.styles.sidebar.sidebar_bottom_spacer_height_px);
        let content_height =
            prefix_height + items.len() as f32 * item_height + bottom_spacer_height;
        let max_scroll_top = (content_height - viewport_height).max(0.0);
        let target_scroll_top = scroll_top_for_sidebar_anchor(
            &anchor,
            items,
            prefix_height,
            item_height,
            max_scroll_top,
        );
        self.pending_sidebar_scroll_top.set(Some(target_scroll_top));
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
        self.workspace_drop_target = None;
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
        centered: bool,
        cx: &mut Context<Self>,
    ) {
        let root = if !show_sidebar {
            (!self.sidebar_root.as_os_str().is_empty() && path.starts_with(&self.sidebar_root))
                .then(|| self.sidebar_root.clone())
        } else {
            self.workspace_root_for_path(&path)
        };
        let Some(root) = root else {
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

        // Explicit navigation wins over passive refresh scroll restoration;
        // clear after a possible workspace switch, which invalidates caches.
        self.pending_sidebar_scroll_top.set(None);
        self.pending_sidebar_scroll_anchor = None;
        self.pending_file_list_scroll_top.set(None);
        self.pending_sidebar_reveal_path = Some(path.clone());

        let mut expanded_changed = switched_workspace;
        let mut directories_to_load = Vec::new();
        expanded_changed |= self.expanded_dirs.insert(root.clone());
        directories_to_load.push(root.clone());
        let mut ancestor = path.parent();
        while let Some(dir) = ancestor {
            if !dir.starts_with(&root) {
                break;
            }
            expanded_changed |= self.expanded_dirs.insert(dir.to_path_buf());
            directories_to_load.push(dir.to_path_buf());
            if dir == root {
                break;
            }
            ancestor = dir.parent();
        }

        self.set_file_list_dir_for_path(&path, cx);
        if centered {
            self.file_list_pending_reveal_centered = true;
        }
        self.selected_sidebar_paths.clear();
        self.selected_sidebar_paths.insert(path.clone());
        self.sidebar_selection_anchor = Some(path.clone());
        for directory in directories_to_load {
            self.schedule_sidebar_directory_load(directory, cx);
        }
        self.invalidate_sidebar_flattened_items();
        let items = self.sidebar_items();
        let item_count = items.len();
        let sidebar_is_available = show_sidebar || self.sidebar_visible;
        if sidebar_is_available {
            self.focused_sidebar_path = Some(path.clone());
            let current_index = items.iter().position(|item| item.path == path);
            let immediate_scrolled = current_index.is_some_and(|index| {
                if centered {
                    self.center_sidebar_item_in_view(index, item_count)
                } else {
                    self.ensure_sidebar_item_minimally_visible(index, item_count)
                }
            });
            let needs_deferred_reveal = switched_workspace
                || expanded_changed
                || immediate_scrolled
                || current_index.is_none();
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

                            if centered {
                                editor.center_sidebar_item_in_view(index, item_count);
                                cx.notify();
                            } else if editor
                                .ensure_sidebar_item_minimally_visible(index, item_count)
                            {
                                cx.notify();
                            }
                            if editor.pending_sidebar_reveal_path.as_ref() == Some(path) {
                                editor.pending_sidebar_reveal_path = None;
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
            self.focused_workspace_root = None;
        }

        cx.notify();
    }

    pub(crate) fn reveal_path_in_sidebar(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.reveal_path_in_sidebar_internal(path, true, false, cx);
    }

    /// "Reveal in tree": expands ancestors and centers the row in both the file
    /// tree and the FileList.
    pub(crate) fn locate_path_in_sidebar(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.reveal_path_in_sidebar_internal(path, true, true, cx);
    }

    pub(crate) fn sync_path_in_sidebar_state(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.reveal_path_in_sidebar_internal(path, false, false, cx);
    }
}
