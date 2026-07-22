use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::Context;

use super::*;

impl EditorView {
    pub(super) fn is_sidebar_item_selected(&self, item_path: &Path) -> bool {
        self.selected_sidebar_paths.contains(item_path)
    }

    pub(super) fn sidebar_select_item(&mut self, path: &Path, shift: bool, cmd: bool) {
        let items = self.sidebar_items();
        let path_buf = path.to_path_buf();

        if shift {
            let anchor = self
                .sidebar_selection_anchor
                .clone()
                .unwrap_or_else(|| path_buf.clone());
            let anchor_idx = items.iter().position(|item| item.path == anchor);
            let target_idx = items.iter().position(|item| item.path == path_buf);
            if let (Some(anchor_index), Some(target_index)) = (anchor_idx, target_idx) {
                let (start, end) = if anchor_index <= target_index {
                    (anchor_index, target_index)
                } else {
                    (target_index, anchor_index)
                };
                if !cmd {
                    self.selected_sidebar_paths.clear();
                }
                for index in start..=end {
                    self.selected_sidebar_paths
                        .insert(items[index].path.clone());
                }
            } else {
                if !cmd {
                    self.selected_sidebar_paths.clear();
                }
                self.selected_sidebar_paths.insert(path_buf.clone());
                self.sidebar_selection_anchor = Some(path_buf.clone());
            }
        } else if cmd {
            if self.selected_sidebar_paths.contains(&path_buf) {
                self.selected_sidebar_paths.remove(&path_buf);
            } else {
                self.selected_sidebar_paths.insert(path_buf.clone());
            }
            self.sidebar_selection_anchor = Some(path_buf.clone());
        } else {
            self.selected_sidebar_paths.clear();
            self.selected_sidebar_paths.insert(path_buf.clone());
            self.sidebar_selection_anchor = Some(path_buf.clone());
        }

        self.focused_sidebar_path = Some(path_buf);
    }

    pub(super) fn filter_nested_paths(paths: &HashSet<PathBuf>) -> Vec<PathBuf> {
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

    pub(crate) fn normalize_sidebar_path(path: &Path) -> PathBuf {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            normalized.push(component.as_os_str());
        }
        if normalized.as_os_str().is_empty() && !path.as_os_str().is_empty() {
            path.to_path_buf()
        } else {
            normalized
        }
    }

    fn sidebar_drop_destination(src: &Path, target_dir: &Path) -> Option<PathBuf> {
        let src = Self::normalize_sidebar_path(src);
        let target_dir = Self::normalize_sidebar_path(target_dir);
        let name = src.file_name()?;
        let destination = target_dir.join(name);
        (src != destination && !destination.starts_with(&src)).then_some(destination)
    }

    pub(super) fn sidebar_drop_would_move(paths: &[PathBuf], target_dir: &Path) -> bool {
        paths
            .iter()
            .any(|src| Self::sidebar_drop_destination(src, target_dir).is_some())
    }

    pub(crate) fn move_sidebar_items(
        &mut self,
        paths: Vec<PathBuf>,
        target_dir: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let mut first_revealed_path = None;
        let rewrite_context = self.capture_workspace_rewrite_context();
        let mut successful_mappings = Vec::new();
        let target_dir = Self::normalize_sidebar_path(&target_dir);
        for src in paths.iter().map(|path| Self::normalize_sidebar_path(path)) {
            let Some(dst) = Self::sidebar_drop_destination(&src, &target_dir) else {
                continue;
            };
            if let Err(error) = std::fs::rename(&src, &dst) {
                let message = format!(
                    "{}: {error}",
                    self.t(crate::i18n::TextKey::FileOperationFailed)
                );
                self.notify_error(message, cx);
                continue;
            }
            successful_mappings.push(attachments::PathMapping::new(src.clone(), dst.clone()));
            if first_revealed_path.is_none() {
                first_revealed_path = Some(dst.clone());
            }

            for tab in self.all_tabs_mut() {
                if let Some(ref tab_path) = tab.path {
                    let normalized_tab_path = Self::normalize_sidebar_path(tab_path);
                    if normalized_tab_path.starts_with(&src) {
                        let relative = normalized_tab_path
                            .strip_prefix(&src)
                            .unwrap_or(normalized_tab_path.as_path());
                        let new_tab_path = dst.join(relative);
                        tab.title = Self::title_for_path(&new_tab_path);
                        tab.path = Some(new_tab_path);
                    }
                }
            }

            let old_selected: Vec<PathBuf> = self
                .selected_sidebar_paths
                .iter()
                .filter(|path| Self::normalize_sidebar_path(path).starts_with(&src))
                .cloned()
                .collect();
            for old in old_selected {
                self.selected_sidebar_paths.remove(&old);
                let normalized_old = Self::normalize_sidebar_path(&old);
                let relative = normalized_old
                    .strip_prefix(&src)
                    .unwrap_or(normalized_old.as_path());
                self.selected_sidebar_paths.insert(dst.join(relative));
            }

            if self
                .current_file_path
                .as_ref()
                .is_some_and(|path| Self::normalize_sidebar_path(path).starts_with(&src))
                && let Some(ref path) = self.current_file_path
            {
                let normalized_path = Self::normalize_sidebar_path(path);
                let relative = normalized_path
                    .strip_prefix(&src)
                    .unwrap_or(normalized_path.as_path());
                self.current_file_path = Some(dst.join(relative));
            }

            if self
                .sidebar_selection_anchor
                .as_ref()
                .is_some_and(|path| Self::normalize_sidebar_path(path).starts_with(&src))
                && let Some(anchor) = self.sidebar_selection_anchor.as_ref()
            {
                let normalized_anchor = Self::normalize_sidebar_path(anchor);
                let relative = normalized_anchor
                    .strip_prefix(&src)
                    .unwrap_or(normalized_anchor.as_path());
                self.sidebar_selection_anchor = Some(dst.join(relative));
            }
        }
        self.apply_workspace_link_rewrites(rewrite_context, &successful_mappings, cx);
        self.context_menu = None;
        self.invalidate_sidebar_items_cache();
        self.clear_sidebar_drag_state(cx);
        if let Some(reveal_path) = first_revealed_path {
            let mut moved_selection = self.selected_sidebar_paths.clone();
            if !moved_selection.contains(&reveal_path) {
                moved_selection.insert(reveal_path.clone());
            }
            self.reveal_path_in_sidebar(reveal_path.clone(), cx);
            if self.file_list_effective_dir() == target_dir {
                self.file_list_pending_reveal_path = Some(reveal_path.clone());
                self.file_list_dirty = true;
            }
            self.selected_sidebar_paths = moved_selection;
            self.focused_sidebar_path = Some(reveal_path.clone());
            self.sidebar_selection_anchor = Some(reveal_path);
        }
        cx.notify();
    }
}
