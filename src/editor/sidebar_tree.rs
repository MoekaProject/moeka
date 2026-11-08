use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::Context;

use super::*;

pub(crate) struct SidebarFsError {
    operation: String,
    path: PathBuf,
    message: String,
}

impl SidebarFsError {
    fn matches(&self, operation: &str, path: &Path) -> bool {
        self.operation == operation && self.path == path
    }
}

impl std::fmt::Display for SidebarFsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Clone)]
pub(crate) struct SidebarDirectoryChild {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) is_dir: bool,
}

pub(super) fn load_sidebar_directory(
    path: &Path,
    sort_mode: SidebarSortMode,
) -> std::io::Result<Vec<SidebarDirectoryChild>> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir(path)? {
        crate::platform::fs_task::check_deadline()?;
        let entry = entry?;
        let entry_path = entry.path();
        let is_dir = entry.file_type()?.is_dir();
        if !should_skip_path(&entry_path, is_dir) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let created = if matches!(
                sort_mode,
                SidebarSortMode::CreatedAsc | SidebarSortMode::CreatedDesc
            ) {
                let metadata = entry.metadata()?;
                metadata
                    .created()
                    .or_else(|_| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            } else {
                SystemTime::UNIX_EPOCH
            };
            let key = DirectorySortKey::new(&entry_path, &name, is_dir, created);
            children.push((
                key,
                SidebarDirectoryChild {
                    name,
                    path: entry_path,
                    is_dir,
                },
            ));
        }
    }
    children.sort_by(|(left, _), (right, _)| left.compare(right, sort_mode));
    Ok(children.into_iter().map(|(_, child)| child).collect())
}

/// Number of markdown documents directly inside `dir`, matching what the
/// FileList pane lists for that directory.
fn count_dir_markdown_files(dir: &Path) -> std::io::Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir(dir)? {
        crate::platform::fs_task::check_deadline()?;
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir()
            && !should_skip_path(&path, false)
            && is_markdown_file(&path)
        {
            count += 1;
        }
    }
    Ok(count)
}

fn apply_sidebar_directory_snapshot(
    cache: &mut std::collections::HashMap<PathBuf, Vec<SidebarDirectoryChild>>,
    path: PathBuf,
    snapshot: std::io::Result<Vec<SidebarDirectoryChild>>,
) -> std::io::Result<()> {
    cache.insert(path, snapshot?);
    Ok(())
}

fn seed_directory_counts(
    dirs: &[PathBuf],
    counts: &mut std::collections::HashMap<PathBuf, usize>,
    dirty: &HashSet<PathBuf>,
    snapshots: &std::collections::HashMap<PathBuf, Vec<SidebarDirectoryChild>>,
) {
    for dir in dirs {
        if !counts.contains_key(dir)
            && !dirty.contains(dir)
            && let Some(children) = snapshots.get(dir)
        {
            counts.insert(
                dir.clone(),
                children
                    .iter()
                    .filter(|child| !child.is_dir && is_markdown_file(&child.path))
                    .count(),
            );
        }
    }
}

