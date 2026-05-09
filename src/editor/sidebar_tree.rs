use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::Context;

use super::*;

impl EditorView {
    pub(crate) fn invalidate_sidebar_items_cache(&mut self) {
        self.sidebar_items_dirty = true;
    }

    pub(crate) fn invalidate_sidebar_git_statuses(&mut self) {
        self.sidebar_git_statuses_dirty = true;
        self.sidebar_git_statuses_scanned_at = None;
    }

    pub(super) fn reset_sidebar_render_limit(&mut self) {
        self.sidebar_render_limit = 200;
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
        if self.sidebar_root.exists() {
            self.expanded_dirs.insert(self.sidebar_root.clone());
        }
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
                            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
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
            if self.sidebar_error.is_none()
                && self.sidebar_root.exists()
                && self.expanded_dirs.contains(&self.sidebar_root)
            {
                self.collect_sidebar_items(&self.sidebar_root, 1, &mut items);
            }
            self.sidebar_items_cache = items;
            self.sidebar_items_dirty = false;
        }

        self.sidebar_items_cache.clone()
    }
}
