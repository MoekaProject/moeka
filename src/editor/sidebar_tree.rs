use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::Context;

use super::*;

#[derive(Clone)]
pub(crate) struct SidebarDirectoryChild {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

fn load_sidebar_directory(path: &Path, sort_mode: SidebarSortMode) -> Vec<SidebarDirectoryChild> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by(|left, right| compare_dir_entries(sort_mode, left, right));
    entries
        .into_iter()
        .filter_map(|entry| {
            let entry_path = entry.path();
            let is_dir = entry.file_type().ok()?.is_dir();
            (!should_skip_path(&entry_path, is_dir)).then(|| SidebarDirectoryChild {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry_path,
                is_dir,
            })
        })
        .collect()
}

impl EditorView {
    fn capture_file_view_scroll_positions(&mut self) {
        if self.pending_sidebar_scroll_top.get().is_none() {
            let scroll_top = (-f32::from(self.sidebar_scroll_handle.offset().y)).max(0.0);
            self.pending_sidebar_scroll_top.set(Some(scroll_top));
            let item_height = self.sidebar_item_height_px();
            let prefix_height = self.sidebar_scroll_prefix_height(true);
            let items = self.sidebar_items_cache.clone();
            if !items.is_empty() && item_height > 0.0 {
                let anchor_index = (((scroll_top - prefix_height).max(0.0) / item_height).floor()
                    as usize)
                    .min(items.len() - 1);
                let mut candidate_indices = (anchor_index..items.len()).collect::<Vec<_>>();
                candidate_indices.extend((0..anchor_index).rev());
                let candidates = candidate_indices
                    .into_iter()
                    .map(|index| {
                        let relative_y = prefix_height + index as f32 * item_height - scroll_top;
                        (items[index].path.clone(), relative_y)
                    })
                    .collect();
                self.pending_sidebar_scroll_anchor = Some(PendingSidebarScrollAnchor {
                    candidates,
                    fallback_scroll_top: scroll_top,
                });
            }
        }
        if self.pending_file_list_scroll_top.get().is_none() {
            self.pending_file_list_scroll_top.set(Some(
                (-f32::from(self.file_list_scroll_handle.offset().y)).max(0.0),
            ));
        }
    }

    pub(crate) fn sidebar_paths_require_structure_refresh(&self, paths: &[PathBuf]) -> bool {
        paths.iter().any(|path| {
            let Ok(metadata) = std::fs::metadata(path) else {
                return true;
            };
            if metadata.is_dir() {
                return true;
            }
            let Some(parent) = path.parent() else {
                return true;
            };
            !self
                .sidebar_directory_children
                .get(parent)
                .is_some_and(|children| children.iter().any(|child| child.path == *path))
        })
    }

    pub(crate) fn invalidate_sidebar_items_cache(&mut self) {
        self.capture_file_view_scroll_positions();
        // Preserve the rendered tree until the replacement snapshot is ready.
        // Clearing the cache produces an empty frame and a visible scroll jump.
        self.sidebar_directory_loading.clear();
        self.sidebar_directory_epoch = self.sidebar_directory_epoch.wrapping_add(1);
        self.sidebar_directory_refresh_pending = true;
        self.quick_open_index_root = None;
        self.quick_open_file_index.clear();
        self.file_list_dirty = true;
    }

    pub(crate) fn refresh_sidebar_directories_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.sidebar_directory_refresh_pending {
            return;
        }
        self.sidebar_directory_refresh_pending = false;
        let mut paths = self
            .expanded_dirs
            .iter()
            .filter(|path| path.starts_with(&self.sidebar_root))
            .cloned()
            .collect::<Vec<_>>();
        if self.sidebar_root.exists() && !paths.contains(&self.sidebar_root) {
            paths.push(self.sidebar_root.clone());
        }
        if paths.is_empty() {
            self.pending_sidebar_scroll_top.set(None);
            self.pending_sidebar_scroll_anchor = None;
            return;
        }