impl EditorView {
    pub(super) fn report_optional_fs_error(
        &self,
        operation: &'static str,
        path: &Path,
        error: &std::io::Error,
        cx: &mut Context<Self>,
    ) {
        if file_watch::error_notification_due(operation, path) {
            eprintln!(
                "{}",
                crate::platform::fs_task::format_error(operation, path, error)
            );
            let root = self.sidebar_root.clone();
            let epoch = self.sidebar_directory_epoch;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                this.update(cx, |editor, cx| {
                    if editor.sidebar_root == root && editor.sidebar_directory_epoch == epoch {
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// UI decisions use the directory snapshots, never mount-backed metadata.
    pub(crate) fn sidebar_cached_path_is_dir(&self, path: &Path) -> Option<bool> {
        if !path.as_os_str().is_empty() && path == self.sidebar_root {
            return Some(true);
        }
        if let Some(child) = path
            .parent()
            .and_then(|parent| self.sidebar_directory_children.get(parent))
            .and_then(|children| children.iter().find(|child| child.path == path))
        {
            return Some(child.is_dir);
        }
        if self.sidebar_directory_children.contains_key(path) || self.expanded_dirs.contains(path) {
            return Some(true);
        }
        None
    }

    pub(super) fn report_sidebar_fs_error(
        &mut self,
        operation: &str,
        path: &Path,
        error: std::io::Error,
        cx: &mut Context<Self>,
    ) {
        let message = crate::platform::fs_task::format_error(operation, path, error);
        if self
            .sidebar_error
            .as_ref()
            .is_none_or(|error| !error.matches(operation, path) || error.message != message)
        {
            self.sidebar_error = Some(SidebarFsError {
                operation: operation.to_string(),
                path: path.to_path_buf(),
                message: message.clone(),
            });
            self.notify_diagnostic(message, cx);
        }
    }

    pub(super) fn clear_sidebar_fs_error(&mut self, operation: &str, path: &Path) {
        if self
            .sidebar_error
            .as_ref()
            .is_some_and(|error| error.matches(operation, path))
        {
            self.sidebar_error = None;
        }
    }

    /// Marks the badge counts of `dirs` as out of date. The watcher calls this
    /// with the parent of every path that changed on disk, which is the only
    /// thing that can change a count.
    pub(crate) fn mark_sidebar_dir_file_counts_dirty(
        &mut self,
        dirs: impl IntoIterator<Item = PathBuf>,
    ) {
        self.sidebar_dir_file_counts_dirty.extend(dirs);
    }

    /// Loads the badge counts for the given directories in the background.
    /// Already-known counts are kept visible until the new value arrives.
    pub(crate) fn request_sidebar_dir_file_counts(
        &mut self,
        dirs: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        seed_directory_counts(
            &dirs,
            &mut self.sidebar_dir_file_counts,
            &self.sidebar_dir_file_counts_dirty,
            &self.sidebar_directory_children,
        );
        let pending = dirs
            .into_iter()
            .filter(|dir| {
                (self.sidebar_dir_file_counts_dirty.contains(dir)
                    || !self.sidebar_dir_file_counts.contains_key(dir))
                    && !self.sidebar_dir_file_counts_loading.contains(dir)
                    && !file_watch::operation_suppressed("Count directory documents", dir)
            })
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return;
        }
        let root = self.sidebar_root.clone();
        let epoch = self.sidebar_directory_epoch;
        for dir in &pending {
            self.sidebar_dir_file_counts_dirty.remove(dir);
            self.sidebar_dir_file_counts_loading.insert(dir.clone());
        }
        let task_path = pending[0].clone();
        cx.spawn(async move |this, cx| {
            let background_dirs = pending.clone();
            let counted = crate::platform::fs_task::run_background(
                "Count directory documents",
                task_path.clone(),
                move || {
                    let mut counts = Vec::with_capacity(background_dirs.len());
                    for dir in background_dirs {
                        crate::platform::fs_task::check_deadline()?;
                        match count_dir_markdown_files(&dir) {
                            Ok(count) => counts.push((dir, Ok(count))),
                            Err(error) => {
                                counts.push((dir, Err(error)));
                            }
                        }
                    }
                    Ok(counts)
                },
            )
            .await;
            this.update(cx, |this, cx| {
                if this.sidebar_root != root || this.sidebar_directory_epoch != epoch {
                    return;
                }
                for dir in &pending {
                    this.sidebar_dir_file_counts_loading.remove(dir);
                }
                match counted {
                    Ok(counts) => {
                        for (dir, result) in counts {
                            match result {
                                Ok(count) => {
                                    this.clear_sidebar_fs_error("Count directory documents", &dir);
                                    this.sidebar_dir_file_counts.insert(dir, count);
                                }
                                Err(error) => {
                                    this.sidebar_dir_file_counts_dirty.insert(dir.clone());
                                    this.report_optional_fs_error(
                                        "Count directory documents",
                                        &dir,
                                        &error,
                                        cx,
                                    );
                                }
                            }
                        }
                    }
                    Err(error) => {
                        for dir in pending {
                            this.sidebar_dir_file_counts_dirty.insert(dir.clone());
                            this.report_optional_fs_error(
                                "Count directory documents",
                                &dir,
                                &error,
                                cx,
                            );
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn capture_file_view_scroll_positions(&mut self) {
        if self.pending_sidebar_reveal_path.is_some() {
            return;
        }
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

    pub(crate) fn invalidate_sidebar_items_cache(&mut self) {
        self.capture_file_view_scroll_positions();
        // Preserve the rendered tree until the replacement snapshot is ready.
        // Clearing the cache produces an empty frame and a visible scroll jump.
        self.sidebar_directory_loading.clear();
        self.sidebar_dir_file_counts_dirty
            .extend(self.sidebar_dir_file_counts_loading.drain());
        self.sidebar_error = None;
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
        if !self.sidebar_root.as_os_str().is_empty() && !paths.contains(&self.sidebar_root) {
            paths.push(self.sidebar_root.clone());
        }
        if paths.is_empty() {
            self.pending_sidebar_scroll_top.set(None);
            self.pending_sidebar_scroll_anchor = None;
            return;
        }

        for path in paths {
            self.schedule_sidebar_directory_request(path, true, cx);
        }
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
        self.invalidate_sidebar_filter_index();
        self.capture_file_view_scroll_positions();
        self.clear_sidebar_fs_error("Count directory documents", &path);
        self.mark_sidebar_dir_file_counts_dirty([path.clone()]);
        self.schedule_sidebar_directory_request(path, true, cx);
    }

    fn schedule_sidebar_directory_request(
        &mut self,
        path: PathBuf,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        if (!force
            && (self.sidebar_directory_children.contains_key(&path)
                || self.sidebar_error.is_some()))
            || !self.sidebar_directory_loading.insert(path.clone())
        {
            return;
        }
        let epoch = self.sidebar_directory_epoch;
        let root = self.sidebar_root.clone();
        let sort_mode = self.sidebar_sort_mode;
        cx.spawn(async move |this, cx| {
            let background_path = path.clone();
            let children =
                crate::platform::fs_task::run("Read sidebar directory", path.clone(), move || {
                    load_sidebar_directory(&background_path, sort_mode)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.sidebar_root != root || this.sidebar_directory_epoch != epoch {
                    return;
                }
                this.sidebar_directory_loading.remove(&path);
                match apply_sidebar_directory_snapshot(
                    &mut this.sidebar_directory_children,
                    path.clone(),
                    children,
                ) {
                    Ok(()) => {
                        this.clear_sidebar_fs_error("Read sidebar directory", &path);
                        this.invalidate_sidebar_flattened_items();
                    }
                    Err(error) => {
                        this.report_sidebar_fs_error("Read sidebar directory", &path, error, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn toggle_sidebar_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.sidebar_filter_active() {
            self.toggle_filtered_directory(path, cx);
            return;
        }
        let before = self.sidebar_items();
        if !self.expanded_dirs.insert(path.clone()) {
            self.expanded_dirs.remove(&path);
        } else if self.sidebar_error.is_some() {
            self.schedule_sidebar_directory_refresh(path.clone(), cx);
        } else {
            self.schedule_sidebar_directory_load(path.clone(), cx);
        }
        self.invalidate_sidebar_flattened_items();
        self.begin_sidebar_tree_motion(path, before);
        cx.notify();
    }

    pub(crate) fn collapse_all_dirs(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_filter_active() {
            self.set_filtered_directories_collapsed(true, cx);
            return;
        }
        self.expanded_dirs.clear();
        if !self.sidebar_root.as_os_str().is_empty() {
            self.expanded_dirs.insert(self.sidebar_root.clone());
        }
        self.invalidate_sidebar_items_cache();
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn expand_all_dirs(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_filter_active() {
            self.set_filtered_directories_collapsed(false, cx);
            return;
        }
        let root = self.sidebar_root.clone();
        if root.as_os_str().is_empty() {
            return;
        }
        let epoch = self.sidebar_directory_epoch;
        let sort_mode = self.sidebar_sort_mode;
        cx.spawn(async move |this, cx| {
            let background_root = root.clone();
            let expanded_dirs = crate::platform::fs_task::run(
                "Expand sidebar directories",
                root.clone(),
                move || {
                    let mut expanded = HashSet::from([background_root.clone()]);
                    let mut pending = vec![background_root];
                    while let Some(dir) = pending.pop() {
                        let children =
                            load_sidebar_directory(&dir, sort_mode).map_err(|error| {
                                std::io::Error::new(
                                    error.kind(),
                                    format!("{}: {error}", dir.display()),
                                )
                            })?;
                        for child in children {
                            if child.is_dir && expanded.insert(child.path.clone()) {
                                pending.push(child.path);
                            }
                        }
                    }
                    Ok(expanded)
                },
            )
            .await;
            this.update(cx, |this, cx| {
                if this.sidebar_root != root || this.sidebar_directory_epoch != epoch {
                    return;
                }
                match expanded_dirs {
                    Ok(expanded_dirs) => {
                        this.expanded_dirs = expanded_dirs;
                        this.invalidate_sidebar_items_cache();
                        this.context_menu = None;
                    }
                    Err(error) => {
                        this.report_sidebar_fs_error(
                            "Expand sidebar directories",
                            &root,
                            error,
                            cx,
                        );
                    }
                }
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

    pub(crate) fn sidebar_items(&mut self) -> Rc<Vec<SidebarItem>> {
        if self.sidebar_filter_active() {
            return self.sidebar_filter.items.clone();
        }
        if self.sidebar_items_dirty {
            let mut items = Vec::new();
            if !self.sidebar_root.as_os_str().is_empty()
                && self.expanded_dirs.contains(&self.sidebar_root)
            {
                self.collect_sidebar_items(&self.sidebar_root, 1, &mut items);
            }
            self.sidebar_items_cache = Rc::new(items);
            self.sidebar_items_dirty = false;
        }

        self.sidebar_items_cache.clone()
    }
}

pub(super) struct DirectorySortKey {
    name: String,
    extension: String,
    is_dir: bool,
    created: SystemTime,
}

impl DirectorySortKey {
    pub(super) fn new(path: &Path, name: &str, is_dir: bool, created: SystemTime) -> Self {
        Self {
            name: name.to_lowercase(),
            extension: path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase(),
            is_dir,
            created,
        }
    }

    pub(super) fn compare(&self, other: &Self, mode: SidebarSortMode) -> std::cmp::Ordering {
        other.is_dir.cmp(&self.is_dir).then_with(|| match mode {
            SidebarSortMode::NameAsc => self.name.cmp(&other.name),
            SidebarSortMode::NameDesc => other.name.cmp(&self.name),
            SidebarSortMode::Type => self
                .extension
                .cmp(&other.extension)
                .then_with(|| self.name.cmp(&other.name)),
            SidebarSortMode::CreatedAsc => self
                .created
                .cmp(&other.created)
                .then_with(|| self.name.cmp(&other.name)),
            SidebarSortMode::CreatedDesc => other
                .created
                .cmp(&self.created)
                .then_with(|| self.name.cmp(&other.name)),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::{SidebarSortMode, load_sidebar_directory};

    #[test]
    fn sidebar_error_identity_does_not_depend_on_translated_text() {
        let error = super::SidebarFsError {
            operation: "Read sidebar directory".into(),
            path: "workspace".into(),
            message: "无法读取目录".into(),
        };
        assert!(error.matches("Read sidebar directory", std::path::Path::new("workspace")));
        assert!(!error.matches(
            "Read file list directory",
            std::path::Path::new("workspace")
        ));
        assert!(!error.matches("Read sidebar directory", std::path::Path::new("other")));
    }

    #[test]
    fn directory_snapshots_only_seed_missing_clean_counts() {
        use std::collections::{HashMap, HashSet};
        let root = std::path::PathBuf::from("workspace");
        let snapshots = HashMap::from([(
            root.clone(),
            vec![super::SidebarDirectoryChild {
                name: "one.md".into(),
                path: root.join("one.md"),
                is_dir: false,
            }],
        )]);
        let dirs = vec![root.clone()];
        let mut counts = HashMap::new();
        super::seed_directory_counts(
            &dirs,
            &mut counts,
            &HashSet::from([root.clone()]),
            &snapshots,
        );
        assert!(counts.is_empty());
        super::seed_directory_counts(&dirs, &mut counts, &HashSet::new(), &snapshots);
        assert_eq!(counts[&root], 1);
        counts.insert(root.clone(), 2);
        super::seed_directory_counts(&dirs, &mut counts, &HashSet::new(), &snapshots);
        assert_eq!(
            counts[&root], 2,
            "an older snapshot must not overwrite a fresh count"
        );
    }

    #[test]
    fn failed_sidebar_refresh_preserves_cached_children() {
        let root = std::path::PathBuf::from("offline-workspace");
        let child = super::SidebarDirectoryChild {
            name: "note.md".to_string(),
            path: root.join("note.md"),
            is_dir: false,
        };
        let mut cache = std::collections::HashMap::from([(root.clone(), vec![child])]);
        let result = super::apply_sidebar_directory_snapshot(
            &mut cache,
            root.clone(),
            Err(std::io::ErrorKind::TimedOut.into()),
        );
        assert!(result.is_err());
        assert_eq!(cache[&root].len(), 1);
        assert_eq!(cache[&root][0].name, "note.md");
        super::apply_sidebar_directory_snapshot(&mut cache, root.clone(), Ok(Vec::new())).unwrap();
        assert!(cache[&root].is_empty());
    }

    #[test]
    fn directory_provider_and_counts_distinguish_failure_from_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("unavailable");
        assert!(load_sidebar_directory(&missing, SidebarSortMode::NameAsc).is_err());
        assert!(super::count_dir_markdown_files(&missing).is_err());
        assert!(
            load_sidebar_directory(dir.path(), SidebarSortMode::NameAsc)
                .unwrap()
                .is_empty()
        );
        assert_eq!(super::count_dir_markdown_files(dir.path()).unwrap(), 0);
        fs::write(dir.path().join("note.md"), "note").unwrap();
        fs::create_dir(dir.path().join("folder.md")).unwrap();
        assert_eq!(super::count_dir_markdown_files(dir.path()).unwrap(), 1);
    }

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

        let children = load_sidebar_directory(&root, SidebarSortMode::NameAsc).unwrap();

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
