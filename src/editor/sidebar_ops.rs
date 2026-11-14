use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::Context;

use super::*;

impl EditorView {
    fn rewrite_moved_path(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
        let normalized = Self::normalize_sidebar_path(path);
        let relative = normalized.strip_prefix(from).ok()?;
        if relative.as_os_str().is_empty() {
            Some(to.to_path_buf())
        } else {
            Some(to.join(relative))
        }
    }

    pub(super) fn rewrite_open_tab_paths_after_move(&mut self, from: &Path, to: &Path) {
        let from = Self::normalize_sidebar_path(from);
        let to = Self::normalize_sidebar_path(to);
        for tab in self.all_tabs_mut() {
            if let Some(path) = tab.path.as_deref()
                && let Some(new_path) = Self::rewrite_moved_path(path, &from, &to)
            {
                tab.title = Self::title_for_path(&new_path);
                tab.path = Some(new_path);
                tab.is_deleted = false;
            }
            if let Some(source_path) = tab.recovery.source_path.as_deref()
                && let Some(new_source) = Self::rewrite_moved_path(source_path, &from, &to)
            {
                tab.recovery.source_path = Some(new_source);
            }
            if let Some(identity) = tab.history.source_identity.as_deref() {
                let identity_path = Path::new(identity);
                if let Some(new_identity) = Self::rewrite_moved_path(identity_path, &from, &to) {
                    tab.history.source_identity = Some(new_identity.to_string_lossy().into_owned());
                }
            }
        }

        if let Some(path) = self.current_file_path.as_deref()
            && let Some(new_path) = Self::rewrite_moved_path(path, &from, &to)
        {
            self.current_file_path = Some(new_path);
        }
        if let Some(path) = self.focused_sidebar_path.as_deref()
            && let Some(new_path) = Self::rewrite_moved_path(path, &from, &to)
        {
            self.focused_sidebar_path = Some(new_path);
        }
        if let Some(path) = self.sidebar_selection_anchor.as_deref()
            && let Some(new_path) = Self::rewrite_moved_path(path, &from, &to)
        {
            self.sidebar_selection_anchor = Some(new_path);
        }
        self.selected_sidebar_paths = self
            .selected_sidebar_paths
            .iter()
            .map(|path| Self::rewrite_moved_path(path, &from, &to).unwrap_or_else(|| path.clone()))
            .collect();
        self.expanded_dirs = self
            .expanded_dirs
            .iter()
            .map(|path| Self::rewrite_moved_path(path, &from, &to).unwrap_or_else(|| path.clone()))
            .collect();
        if !self.file_list_dir.as_os_str().is_empty()
            && let Some(new_dir) = Self::rewrite_moved_path(&self.file_list_dir, &from, &to)
        {
            self.file_list_dir = new_dir;
            self.file_list_dirty = true;
        }
        if self.app_settings.rename_starred_prefix(&from, &to) {
            self.app_settings.save();
        }
    }

    pub(super) fn is_sidebar_item_selected(&self, item_path: &Path) -> bool {
        self.selected_sidebar_paths.contains(item_path)
    }

    pub(super) fn sidebar_select_item(&mut self, path: &Path, shift: bool, cmd: bool) {
        let items = self.sidebar_items();
        let paths = items
            .iter()
            .map(|item| item.path.clone())
            .collect::<Vec<_>>();
        self.select_sidebar_paths(path, shift, cmd, &paths);
    }

    pub(super) fn select_starred_sidebar_item(&mut self, path: &Path, shift: bool, cmd: bool) {
        let paths = self.visible_starred_sidebar_files();
        self.select_sidebar_paths(path, shift, cmd, &paths);
        self.focused_starred_path = Some(path.to_path_buf());
    }

    fn select_sidebar_paths(&mut self, path: &Path, shift: bool, cmd: bool, paths: &[PathBuf]) {
        self.focused_workspace_root = None;
        self.focused_starred_path = None;
        let path_buf = path.to_path_buf();

        if shift {
            let anchor = self
                .sidebar_selection_anchor
                .clone()
                .unwrap_or_else(|| path_buf.clone());
            let anchor_idx = paths.iter().position(|item| item == &anchor);
            let target_idx = paths.iter().position(|item| item == &path_buf);
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
                    self.selected_sidebar_paths.insert(paths[index].clone());
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
        if self.reject_workspace_root_mutation(&paths, cx) {
            return;
        }
        let rewrite_context = self.capture_workspace_rewrite_context();
        let target_dir = Self::normalize_sidebar_path(&target_dir);
        let moves = paths
            .iter()
            .map(|path| Self::normalize_sidebar_path(path))
            .filter_map(|src| {
                Self::sidebar_drop_destination(&src, &target_dir).map(|dst| (src, dst))
            })
            .collect::<Vec<_>>();
        self.context_menu = None;
        self.clear_sidebar_drag_state(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut results = Vec::new();
            for (src, dst) in moves {
                let operation_src = src.clone();
                let operation_dst = dst.clone();
                let result = Self::run_sidebar_mutation("move", src.clone(), move || {
                    Self::move_path_compat(&operation_src, &operation_dst)
                })
                .await;
                let uncertain = Self::sidebar_mutation_outcome_uncertain(&result);
                results.push((src, dst, result));
                if uncertain {
                    break;
                }
            }
            this.update(cx, |editor, cx| {
                editor.finish_move_sidebar_items(target_dir, rewrite_context, results, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_move_sidebar_items(
        &mut self,
        target_dir: PathBuf,
        rewrite_context: Option<attachments::WorkspaceRewriteContext>,
        results: Vec<(PathBuf, PathBuf, anyhow::Result<()>)>,
        cx: &mut Context<Self>,
    ) {
        let mut first_revealed_path = None;
        let mut successful_mappings = Vec::new();
        for (src, dst, result) in results {
            if let Err(error) = result {
                let message = format!(
                    "{} ({} → {}): {error:#}",
                    self.t(crate::i18n::TextKey::FileOperationFailed),
                    src.display(),
                    dst.display()
                );
                self.notify_error(message, cx);
                continue;
            }
            successful_mappings.push(attachments::PathMapping::new(src.clone(), dst.clone()));
            if first_revealed_path.is_none() {
                first_revealed_path = Some(dst.clone());
            }

            self.rewrite_open_tab_paths_after_move(&src, &dst);

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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::EditorView;

    #[test]
    fn moved_directory_rewrites_nested_source_paths() {
        assert_eq!(
            EditorView::rewrite_moved_path(
                Path::new("/workspace/note.md"),
                Path::new("/workspace/note.md"),
                Path::new("/workspace/archive/note.md"),
            ),
            Some("/workspace/archive/note.md".into())
        );
        assert_eq!(
            EditorView::rewrite_moved_path(
                Path::new("/workspace/docs/note.md"),
                Path::new("/workspace/docs"),
                Path::new("/workspace/archive/docs"),
            ),
            Some("/workspace/archive/docs/note.md".into())
        );
    }
}