        let epoch = self.sidebar_directory_epoch;
        let sort_mode = self.sidebar_sort_mode;
        self.sidebar_directory_loading.extend(paths.iter().cloned());
        cx.spawn(async move |this, cx| {
            let refreshed = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let children = load_sidebar_directory(&path, sort_mode);
                            (path, children)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if this.sidebar_directory_epoch != epoch {
                    return;
                }
                for (path, children) in refreshed {
                    this.sidebar_directory_loading.remove(&path);
                    this.sidebar_directory_children.insert(path, children);
                }
                this.sidebar_items_dirty = true;
                this.invalidate_sidebar_git_statuses();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn invalidate_sidebar_flattened_items(&mut self) {
        self.capture_file_view_scroll_positions();
        self.sidebar_items_dirty = true;
    }

    pub(crate) fn schedule_sidebar_directory_load(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.schedule_sidebar_directory_request(path, false, cx);
    }

    pub(crate) fn schedule_sidebar_directory_refresh(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.capture_file_view_scroll_positions();
        self.schedule_sidebar_directory_request(path, true, cx);
    }

    fn schedule_sidebar_directory_request(
        &mut self,
        path: PathBuf,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        if (!force && self.sidebar_directory_children.contains_key(&path))
            || !self.sidebar_directory_loading.insert(path.clone())
        {
            return;
        }
        let epoch = self.sidebar_directory_epoch;
        let sort_mode = self.sidebar_sort_mode;
        cx.spawn(async move |this, cx| {
            let background_path = path.clone();
            let children = cx
                .background_executor()
                .spawn(async move { load_sidebar_directory(&background_path, sort_mode) })
                .await;
            this.update(cx, |this, cx| {
                if this.sidebar_directory_epoch != epoch {
                    return;
                }
                this.sidebar_directory_loading.remove(&path);
                this.sidebar_directory_children.insert(path, children);
                this.invalidate_sidebar_flattened_items();
                this.invalidate_sidebar_git_statuses();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn invalidate_sidebar_git_statuses(&mut self) {
        self.sidebar_git_statuses_dirty = true;
        self.sidebar_git_statuses_scanned_at = None;
        self.sidebar_git_statuses_refresh_epoch =
            self.sidebar_git_statuses_refresh_epoch.wrapping_add(1);
    }

    pub(crate) fn toggle_sidebar_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.expanded_dirs.insert(path.clone()) {
            self.expanded_dirs.remove(&path);
        } else {
            self.schedule_sidebar_directory_load(path, cx);
        }
        self.invalidate_sidebar_flattened_items();
        cx.notify();
    }

    pub(crate) fn collapse_all_dirs(&mut self, cx: &mut Context<Self>) {
        self.expanded_dirs.clear();
        if self.sidebar_root.exists() {
            self.expanded_dirs.insert(self.sidebar_root.clone());
        }
        self.invalidate_sidebar_flattened_items();
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn expand_all_dirs(&mut self, cx: &mut Context<Self>) {
        let root = self.sidebar_root.clone();
        let sort_mode = self.sidebar_sort_mode;
        cx.spawn(async move |this, cx| {
            let expanded_dirs = cx
                .background_executor()
                .spawn(async move {
                    let mut expanded = HashSet::from([root.clone()]);
                    let mut pending = vec![root];
                    while let Some(dir) = pending.pop() {
                        for child in load_sidebar_directory(&dir, sort_mode) {
                            if child.is_dir && expanded.insert(child.path.clone()) {
                                pending.push(child.path);
                            }
                        }
                    }
                    expanded
                })
                .await;
            this.update(cx, |this, cx| {
                this.expanded_dirs = expanded_dirs;
                this.invalidate_sidebar_items_cache();
                this.context_menu = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open_sidebar_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.open_path_in_permanent_tab(path, true, cx);
    }

    pub(crate) fn collect_sidebar_items(
        &self,
        path: &Path,
        depth: usize,
        items: &mut Vec<SidebarItem>,
    ) {
        let Some(entries) = self.sidebar_directory_children.get(path).cloned() else {
            return;
        };

        for entry in entries {
            if self.file_list_visible && !entry.is_dir {
                continue;
            }
            let is_expanded = entry.is_dir && self.expanded_dirs.contains(&entry.path);
            items.push(SidebarItem {
                name: entry.name,
                path: entry.path.clone(),
                depth,
                is_dir: entry.is_dir,
                is_expanded,
            });
            if is_expanded {
                self.collect_sidebar_items(&entry.path, depth + 1, items);
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

pub(crate) fn compare_dir_entries(
    sort_mode: SidebarSortMode,
    left: &std::fs::DirEntry,
    right: &std::fs::DirEntry,
) -> std::cmp::Ordering {
    let left_is_dir = left.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
    let right_is_dir = right.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
    right_is_dir
        .cmp(&left_is_dir)
        .then_with(|| match sort_mode {
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
                let order = if sort_mode == SidebarSortMode::CreatedDesc {
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

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::{SidebarSortMode, load_sidebar_directory};

    #[test]
    fn directory_provider_loads_one_level_and_filters_hidden_entries() {
        let root = std::env::temp_dir().join(format!(
            "moeka-sidebar-provider-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("folder/nested")).unwrap();
        fs::write(root.join("visible.md"), "visible").unwrap();
        fs::write(root.join(".hidden.md"), "hidden").unwrap();

        let children = load_sidebar_directory(&root, SidebarSortMode::NameAsc);

        assert_eq!(children.len(), 2);
        assert!(children[0].is_dir);
        assert_eq!(children[0].name, "folder");
        assert_eq!(children[1].name, "visible.md");
        assert!(
            children
                .iter()
                .all(|child| child.path.parent() == Some(root.as_path()))
        );
        fs::remove_dir_all(root).unwrap();
    }
}
