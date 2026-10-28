use std::path::PathBuf;
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, Window, div, prelude::*, px, svg,
};

use super::search::{
    SEARCH_CASE_ICON_SVG_PATH, SEARCH_CLOSE_ICON_SVG_PATH, SEARCH_COLLAPSE_ALL_ICON_SVG_PATH,
    SEARCH_EXPAND_ALL_ICON_SVG_PATH, SEARCH_FILTER_ICON_SVG_PATH, SEARCH_ICON_SVG_PATH,
    SEARCH_REGEX_ICON_SVG_PATH, SEARCH_WHOLE_WORD_ICON_SVG_PATH, find_search_matches,
};
use super::*;

fn search_dates() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, SystemTime>> {
    static DATES: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, SystemTime>>,
    > = std::sync::OnceLock::new();
    DATES.get_or_init(Default::default)
}

fn remember_search_date(path: &std::path::Path, metadata: &std::fs::Metadata) {
    let date = metadata
        .created()
        .or_else(|_| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut dates = search_dates()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if dates.len() >= 50_000 {
        dates.clear();
    }
    dates.insert(path.to_path_buf(), date);
}

fn take_pending_workspace_search_navigation(
    pending: &mut Option<(usize, PathBuf, WorkspaceSearchMatch)>,
    open_epoch: usize,
    active_path: Option<&std::path::Path>,
) -> Option<(PathBuf, WorkspaceSearchMatch)> {
    let is_current_target = pending.as_ref().is_some_and(|(epoch, path, _)| {
        *epoch == open_epoch && active_path == Some(path.as_path())
    });
    is_current_target
        .then(|| pending.take())
        .flatten()
        .map(|(_, path, search_match)| (path, search_match))
}

fn sort_search_results(results: &mut [WorkspaceSearchResult], mode: SidebarSortMode) {
    // Snapshot observations once: sorting never performs filesystem I/O or
    // observes changing timestamps while another worker loads dates.
    let dates = if matches!(
        mode,
        SidebarSortMode::CreatedAsc | SidebarSortMode::CreatedDesc
    ) {
        let cached = search_dates()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        results
            .iter()
            .filter_map(|result| {
                cached
                    .get(&result.path)
                    .map(|date| (result.path.clone(), *date))
            })
            .collect::<std::collections::HashMap<_, _>>()
    } else {
        Default::default()
    };
    results.sort_by(|left, right| {
        let order = if dates.is_empty() {
            std::cmp::Ordering::Equal
        } else {
            let order = dates.get(&left.path).cmp(&dates.get(&right.path));
            if mode == SidebarSortMode::CreatedDesc {
                order.reverse()
            } else {
                order
            }
        };
        order.then_with(|| compare_workspace_search_paths(mode, &left.path, &right.path))
    });
}

/// Include/exclude regexes matched against `/`-separated paths relative to
/// the workspace root. Exclusion prunes whole directories; inclusion only
/// decides which files are read.
#[derive(Clone, Default)]
pub(crate) struct SearchPathFilter {
    include: Option<regex::Regex>,
    exclude: Option<regex::Regex>,
}

impl SearchPathFilter {
    pub(crate) fn new(include: &str, exclude: &str) -> Result<Self, regex::Error> {
        let compile = |pattern: &str| {
            let pattern = pattern.trim();
            (!pattern.is_empty())
                .then(|| regex::Regex::new(pattern))
                .transpose()
        };
        Ok(Self {
            include: compile(include)?,
            exclude: compile(exclude)?,
        })
    }

    fn relative_path(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
        let relative = path.strip_prefix(root).ok()?;
        let parts = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>();
        (!parts.is_empty()).then(|| parts.join("/"))
    }

    fn excludes_dir(&self, root: &std::path::Path, dir: &std::path::Path) -> bool {
        let Some(exclude) = &self.exclude else {
            return false;
        };
        Self::relative_path(root, dir).is_some_and(|relative| exclude.is_match(&relative))
    }

    /// Index candidates arrive as bare file paths, so every ancestor is
    /// checked here too to honour directory exclusions a walk would prune.
    fn allows_file(&self, root: &std::path::Path, file: &std::path::Path) -> bool {
        let Some(relative) = Self::relative_path(root, file) else {
            return true;
        };
        if let Some(exclude) = &self.exclude {
            let ancestor_excluded = relative
                .match_indices('/')
                .any(|(index, _)| exclude.is_match(&relative[..index]));
            if ancestor_excluded || exclude.is_match(&relative) {
                return false;
            }
        }
        self.include
            .as_ref()
            .is_none_or(|include| include.is_match(&relative))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceSearchStatus {
    Empty,
    Searching,
    NoResults,
    Summary,
}

fn workspace_search_status(
    query_is_empty: bool,
    search_running: bool,
    result_count: usize,
) -> WorkspaceSearchStatus {
    if query_is_empty {
        WorkspaceSearchStatus::Empty
    } else if search_running {
        WorkspaceSearchStatus::Searching
    } else if result_count == 0 {
        WorkspaceSearchStatus::NoResults
    } else {
        WorkspaceSearchStatus::Summary
    }
}

fn begin_workspace_search_staging(
    visible_results: &mut Vec<WorkspaceSearchResult>,
    staging_results: &mut Vec<WorkspaceSearchResult>,
) {
    // Keep the currently painted result set until every batch for the new
    // query is ready; only the invisible staging buffer resets here.
    let _ = visible_results;
    staging_results.clear();
}

impl EditorView {
    /// Rendered width of the search column, which sits to the right of the
    /// FileList and keeps its own persisted width.
    pub(crate) fn workspace_search_panel_width(&self) -> f32 {
        if !self.workspace_search_visible || !self.sidebar_visible {
            return 0.0;
        }
        self.app_settings
            .workspace_search_width
            .unwrap_or_else(|| self.scaled(self.styles.sidebar.sidebar_width_px))
    }

    pub(crate) fn set_workspace_search_panel_width(&mut self, width: f32) {
        self.app_settings.workspace_search_width = Some(width);
    }

    pub(crate) fn finish_workspace_search_resize(&mut self, cx: &mut Context<Self>) {
        if !self.workspace_search_resizing {
            return;
        }
        self.workspace_search_resizing = false;
        self.app_settings.save();
        cx.notify();
    }

    pub(crate) fn sort_workspace_search_results(&mut self) {
        let sort_mode = self.sidebar_sort_mode;
        sort_search_results(&mut self.workspace_search_results, sort_mode);
    }

    pub(crate) fn on_find_in_files(
        &mut self,
        _: &FindInFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_search(window, cx);
    }

    pub(crate) fn toggle_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspaces.is_empty() {
            self.workspace_search_visible = false;
            self.workspace_search_task = None;
            cx.notify();
            return;
        }
        self.workspace_search_visible = !self.workspace_search_visible;
        if !self.workspace_search_visible {
            self.close_workspace_search(window, cx);
            return;
        }

        // The search column sits beside the sidebar, so the sidebar only has to
        // be brought back when it was hidden entirely.
        if !self.sidebar_visible {
            self.sidebar_visible = true;
            self.sync_sidebar_width_animation(cx);
        }
        if self.workspace_search_editor.is_none() {
            let styles = self.workspace_search_text_input_styles();
            let initial_query = self.workspace_search_query.clone();
            let placeholder = self
                .t(crate::i18n::TextKey::WorkspaceSearchPlaceholder)
                .to_string();
            let editor = cx.new(move |cx| {
                let mut input = crate::text_input::TextInput::new(styles, &placeholder, cx);
                if !initial_query.is_empty() {
                    input.set_text(&initial_query, cx);
                    input.select_all(cx);
                }
                input
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let query = this
                            .workspace_search_editor
                            .as_ref()
                            .map(|entity| entity.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.workspace_search_query = query;
                        this.run_workspace_search(cx);
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.commit_workspace_search_selected_match(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    crate::text_input::TextInputEvent::MoveUp => {
                        this.move_workspace_search_selection(-1, cx);
                    }
                    crate::text_input::TextInputEvent::MoveDown => {
                        this.move_workspace_search_selection(1, cx);
                    }
                },
            )
            .detach();
            self.workspace_search_editor = Some(editor);
        }

        if let Some(editor) = &self.workspace_search_editor {
            let focus_handle = editor.read(cx).focus_handle.clone();
            let focus_sub = cx.on_focus(&focus_handle, window, |this, _window, cx| {
                this.workspace_search_input_focused = true;
                cx.notify();
            });
            let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
                this.workspace_search_input_focused = false;
                cx.notify();
            });
            self.workspace_search_focus_sub = Some(focus_sub);
            self.workspace_search_blur_sub = Some(blur_sub);
        }

        let styles = self.workspace_search_text_input_styles();
        let query = self.workspace_search_query.clone();
        if let Some(editor) = &self.workspace_search_editor {
            editor.update(cx, |input, cx| {
                input.styles = styles;
                input.set_text(&query, cx);
                input.select_all(cx);
            });
            editor.focus_handle(cx).focus(window);
        }

        cx.notify();
    }

    pub(crate) fn close_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_search_visible = false;
        self.workspace_search_focus_sub = None;
        self.workspace_search_task = None;
        self.workspace_search_epoch = self.workspace_search_epoch.wrapping_add(1);
        self.workspace_search_has_more = false;
        self.workspace_search_blur_sub = None;
        self.workspace_search_input_focused = false;
        self.clear_workspace_search_preview_state();
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    pub(crate) fn clear_workspace_search_preview_state(&mut self) {
        self.workspace_search_selected_match = None;
        self.workspace_search_pending_navigation = None;
        self.workspace_search_highlight_path = None;
        self.workspace_search_highlight_ranges.clear();
        self.workspace_search_highlight_current_index = None;
    }

    pub(crate) fn workspace_search_file_collapsed(&self, path: &std::path::Path) -> bool {
        self.workspace_search_collapse_all != self.workspace_search_collapse_toggled.contains(path)
    }

    /// Match rows a result contributes to the list, which keyboard
    /// navigation and the row budget both follow.
    fn workspace_search_visible_matches<'a>(
        &self,
        result: &'a WorkspaceSearchResult,
    ) -> &'a [WorkspaceSearchMatch] {
        if self.workspace_search_file_collapsed(&result.path) {
            &[]
        } else {
            &result.matches[..result.matches.len().min(50)]
        }
    }

    fn workspace_search_resume_visible(&self, result: &WorkspaceSearchResult) -> bool {
        result.resume.is_some() && !self.workspace_search_file_collapsed(&result.path)
    }

    fn workspace_search_any_expanded(&self) -> bool {
        self.workspace_search_results
            .iter()
            .any(|result| !self.workspace_search_file_collapsed(&result.path))
    }

    fn toggle_workspace_search_file_collapsed(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.workspace_search_collapse_toggled.remove(&path) {
            self.workspace_search_collapse_toggled.insert(path);
        }
        // Match indices shift once rows fold, so a stale selection would
        // point at a different line.
        self.workspace_search_selected_match = None;
        cx.notify();
    }

    fn set_workspace_search_all_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.workspace_search_collapse_all = collapsed;
        self.workspace_search_collapse_toggled.clear();
        self.workspace_search_selected_match = None;
        cx.notify();
    }

    fn workspace_search_total_match_count(&self) -> usize {
        self.workspace_search_results
            .iter()
            .map(|result| self.workspace_search_visible_matches(result).len())
            .sum()
    }

    fn workspace_search_match_at(
        &self,
        target_index: usize,
    ) -> Option<(PathBuf, WorkspaceSearchMatch)> {
        let mut current_index = 0usize;
        for result in &self.workspace_search_results {
            for search_match in self.workspace_search_visible_matches(result) {
                if current_index == target_index {
                    return Some((result.path.clone(), search_match.clone()));
                }
                current_index += 1;
            }
        }
        None
    }

    fn workspace_search_child_index_for_match(&self, target_index: usize) -> Option<usize> {
        let mut current_index = 0usize;
        let mut child_index = 2usize;
        for result in &self.workspace_search_results {
            child_index += 1;
            for _ in self.workspace_search_visible_matches(result) {
                if current_index == target_index {
                    return Some(child_index);
                }
                current_index += 1;
                child_index += 1;
            }
            child_index += usize::from(self.workspace_search_resume_visible(result));
        }
        None
    }

    fn show_workspace_search_match(
        &mut self,
        path: PathBuf,
        search_match: WorkspaceSearchMatch,
        permanent: bool,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) {
        if permanent {
            self.open_path_in_permanent_tab(path.clone(), focus_editor, cx);
        } else {
            self.open_path_in_preview_tab(path.clone(), focus_editor, cx);
        }

        if self.current_file_path.as_deref() == Some(path.as_path()) {
            self.reveal_workspace_search_match(path, search_match, cx);
        } else {
            self.workspace_search_pending_navigation =
                Some((self.preview_open_epoch, path, search_match));
        }
    }

    fn reveal_workspace_search_match(
        &mut self,
        path: PathBuf,
        search_match: WorkspaceSearchMatch,
        cx: &mut Context<Self>,
    ) {
        self.workspace_search_pending_navigation = None;
        self.workspace_search_highlight_path = Some(path);
        self.workspace_search_highlight_ranges =
            vec![search_match.byte_offset..search_match.byte_end];
        self.workspace_search_highlight_current_index = Some(0);
        self.cursor.offset = search_match.byte_offset.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.center_cursor_in_view_once(cx);
    }

    pub(super) fn apply_pending_workspace_search_match(
        &mut self,
        open_epoch: usize,
        cx: &mut Context<Self>,
    ) {
        let Some((path, search_match)) = take_pending_workspace_search_navigation(
            &mut self.workspace_search_pending_navigation,
            open_epoch,
            self.current_file_path.as_deref(),
        ) else {
            return;
        };
        self.reveal_workspace_search_match(path, search_match, cx);
    }

    fn preview_workspace_search_match_at(
        &mut self,
        match_index: usize,
        focus_editor: bool,
        permanent: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((path, search_match)) = self.workspace_search_match_at(match_index) else {
            return;
        };
        self.workspace_search_selected_match = Some(match_index);
        if let Some(child_index) = self.workspace_search_child_index_for_match(match_index) {
            let needed_rows = child_index.saturating_sub(1);
            if needed_rows > self.workspace_search_render_limit {
                self.workspace_search_render_limit = super::list_viewport::SEARCH_LIST_POLICY
                    .ensure_index(
                        self.workspace_search_render_limit,
                        self.workspace_search_total_row_count(),
                        needed_rows,
                    );
            }
            self.workspace_search_scroll_handle
                .scroll_to_item(child_index);
        }
        self.show_workspace_search_match(path, search_match, permanent, focus_editor, cx);
    }

    fn move_workspace_search_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let total = self.workspace_search_total_match_count();
        if total == 0 {
            if self.workspace_search_has_more {
                self.schedule_workspace_search_batch(cx);
            }
            return;
        }

        let next_index = match self.workspace_search_selected_match {
            Some(current) => (current as isize + delta).clamp(0, total as isize - 1) as usize,
            None if delta >= 0 => 0,
            None => total - 1,
        };
        if delta > 0 && next_index + 1 >= total && self.workspace_search_has_more {
            self.schedule_workspace_search_batch(cx);
        }
        self.preview_workspace_search_match_at(next_index, false, false, cx);
    }

    fn commit_workspace_search_selected_match(&mut self, cx: &mut Context<Self>) {
        let total = self.workspace_search_total_match_count();
        if total == 0 {
            return;
        }

        let selected = self.workspace_search_selected_match.unwrap_or(0);
        self.preview_workspace_search_match_at(selected.min(total - 1), true, true, cx);
    }

    /// Continues scanning one oversized file from where the previous pass
    /// stopped, appending the new matches under the same file header.
    fn continue_workspace_search_in_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(resume) = self
            .workspace_search_results
            .iter()
            .find(|result| result.path == path)
            .and_then(|result| result.resume)
        else {
            return;
        };
        let query = self.workspace_search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        let case_sensitive = self.workspace_search_case_sensitive;
        let whole_word = self.workspace_search_whole_word;
        let use_regex = self.workspace_search_use_regex;
        let epoch = self.workspace_search_epoch;
        let scan_path = path.clone();

        if self.workspace_search_task.is_some()
            || file_watch::operation_suppressed("Continue search", &path)
        {
            return;
        }
        let root = self.sidebar_root.clone();
        self.workspace_search_task = Some(cx.spawn(async move |this, cx| {
            let scanned =
                crate::platform::fs_task::run("Continue search", path.clone(), move || {
                    let bytes = std::fs::read(&scan_path)?;
                    let content = String::from_utf8_lossy(&bytes).into_owned();
                    let mut offset = resume.next_byte_offset.min(content.len());
                    while !content.is_char_boundary(offset) {
                        offset -= 1;
                    }
                    Ok(search_file_window(
                        &content,
                        offset,
                        &query,
                        case_sensitive,
                        whole_word,
                        use_regex,
                    ))
                })
                .await;

            this.update(cx, |this, cx| {
                if this.workspace_search_epoch != epoch {
                    return;
                }
                this.workspace_search_task = None;
                if this.sidebar_root != root {
                    cx.notify();
                    return;
                }
                let (matches, next_resume) = match scanned {
                    Ok(scanned) => scanned,
                    Err(error) => {
                        this.notify_fs_error("Continue search", &path, error, cx);
                        return;
                    }
                };
                let Some(result) = this
                    .workspace_search_results
                    .iter_mut()
                    .find(|result| result.path == path)
                else {
                    return;
                };
                result.matches.extend(matches);
                result.resume = next_resume;
                this.workspace_search_render_limit = super::list_viewport::SEARCH_LIST_POLICY
                    .ensure_index(
                        this.workspace_search_render_limit,
                        this.workspace_search_total_row_count(),
                        this.workspace_search_total_row_count(),
                    );
                cx.notify();
            })
            .ok();
        }));
    }

    fn workspace_search_total_row_count(&self) -> usize {
        self.workspace_search_results
            .iter()
            .map(|result| {
                1 + self.workspace_search_visible_matches(result).len()
                    + usize::from(self.workspace_search_resume_visible(result))
            })
            .sum()
    }

    fn workspace_search_rendered_child_count(&self) -> usize {
        let trimmed_query = self.workspace_search_query.trim();
        if trimmed_query.is_empty() {
            return 1;
        }
        if self.workspace_search_results.is_empty() {
            return 2;
        }

        2 + self
            .workspace_search_render_limit
            .min(self.workspace_search_total_row_count())
    }

    fn maybe_extend_workspace_search_results(&mut self, cx: &mut Context<Self>) {
        let total_rows = self.workspace_search_total_row_count();
        let rendered_children = self.workspace_search_rendered_child_count();
        let bottom_item = self.workspace_search_scroll_handle.bottom_item();
        let near_loaded_end = bottom_item
            .saturating_add(super::list_viewport::SEARCH_LIST_POLICY.preload_items)
            >= rendered_children.saturating_sub(1);
        if near_loaded_end && self.workspace_search_has_more {
            self.schedule_workspace_search_batch(cx);
        }
        if total_rows <= self.workspace_search_render_limit {
            return;
        }
        let next = super::list_viewport::SEARCH_LIST_POLICY.extend_near_end(
            self.workspace_search_render_limit,
            total_rows,
            bottom_item.min(rendered_children),
        );
        if next != self.workspace_search_render_limit {
            self.workspace_search_render_limit = next;
            cx.notify();
        }
    }

    pub(crate) fn workspace_search_path_filter(&self) -> Result<SearchPathFilter, regex::Error> {
        SearchPathFilter::new(
            &self.workspace_search_include,
            &self.workspace_search_exclude,
        )
    }

    pub(crate) fn run_workspace_search(&mut self, cx: &mut Context<Self>) {
        let query = self.workspace_search_query.trim().to_string();
        self.clear_workspace_search_preview_state();
        self.workspace_search_collapse_toggled.clear();
        self.workspace_search_filter_error = self.workspace_search_path_filter().is_err();
        if query.is_empty() || self.workspace_search_filter_error {
            self.workspace_search_results.clear();
            self.workspace_search_staging_results.clear();
            self.workspace_search_task = None;
            self.workspace_search_render_limit =
                super::list_viewport::SEARCH_LIST_POLICY.initial_count;
            self.workspace_search_pending_paths.clear();
            self.workspace_search_has_more = false;
            self.workspace_search_epoch = self.workspace_search_epoch.wrapping_add(1);
            cx.notify();
            return;
        }

        let root = self.sidebar_root.clone();
        if root.as_os_str().is_empty() {
            self.workspace_search_results.clear();
            self.workspace_search_staging_results.clear();
            self.workspace_search_task = None;
            self.workspace_search_render_limit =
                super::list_viewport::SEARCH_LIST_POLICY.initial_count;
            self.workspace_search_pending_paths.clear();
            self.workspace_search_has_more = false;
            self.workspace_search_epoch = self.workspace_search_epoch.wrapping_add(1);
            cx.notify();
            return;
        }

        self.workspace_search_epoch = self.workspace_search_epoch.wrapping_add(1);
        self.workspace_search_task = None;
        if file_watch::operation_suppressed("Search workspace", &root)
            || file_watch::operation_suppressed("Prepare search", &root)
        {
            self.workspace_search_staging_results.clear();
            self.workspace_search_pending_paths.clear();
            self.workspace_search_has_more = false;
            cx.notify();
            return;
        }
        begin_workspace_search_staging(
            &mut self.workspace_search_results,
            &mut self.workspace_search_staging_results,
        );
        self.workspace_search_pending_paths.clear();
        self.workspace_search_has_more = true;
        self.ensure_workspace_search_index(cx);
        self.seed_workspace_search_paths(root, cx);
    }

    /// Compute the initial `pending_paths` for a search: candidate files from
    /// the tantivy index when it can answer the query, otherwise the workspace
    /// root for a full scan. Runs the index query off the UI thread and then
    /// starts the first scan batch.
    fn seed_workspace_search_paths(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let query = self.workspace_search_query.trim().to_string();
        let use_regex = self.workspace_search_use_regex;
        let index = self
            .workspace_search_index
            .clone()
            .filter(|index| index.root() == root);
        let dirty_paths: Vec<PathBuf> = self
            .workspace_search_index_dirty_paths
            .iter()
            .cloned()
            .collect();
        let epoch = self.workspace_search_epoch;
        let fallback_root = root.clone();
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            if this
                .read_with(cx, |editor, _| {
                    editor.workspace_search_epoch != epoch || editor.sidebar_root != fallback_root
                })
                .unwrap_or(true)
            {
                return;
            }
            let seed =
                crate::platform::fs_task::run_value("Prepare search", root.clone(), move || {
                    let index = index?;
                    let mut candidates = index.candidate_paths(&query, use_regex)?;
                    candidates.extend(dirty_paths);
                    candidates.retain(|path| path.starts_with(&root));
                    candidates.sort();
                    candidates.dedup();
                    // `search_directory_batch` pops from the end, so reverse to
                    // keep alphabetical result order.
                    candidates.reverse();
                    Some(candidates)
                })
                .await;

            this.update(cx, |this, cx| {
                if this.workspace_search_epoch != epoch {
                    return;
                }
                this.workspace_search_task = None;
                if this.sidebar_root != fallback_root {
                    this.workspace_search_has_more = false;
                    cx.notify();
                    return;
                }
                let seed = match seed {
                    Ok(seed) => seed,
                    Err(error) => {
                        this.workspace_search_has_more = false;
                        this.workspace_search_staging_results.clear();
                        this.notify_fs_error("Prepare search", &fallback_root, error, cx);
                        return;
                    }
                };
                this.workspace_search_pending_paths = seed.unwrap_or_else(|| vec![fallback_root]);
                this.workspace_search_has_more = !this.workspace_search_pending_paths.is_empty();
                this.schedule_workspace_search_batch(cx);
                cx.notify();
            })
            .ok();
        });
        self.workspace_search_task = Some(task);
    }

    pub(crate) fn invalidate_workspace_search_index(&mut self) {
        self.workspace_search_index_epoch = self.workspace_search_index_epoch.wrapping_add(1);
        self.workspace_search_index_task = None;
        self.workspace_search_index = None;
        self.workspace_search_index_dirty_paths.clear();
    }

    /// Build (or rebuild) the workspace search index in the background. A
    /// rebuild is triggered when there is no index for the current workspace
    /// or when enough files changed since the last snapshot.
    pub(crate) fn ensure_workspace_search_index(&mut self, cx: &mut Context<Self>) {
        const REBUILD_DIRTY_THRESHOLD: usize = 256;

        let root = self.sidebar_root.clone();
        if root.as_os_str().is_empty() || file_watch::operation_suppressed("Index workspace", &root)
        {
            return;
        }
        if self.workspace_search_index_task.is_some() {
            return;
        }
        let stale_root = self
            .workspace_search_index
            .as_ref()
            .is_some_and(|index| index.root() != root);
        if stale_root {
            self.workspace_search_index = None;
            self.workspace_search_index_dirty_paths.clear();
        }
        let needs_rebuild = self.workspace_search_index.is_none()
            || self.workspace_search_index_dirty_paths.len() > REBUILD_DIRTY_THRESHOLD;
        if !needs_rebuild {
            return;
        }
        self.workspace_search_index_epoch = self.workspace_search_index_epoch.wrapping_add(1);
        let epoch = self.workspace_search_index_epoch;
        let dirty_snapshot: Vec<PathBuf> = self
            .workspace_search_index_dirty_paths
            .iter()
            .cloned()
            .collect();
        let task = cx.spawn(async move |this, cx| {
            let build_root = root.clone();
            let built =
                crate::platform::fs_task::run_anyhow("Index workspace", root.clone(), move || {
                    super::search_index::WorkspaceSearchIndex::build(&build_root)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.workspace_search_index_epoch != epoch {
                    return;
                }
                this.workspace_search_index_task = None;
                if this.sidebar_root != root {
                    this.ensure_workspace_search_index(cx);
                    return;
                }
                if let Ok(index) = built {
                    this.workspace_search_index = Some(std::sync::Arc::new(index));
                    for path in &dirty_snapshot {
                        this.workspace_search_index_dirty_paths.remove(path);
                    }
                } else if let Err(error) = built {
                    this.notify_fs_error("Index workspace", &root, error, cx);
                }
            })
            .ok();
        });
        self.workspace_search_index_task = Some(task);
    }

    /// Record workspace files changed since the index snapshot; they are
    /// unioned into every candidate set so search results never go stale.
    pub(crate) fn mark_workspace_search_index_dirty(&mut self, paths: &[PathBuf]) {
        if self.workspace_search_index.is_none() && self.workspace_search_index_task.is_none() {
            return;
        }
        for path in paths {
            self.workspace_search_index_dirty_paths.insert(path.clone());
        }
    }

    fn schedule_workspace_search_batch(&mut self, cx: &mut Context<Self>) {
        if self.workspace_search_task.is_some() || !self.workspace_search_has_more {
            return;
        }
        let query = self.workspace_search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        let case_sensitive = self.workspace_search_case_sensitive;
        let whole_word = self.workspace_search_whole_word;
        let use_regex = self.workspace_search_use_regex;
        let path_filter = self.workspace_search_path_filter().unwrap_or_default();
        let root = self.sidebar_root.clone();
        if file_watch::operation_suppressed("Search workspace", &root) {
            self.workspace_search_has_more = false;
            self.workspace_search_staging_results.clear();
            cx.notify();
            return;
        }
        let pending_paths = std::mem::take(&mut self.workspace_search_pending_paths);
        let epoch = self.workspace_search_epoch;
        let task = cx.spawn(async move |this, cx| {
            let scan_root = root.clone();
            let batch =
                crate::platform::fs_task::run("Search workspace", root.clone(), move || {
                    search_directory_batch(
                        &scan_root,
                        pending_paths,
                        &query,
                        case_sensitive,
                        whole_word,
                        use_regex,
                        &path_filter,
                        super::list_viewport::SEARCH_LIST_POLICY.batch_size,
                    )
                })
                .await;

            this.update(cx, |this, cx| {
                if this.workspace_search_epoch != epoch {
                    // The query may have changed, but a blocked read still
                    // describes this workspace and must stop replacement work.
                    if this.sidebar_root == root
                        && let Err(error) = batch
                    {
                        this.notify_fs_error("Search workspace", &root, error, cx);
                    }
                    return;
                }
                this.workspace_search_task = None;
                if this.sidebar_root != root {
                    this.workspace_search_has_more = false;
                    cx.notify();
                    return;
                }
                let batch = match batch {
                    Ok(batch) => batch,
                    Err(error) => {
                        this.workspace_search_has_more = false;
                        this.workspace_search_pending_paths.clear();
                        this.workspace_search_staging_results.clear();
                        this.notify_fs_error("Search workspace", &root, error, cx);
                        return;
                    }
                };
                this.workspace_search_staging_results.extend(batch.results);
                this.workspace_search_pending_paths = batch.pending_paths;
                this.workspace_search_has_more = !this.workspace_search_pending_paths.is_empty();
                if this.workspace_search_has_more {
                    this.schedule_workspace_search_batch(cx);
                } else {
                    let mut results = std::mem::take(&mut this.workspace_search_staging_results);
                    sort_search_results(&mut results, this.sidebar_sort_mode);
                    this.workspace_search_results = results;
                    this.workspace_search_render_limit =
                        super::list_viewport::SEARCH_LIST_POLICY.initial_count;
                    this.workspace_search_scroll_handle.scroll_to_item(0);
                }
                cx.notify();
            })
            .ok();
        });
        self.workspace_search_task = Some(task);
    }

    fn new_workspace_search_filter_input(
        &mut self,
        include: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<crate::text_input::TextInput> {
        let styles = self.workspace_search_filter_text_input_styles();
        let placeholder = crate::i18n::search_option_label(
            self.ui_language,
            if include {
                "include_placeholder"
            } else {
                "exclude_placeholder"
            },
        );
        let initial = if include {
            self.workspace_search_include.clone()
        } else {
            self.workspace_search_exclude.clone()
        };
        let editor = cx.new(move |cx| {
            let mut input = crate::text_input::TextInput::new(styles, placeholder, cx);
            input.set_text(&initial, cx);
            input
        });
        cx.subscribe(
            &editor,
            move |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Changed => {
                    let text = editor.read(cx).text().to_string();
                    if include {
                        this.workspace_search_include = text;
                    } else {
                        this.workspace_search_exclude = text;
                    }
                    this.run_workspace_search(cx);
                }
                crate::text_input::TextInputEvent::Submit
                | crate::text_input::TextInputEvent::SubmitBackward => {
                    this.commit_workspace_search_selected_match(cx);
                }
                crate::text_input::TextInputEvent::Escape => {}
                crate::text_input::TextInputEvent::MoveUp => {
                    this.move_workspace_search_selection(-1, cx);
                }
                crate::text_input::TextInputEvent::MoveDown => {
                    this.move_workspace_search_selection(1, cx);
                }
            },
        )
        .detach();
        editor
    }

    fn toggle_workspace_search_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_search_filters_visible = !self.workspace_search_filters_visible;
        if self.workspace_search_filters_visible {
            if self.workspace_search_include_editor.is_none() {
                self.workspace_search_include_editor =
                    Some(self.new_workspace_search_filter_input(true, cx));
            }
            if self.workspace_search_exclude_editor.is_none() {
                self.workspace_search_exclude_editor =
                    Some(self.new_workspace_search_filter_input(false, cx));
            }
            if let Some(editor) = &self.workspace_search_include_editor {
                editor.focus_handle(cx).focus(window);
            }
        } else if let Some(editor) = &self.workspace_search_editor {
            editor.focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    /// Include/exclude fields carry no leading search icon.
    pub(crate) fn workspace_search_filter_text_input_styles(
        &self,
    ) -> crate::style::TextInputStyles {
        self.styles
            .input_styles(crate::style::InputRole::WorkspaceFilter, self.ui_scale)
    }

    pub(crate) fn workspace_search_text_input_styles(&self) -> crate::style::TextInputStyles {
        self.styles
            .input_styles(crate::style::InputRole::WorkspaceSearch, self.ui_scale)
    }

    /// Standalone column to the right of the sidebar, with its own trailing
    /// hairline so it reads as a separate panel.
    pub(crate) fn render_workspace_search_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = &self.styles.sidebar;
        crate::ui::panel::panel(ss.panel())
            .id("workspace-search-column")
            .w(px(self.workspace_search_panel_width()))
            .flex_none()
            .child(self.render_workspace_search(cx))
            .into_any_element()
    }

    pub(crate) fn render_workspace_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let trimmed_query = self.workspace_search_query.trim();
        let workspace_styles = &self.styles.workspace_search;
        let input_height = self.scaled(workspace_styles.ws_search_input_height_px);
        let button_size = self.scaled(workspace_styles.ws_search_btn_size_px);
        let button_icon_size = self.scaled(workspace_styles.ws_search_btn_icon_size_px);
        let button_gap = self.scaled(1.0);
        let input_gap = self.scaled(workspace_styles.ws_search_input_gap_px);
        let icon_size = self.scaled(workspace_styles.ws_search_input_icon_size_px);
        let chrome_padding_x = ((input_height - button_size).max(0.0) / 2.0).round();
        let input_left_inset = self.scaled(workspace_styles.ws_search_input_left_inset_px);
        let input_left = (input_left_inset - icon_size).max(0.0) / 2.0;
        let icon_top = (input_height - icon_size).max(0.0) / 2.0;
        let button_radius = self.scaled(4.0);
        let button_hover_bg = workspace_styles.ws_search_btn_hover_bg;
        let button_icon_color = workspace_styles.ws_search_btn_icon_color;
        let button_active_icon_color = workspace_styles.ws_search_btn_active_icon_color;
        let input_icon_color = workspace_styles.ws_search_input_icon_color;
        let search_border_width = self.scaled(workspace_styles.ws_search_input_border_width_px);
        let search_border_color = workspace_styles.ws_search_input_border_color;
        let search_is_active = self.workspace_search_input_focused || !trimmed_query.is_empty();
        let search_bg = if search_is_active {
            workspace_styles.ws_search_input_focus_bg
        } else {
            workspace_styles.ws_search_input_bg
        };
        let search_hover_bg = if search_is_active {
            workspace_styles.ws_search_input_focus_bg
        } else {
            workspace_styles.ws_search_input_hover_bg
        };
        let filters_active = self.workspace_search_filters_visible
            || !self.workspace_search_include.trim().is_empty()
            || !self.workspace_search_exclude.trim().is_empty();

        let filter_field = |editor: Option<gpui::Entity<crate::text_input::TextInput>>,
                            has_text: bool| {
            let background = if has_text {
                workspace_styles.ws_search_input_focus_bg
            } else {
                workspace_styles.ws_search_input_bg
            };
            let hover_background = if has_text {
                workspace_styles.ws_search_input_focus_bg
            } else {
                workspace_styles.ws_search_input_hover_bg
            };
            div()
                .flex_1()
                .min_w(px(0.0))
                .h(px(input_height))
                .bg(background)
                .hover(move |style| style.bg(hover_background))
                .border(px(search_border_width))
                .border_color(search_border_color)
                .rounded(px(self.scaled(workspace_styles.ws_search_input_radius_px)))
                .flex()
                .items_center()
                .when_some(editor, |this, editor| this.child(editor))
        };
        let filters = self.workspace_search_filters_visible.then(|| {
            div()
                .w_full()
                .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
                .pt(px(self.scaled(
                    self.styles.ui.space(crate::style::SpaceStep::Half),
                )))
                .pb(px(self.scaled(
                    self.styles.ui.space(crate::style::SpaceStep::Half),
                )))
                .flex()
                .gap(px(
                    self.scaled(self.styles.ui.space(crate::style::SpaceStep::One))
                ))
                .child(filter_field(
                    self.workspace_search_include_editor.clone(),
                    !self.workspace_search_include.trim().is_empty(),
                ))
                .child(filter_field(
                    self.workspace_search_exclude_editor.clone(),
                    !self.workspace_search_exclude.trim().is_empty(),
                ))
        });

        let input = div()
            .w_full()
            .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
            .pt(px(self.scaled(workspace_styles.ws_search_padding_px)))
            .pb(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Half),
            )))
            .child(
                div()
                    .w_full()
                    .h(px(input_height))
                    .px(px(chrome_padding_x))
                    .bg(search_bg)
                    .hover(move |style| style.bg(search_hover_bg))
                    .border(px(search_border_width))
                    .border_color(search_border_color)
                    .rounded(px(self.scaled(workspace_styles.ws_search_input_radius_px)))
                    .flex()
                    .items_center()
                    .gap(px(input_gap))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .when_some(self.workspace_search_editor.clone(), |this, editor| {
                                this.child(editor)
                            })
                            .child(
                                div()
                                    .absolute()
                                    .left(px(input_left))
                                    .top(px(icon_top))
                                    .w(px(icon_size))
                                    .h(px(icon_size))
                                    .child(
                                        svg()
                                            .path(SEARCH_ICON_SVG_PATH)
                                            .size(px(icon_size))
                                            .text_color(input_icon_color),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(button_gap))
                            .child(
                                self.with_search_tooltip(
                                    crate::ui::button::icon_button(
                                        "workspace-search-case-btn",
                                        self.styles.workspace_toggle(),
                                        crate::style::ControlState {
                                            selected: self.workspace_search_case_sensitive,
                                            ..Default::default()
                                        },
                                        self.ui_scale,
                                    )
                                    .child(
                                        svg()
                                            .path(SEARCH_CASE_ICON_SVG_PATH)
                                            .size(px(button_icon_size))
                                            .text_color(if self.workspace_search_case_sensitive {
                                                button_active_icon_color
                                            } else {
                                                button_icon_color
                                            }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.workspace_search_case_sensitive =
                                                !this.workspace_search_case_sensitive;
                                            this.run_workspace_search(cx);
                                        }),
                                    ),
                                    "case",
                                    Some(crate::editor::shortcut::hint("secondary-alt-c")),
                                    cx,
                                ),
                            )
                            .child(
                                self.with_search_tooltip(
                                    crate::ui::button::icon_button(
                                        "workspace-search-word-btn",
                                        self.styles.workspace_toggle(),
                                        crate::style::ControlState {
                                            selected: self.workspace_search_whole_word,
                                            ..Default::default()
                                        },
                                        self.ui_scale,
                                    )
                                    .child(
                                        svg()
                                            .path(SEARCH_WHOLE_WORD_ICON_SVG_PATH)
                                            .size(px(button_icon_size))
                                            .text_color(if self.workspace_search_whole_word {
                                                button_active_icon_color
                                            } else {
                                                button_icon_color
                                            }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.workspace_search_whole_word =
                                                !this.workspace_search_whole_word;
                                            this.run_workspace_search(cx);
                                        }),
                                    ),
                                    "word",
                                    Some(crate::editor::shortcut::hint("secondary-alt-w")),
                                    cx,
                                ),
                            )
                            .child(
                                self.with_search_tooltip(
                                    crate::ui::button::icon_button(
                                        "workspace-search-regex-btn",
                                        self.styles.workspace_toggle(),
                                        crate::style::ControlState {
                                            selected: self.workspace_search_use_regex,
                                            ..Default::default()
                                        },
                                        self.ui_scale,
                                    )
                                    .child(
                                        svg()
                                            .path(SEARCH_REGEX_ICON_SVG_PATH)
                                            .size(px(button_icon_size))
                                            .text_color(if self.workspace_search_use_regex {
                                                button_active_icon_color
                                            } else {
                                                button_icon_color
                                            }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.workspace_search_use_regex =
                                                !this.workspace_search_use_regex;
                                            this.run_workspace_search(cx);
                                        }),
                                    ),
                                    "regex",
                                    Some(crate::editor::shortcut::hint("secondary-alt-r")),
                                    cx,
                                ),
                            )
                            .child(
                                self.with_search_tooltip(
                                    crate::ui::button::icon_button(
                                        "workspace-search-filters-btn",
                                        self.styles.workspace_toggle(),
                                        crate::style::ControlState {
                                            selected: self.workspace_search_filters_visible,
                                            ..Default::default()
                                        },
                                        self.ui_scale,
                                    )
                                    .child(
                                        svg()
                                            .path(SEARCH_FILTER_ICON_SVG_PATH)
                                            .size(px(button_icon_size))
                                            .text_color(if filters_active {
                                                button_active_icon_color
                                            } else {
                                                button_icon_color
                                            }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.toggle_workspace_search_filters(window, cx);
                                        }),
                                    ),
                                    "filters",
                                    None,
                                    cx,
                                ),
                            )
                            .child(
                                self.with_search_tooltip(
                                    div()
                                        .id("workspace-search-close-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .hover(move |style| style.bg(button_hover_bg))
                                        .child(
                                            svg()
                                                .path(SEARCH_CLOSE_ICON_SVG_PATH)
                                                .size(px(button_icon_size))
                                                .text_color(button_icon_color),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.close_workspace_search(window, cx);
                                            }),
                                        ),
                                    "close",
                                    None,
                                    cx,
                                ),
                            ),
                    ),
            )
            .into_any_element();

        let total_matches: usize = self
            .workspace_search_results
            .iter()
            .map(|result| result.matches.len())
            .sum();
        let file_count = self.workspace_search_results.len();
        let show_sort = !trimmed_query.is_empty() && file_count > 0;
        let status_text = if self.workspace_search_filter_error {
            crate::i18n::search_option_label(self.ui_language, "invalid_regex").to_string()
        } else {
            match workspace_search_status(
                trimmed_query.is_empty(),
                self.workspace_search_task.is_some(),
                file_count,
            ) {
                WorkspaceSearchStatus::Empty => String::new(),
                WorkspaceSearchStatus::Searching => {
                    self.t(crate::i18n::TextKey::Searching).to_string()
                }
                WorkspaceSearchStatus::NoResults => {
                    self.t(crate::i18n::TextKey::SearchNoResults).to_string()
                }
                WorkspaceSearchStatus::Summary => crate::i18n::workspace_search_summary(
                    self.ui_language,
                    total_matches,
                    file_count,
                ),
            }
        };
        let any_expanded = self.workspace_search_any_expanded();
        let status = div()
            .w_full()
            .h(px(self.scaled(workspace_styles.ws_search_summary_height_px)))
            .mb(px(self.scaled(
                workspace_styles.ws_search_summary_margin_bottom_px,
            )))
            .flex_none()
            .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(
                self.scaled(workspace_styles.ws_search_summary_font_size_px)
            ))
            .text_color(workspace_styles.ws_search_summary_text_color)
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .opacity(0.6)
                    .child(status_text),
            )
            .when(show_sort, |this| {
                this.child(self.render_sidebar_header_action_button(
                    "workspace-search-collapse-all",
                    if any_expanded {
                        SEARCH_COLLAPSE_ALL_ICON_SVG_PATH
                    } else {
                        SEARCH_EXPAND_ALL_ICON_SVG_PATH
                    },
                    crate::i18n::search_option_label(
                        self.ui_language,
                        if any_expanded {
                            "collapse_all"
                        } else {
                            "expand_all"
                        },
                    ),
                    None,
                    cx,
                    |this, _event, _window, cx| {
                        let collapse = this.workspace_search_any_expanded();
                        this.set_workspace_search_all_collapsed(collapse, cx);
                    },
                ))
                .child(self.render_sidebar_header_action_button(
                    "workspace-search-sort",
                    FILETREE_SORT_SVG_PATH,
                    crate::i18n::sidebar_action_label(self.ui_language, "sort"),
                    None,
                    cx,
                    |this, event, _window, cx| {
                        this.open_sidebar_sort_menu(event.position, cx);
                    },
                ))
            })
            .into_any_element();

        let mut result_children: Vec<AnyElement> = Vec::new();
        if !trimmed_query.is_empty() && !self.workspace_search_results.is_empty() {
            let mut remaining_rows = self.workspace_search_render_limit;
            let mut match_index = 0usize;
            'files: for (result_index, result) in self.workspace_search_results.iter().enumerate() {
                if remaining_rows == 0 {
                    break;
                }
                let relative_path = self
                    .relative_path_for_path(&result.path)
                    .or_else(|| {
                        result
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                    .unwrap_or_default();
                let relative_path = std::path::Path::new(&relative_path);
                let file_name = relative_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| relative_path.to_string_lossy().into_owned());
                let directory_path = relative_path
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .filter(|parent| !parent.is_empty() && parent != ".");
                let match_count = result.matches.len();
                let is_collapsed = self.workspace_search_file_collapsed(&result.path);
                let is_last_result = result_index + 1 == self.workspace_search_results.len();
                let show_bottom_border = !is_collapsed || is_last_result;
                let header_path = result.path.clone();
                let file_header_hover_bg = workspace_styles.ws_search_match_hover_bg;
                let file_header_border_width =
                    px(workspace_styles.ws_search_file_header_border_width_px);
                let file_header_border_color = workspace_styles.ws_search_file_header_border_color;

                result_children.push(
                    div()
                        .w_full()
                        .h(px(
                            self.scaled(workspace_styles.ws_search_file_header_height_px)
                        ))
                        .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
                        .flex()
                        .items_center()
                        .gap(px(
                            self.scaled(self.styles.ui.space(crate::style::SpaceStep::One))
                        ))
                        .bg(workspace_styles.ws_search_file_header_bg)
                        .relative()
                        .border_t(file_header_border_width)
                        .border_b(file_header_border_width)
                        .border_color(gpui::transparent_black())
                        .cursor(CursorStyle::Arrow)
                        .hover(move |style| style.bg(file_header_hover_bg))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_workspace_search_file_collapsed(
                                    header_path.clone(),
                                    cx,
                                );
                            }),
                        )
                        .child(
                            svg()
                                .path(FILETREE_FOLD_SVG_PATH)
                                .flex_none()
                                .size(px(
                                    self.scaled(self.styles.sidebar.sidebar_item_fold_icon_size_px)
                                ))
                                .text_color(self.styles.sidebar.sidebar_item_fold_icon_color)
                                .when(!is_collapsed, |icon| {
                                    icon.with_transformation(gpui::Transformation::rotate(
                                        gpui::radians(std::f32::consts::FRAC_PI_2),
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex()
                                .items_center()
                                .gap(px(
                                    self.scaled(workspace_styles.ws_search_file_header_dir_gap_px)
                                ))
                                .text_size(px(self
                                    .scaled(workspace_styles.ws_search_file_header_font_size_px)))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(
                                            workspace_styles.ws_search_file_header_text_color,
                                        )
                                        .font_weight(FontWeight::BOLD)
                                        .child(file_name),
                                )
                                .when_some(directory_path, |this, directory_path| {
                                    this.child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_color(
                                                workspace_styles
                                                    .ws_search_file_header_dir_text_color,
                                            )
                                            .child(directory_path),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(self.scaled(
                                    workspace_styles.ws_search_file_header_count_font_size_px,
                                )))
                                .text_color(workspace_styles.ws_search_file_header_count_text_color)
                                .opacity(0.4)
                                .child(format!("{match_count}")),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(file_header_border_width)
                                .bg(file_header_border_color),
                        )
                        .when(show_bottom_border, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .left_0()
                                    .right_0()
                                    .h(file_header_border_width)
                                    .bg(file_header_border_color),
                            )
                        })
                        .into_any_element(),
                );
                remaining_rows = remaining_rows.saturating_sub(1);
                if remaining_rows == 0 {
                    break;
                }

                let hover_bg = workspace_styles.ws_search_match_hover_bg;
                let hl_bg = workspace_styles.ws_search_match_highlight_bg;
                let hl_color = workspace_styles.ws_search_match_highlight_text_color;
                for search_match in self.workspace_search_visible_matches(result) {
                    if remaining_rows == 0 {
                        break 'files;
                    }
                    let current_match_index = match_index;
                    match_index += 1;
                    let path = result.path.clone();
                    let path_for_click = path.clone();
                    let search_match_for_click = search_match.clone();
                    let line_num = search_match.line_number;
                    let line_text = search_match.line_text.clone();
                    let start = search_match.match_start;
                    let end = search_match.match_end;
                    let is_selected =
                        self.workspace_search_selected_match == Some(current_match_index);

                    let (before, matched, after) = self.workspace_search_match_excerpt(
                        &line_text,
                        start,
                        end,
                        workspace_styles.ws_search_match_font_size_px,
                    );

                    result_children.push(
                        crate::ui::list::list_item(
                            self.styles.workspace_match_row(),
                            crate::style::ControlState {
                                selected: is_selected,
                                ..Default::default()
                            },
                            self.ui_scale,
                        )
                        .id(gpui::SharedString::from(format!(
                            "ws-match-{}-{}-{}",
                            result.path.display(),
                            line_num,
                            start
                        )))
                        .child(
                            div()
                                .min_w(px(self.scaled(28.0)))
                                .text_size(px(
                                    self.scaled(workspace_styles.ws_search_match_font_size_px)
                                ))
                                .text_color(workspace_styles.ws_search_match_line_number_color)
                                .opacity(0.4)
                                .child(format!("{line_num}")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(
                                    self.scaled(workspace_styles.ws_search_match_font_size_px)
                                ))
                                .text_color(workspace_styles.ws_search_match_text_color)
                                .flex()
                                .child(before)
                                .child(div().bg(hl_bg).text_color(hl_color).child(matched))
                                .child(after),
                        )
                        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                            let is_double_click = event.click_count() >= 2;
                            cx.stop_propagation();
                            if is_double_click {
                                this.preview_workspace_search_match_at(
                                    current_match_index,
                                    true,
                                    true,
                                    cx,
                                );
                            } else {
                                this.workspace_search_selected_match = Some(current_match_index);
                                if this.switch_to_existing_editor_tab_for_path(
                                    &path_for_click,
                                    true,
                                    cx,
                                ) {
                                    this.reveal_workspace_search_match(
                                        path_for_click.clone(),
                                        search_match_for_click.clone(),
                                        cx,
                                    );
                                } else {
                                    this.show_workspace_search_match(
                                        path_for_click.clone(),
                                        search_match_for_click.clone(),
                                        false,
                                        false,
                                        cx,
                                    );
                                }
                            }
                        }))
                        .into_any_element(),
                    );
                    remaining_rows = remaining_rows.saturating_sub(1);
                }

                if let Some(resume) = result.resume.filter(|_| !is_collapsed) {
                    if remaining_rows == 0 {
                        break 'files;
                    }
                    let resume_path = result.path.clone();
                    result_children.push(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "ws-resume-{}",
                                result.path.display()
                            )))
                            .w_full()
                            .h(px(self.scaled(workspace_styles.ws_search_match_height_px)))
                            .px(px(
                                self.scaled(workspace_styles.ws_search_match_padding_x_px)
                            ))
                            .flex()
                            .items_center()
                            .cursor(CursorStyle::PointingHand)
                            .text_size(px(
                                self.scaled(workspace_styles.ws_search_match_font_size_px)
                            ))
                            .text_color(workspace_styles.ws_search_file_header_text_color)
                            .hover(move |style| style.bg(hover_bg))
                            .child(crate::i18n::search_continue_in_file_label(
                                self.ui_language,
                                resume.next_line,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.continue_workspace_search_in_file(resume_path.clone(), cx);
                            }))
                            .into_any_element(),
                    );
                    remaining_rows = remaining_rows.saturating_sub(1);
                }
            }
        }

        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::WorkspaceSearch,
            &self.workspace_search_scroll_handle,
            f32::from(self.workspace_search_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.self_drawn.scrollbar.right_margin_px),
            cx,
        );
        div()
            .key_context("SearchBar")
            .id("workspace-search-panel")
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(input)
            .children(filters)
            .child(status)
            .child(
                div()
                    .id("workspace-search-results-container")
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .id("workspace-search-results")
                            .h_full()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                this.reveal_panel_scrollbar(
                                    PanelScrollbarKind::WorkspaceSearch,
                                    cx,
                                );
                                this.maybe_extend_workspace_search_results(cx);
                            }))
                            .track_scroll(&self.workspace_search_scroll_handle)
                            .children(result_children),
                    )
                    .when_some(scrollbar, |this, scrollbar| this.child(scrollbar)),
            )
            .into_any_element()
    }

    fn workspace_search_match_excerpt(
        &self,
        line_text: &str,
        match_start: usize,
        match_end: usize,
        font_size_px: f32,
    ) -> (String, String, String) {
        let start = previous_char_boundary(line_text, match_start.min(line_text.len()));
        let end = next_char_boundary(line_text, match_end.min(line_text.len()));
        let matched = line_text[start..end].to_string();
        let available_width = (self.sidebar_layout_width()
            - self.scaled(
                28.0 + 4.0 + 2.0 * self.styles.workspace_search.ws_search_match_padding_x_px,
            ))
        .max(self.scaled(80.0));
        let approx_char_width = (self.scaled(font_size_px) * 0.58).max(1.0);
        let max_chars = (available_width / approx_char_width).floor().max(8.0) as usize;
        let match_chars = line_text[start..end].chars().count();
        if line_text.chars().count() <= max_chars || match_chars >= max_chars.saturating_sub(2) {
            return (
                line_text[..start].to_string(),
                matched,
                line_text[end..].to_string(),
            );
        }

        let context_chars = max_chars.saturating_sub(match_chars).saturating_sub(2);
        let before_chars = context_chars / 2;
        let after_chars = context_chars.saturating_sub(before_chars);
        let before_start = byte_index_before(line_text, start, before_chars);
        let after_end = byte_index_after(line_text, end, after_chars);
        let before_prefix = if before_start > 0 { "..." } else { "" };
        let after_suffix = if after_end < line_text.len() {
            "..."
        } else {
            ""
        };
        (
            format!("{}{}", before_prefix, &line_text[before_start..start]),
            matched,
            format!("{}{}", &line_text[end..after_end], after_suffix),
        )
    }
}

fn compare_workspace_search_paths(
    sort_mode: SidebarSortMode,
    left: &std::path::Path,
    right: &std::path::Path,
) -> std::cmp::Ordering {
    let file_name = |path: &std::path::Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    match sort_mode {
        SidebarSortMode::NameAsc => file_name(left).cmp(&file_name(right)),
        SidebarSortMode::NameDesc => file_name(right).cmp(&file_name(left)),
        SidebarSortMode::Type => left
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .cmp(
                &right
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase(),
            )
            .then_with(|| file_name(left).cmp(&file_name(right))),
        // Timestamp ordering comes from sort_search_results' cached worker
        // observations, with a deterministic fallback for missing dates.
        SidebarSortMode::CreatedDesc | SidebarSortMode::CreatedAsc => file_name(left)
            .cmp(&file_name(right))
            .then_with(|| left.cmp(right)),
    }
}

#[cfg(test)]
mod atomic_search_tests {
    use super::{begin_workspace_search_staging, compare_workspace_search_paths};
    use crate::editor::{SidebarSortMode, WorkspaceSearchResult};
    use std::cmp::Ordering;
    use std::path::PathBuf;

    #[test]
    fn starting_a_new_search_preserves_visible_results() {
        let mut visible = vec![WorkspaceSearchResult {
            path: PathBuf::from("old.md"),
            matches: Vec::new(),
            resume: None,
        }];
        let mut staging = vec![WorkspaceSearchResult {
            path: PathBuf::from("partial.md"),
            matches: Vec::new(),
            resume: None,
        }];

        begin_workspace_search_staging(&mut visible, &mut staging);

        assert_eq!(visible.len(), 1);
        assert!(staging.is_empty());
    }

    #[test]
    fn search_file_headers_follow_sidebar_name_and_type_sorting() {
        let alpha = PathBuf::from("z/alpha.rs");
        let beta = PathBuf::from("a/beta.md");
        assert_eq!(
            compare_workspace_search_paths(SidebarSortMode::NameAsc, &alpha, &beta),
            Ordering::Less
        );
        assert_eq!(
            compare_workspace_search_paths(SidebarSortMode::NameDesc, &alpha, &beta),
            Ordering::Greater
        );
        assert_eq!(
            compare_workspace_search_paths(SidebarSortMode::Type, &alpha, &beta),
            Ordering::Greater
        );
    }
}

#[cfg(test)]
mod status_tests {
    use super::{WorkspaceSearchStatus, workspace_search_status};

    #[test]
    fn search_statuses_are_mutually_exclusive() {
        assert_eq!(
            workspace_search_status(true, false, 0),
            WorkspaceSearchStatus::Empty
        );
        assert_eq!(
            workspace_search_status(false, true, 4),
            WorkspaceSearchStatus::Searching
        );
        assert_eq!(
            workspace_search_status(false, false, 0),
            WorkspaceSearchStatus::NoResults
        );
        assert_eq!(
            workspace_search_status(false, false, 4),
            WorkspaceSearchStatus::Summary
        );
    }
}

fn previous_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn byte_index_before(text: &str, end: usize, max_chars: usize) -> usize {
    text[..end]
        .char_indices()
        .rev()
        .nth(max_chars.saturating_sub(1))
        .map_or(0, |(idx, _)| idx)
}

fn byte_index_after(text: &str, start: usize, max_chars: usize) -> usize {
    if max_chars == 0 {
        return start;
    }
    text[start..]
        .char_indices()
        .nth(max_chars)
        .map_or(text.len(), |(idx, _)| start + idx)
}

struct WorkspaceSearchBatch {
    results: Vec<WorkspaceSearchResult>,
    pending_paths: Vec<std::path::PathBuf>,
}

/// Per-pass line budget for a single file; the rest is searched on demand so a
/// huge file cannot stall the whole workspace scan.
const WORKSPACE_SEARCH_FILE_LINE_LIMIT: usize = 10_000;

/// Byte offset just past the `limit`-th newline at or after `start`, or `None`
/// when the remaining text is within budget.
fn line_budget_end(content: &str, start: usize, limit: usize) -> Option<usize> {
    let mut seen = 0usize;
    for (index, byte) in content.as_bytes().iter().enumerate().skip(start) {
        if *byte == b'\n' {
            seen += 1;
            if seen == limit {
                let end = index + 1;
                return (end < content.len()).then_some(end);
            }
        }
    }
    None
}

/// Searches at most `WORKSPACE_SEARCH_FILE_LINE_LIMIT` lines starting at
/// `start_offset`, reporting where a follow-up pass should resume.
fn search_file_window(
    content: &str,
    start_offset: usize,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> (Vec<WorkspaceSearchMatch>, Option<WorkspaceSearchResume>) {
    let budget_end = line_budget_end(content, start_offset, WORKSPACE_SEARCH_FILE_LINE_LIMIT);
    let window_end = budget_end.unwrap_or(content.len());
    let window = &content[start_offset..window_end];
    let mut matches = Vec::new();

    for range in find_search_matches(window, query, case_sensitive, whole_word, use_regex) {
        let byte_offset = start_offset + range.start;
        let byte_end = start_offset + range.end;
        let line_start = content[..byte_offset]
            .rfind('\n')
            .map(|position| position + 1)
            .unwrap_or(0);
        let line_end = content[byte_offset..]
            .find('\n')
            .map(|position| byte_offset + position)
            .unwrap_or(content.len());
        let match_start_in_line = byte_offset - line_start;
        matches.push(WorkspaceSearchMatch {
            line_number: content[..byte_offset].matches('\n').count() + 1,
            line_text: content[line_start..line_end].to_string(),
            match_start: match_start_in_line,
            match_end: match_start_in_line + (byte_end - byte_offset),
            byte_offset,
            byte_end,
        });
    }

    let resume = budget_end.map(|offset| WorkspaceSearchResume {
        next_line: content[..offset].matches('\n').count() + 1,
        next_byte_offset: offset,
    });
    (matches, resume)
}

fn search_directory_batch(
    workspace_root: &std::path::Path,
    mut pending_paths: Vec<std::path::PathBuf>,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
    path_filter: &SearchPathFilter,
    row_budget: usize,
) -> std::io::Result<WorkspaceSearchBatch> {
    let mut results = Vec::new();
    let mut loaded_rows = 0usize;
    let mut scanned_paths = 0usize;
    while let Some(path) = pending_paths.pop() {
        crate::platform::fs_task::check_deadline()?;
        if scanned_paths >= 128 {
            pending_paths.push(path);
            break;
        }
        scanned_paths += 1;
        let observed = if path == workspace_root {
            std::fs::metadata(&path)
        } else {
            std::fs::symlink_metadata(&path)
        };
        let metadata = match observed {
            Ok(metadata) => metadata,
            Err(error)
                if path != workspace_root && error.kind() == std::io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(error) => {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!("Inspect {}: {error}", path.display()),
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        let is_dir = metadata.is_dir();
        if path != workspace_root
            && (should_skip_path(&path, is_dir)
                || if is_dir {
                    path_filter.excludes_dir(workspace_root, &path)
                } else {
                    !path_filter.allows_file(workspace_root, &path)
                })
        {
            continue;
        }
        if is_dir {
            let mut children = Vec::new();
            let entries = std::fs::read_dir(&path).map_err(|error| {
                std::io::Error::new(error.kind(), format!("List {}: {error}", path.display()))
            })?;
            for entry in entries {
                crate::platform::fs_task::check_deadline()?;
                children.push(
                    entry
                        .map_err(|error| {
                            std::io::Error::new(
                                error.kind(),
                                format!("List {}: {error}", path.display()),
                            )
                        })?
                        .path(),
                );
            }
            children.sort();
            pending_paths.extend(children.into_iter().rev());
        } else if metadata.is_file() {
            remember_search_date(&path, &metadata);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(std::io::Error::new(
                        error.kind(),
                        format!("Read {}: {error}", path.display()),
                    ));
                }
            };
            let file_kind = classify_file_kind_for_bytes(&path, &bytes);
            if file_kind.supports_text_search() {
                let content = String::from_utf8_lossy(&bytes).into_owned();
                let (matches, resume) =
                    search_file_window(&content, 0, query, case_sensitive, whole_word, use_regex);
                if matches.is_empty() && resume.is_none() {
                    continue;
                }

                loaded_rows += 1 + matches.len().min(50) + usize::from(resume.is_some());
                results.push(WorkspaceSearchResult {
                    path,
                    matches,
                    resume,
                });
                if loaded_rows >= row_budget {
                    break;
                }
            }
        }
        // A no-match query must yield too, not just a query with many rows.
        if scanned_paths >= 128 {
            break;
        }
    }
    crate::platform::fs_task::check_deadline()?;
    Ok(WorkspaceSearchBatch {
        results,
        pending_paths,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::{
        SearchPathFilter, search_directory_batch, take_pending_workspace_search_navigation,
    };
    use super::{WORKSPACE_SEARCH_FILE_LINE_LIMIT, search_file_window};

    #[test]
    fn a_single_file_is_scanned_in_line_limited_passes() {
        let mut content = String::new();
        for line in 0..(WORKSPACE_SEARCH_FILE_LINE_LIMIT + 5) {
            content.push_str(
                if line == 3 || line == WORKSPACE_SEARCH_FILE_LINE_LIMIT + 2 {
                    "needle\n"
                } else {
                    "filler\n"
                },
            );
        }

        let (matches, resume) = search_file_window(&content, 0, "needle", true, false, false);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_number, 4);
        let resume = resume.expect("oversized file keeps a resume cursor");
        assert_eq!(resume.next_line, WORKSPACE_SEARCH_FILE_LINE_LIMIT + 1);

        let (rest, done) = search_file_window(
            &content,
            resume.next_byte_offset,
            "needle",
            true,
            false,
            false,
        );
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].line_number, WORKSPACE_SEARCH_FILE_LINE_LIMIT + 3);
        assert!(done.is_none());
    }

    #[test]
    fn workspace_search_navigation_waits_for_its_target_tab() {
        let path = std::path::PathBuf::from("target.md");
        let search_match = super::WorkspaceSearchMatch {
            line_number: 3,
            line_text: "needle".to_string(),
            match_start: 0,
            match_end: 6,
            byte_offset: 12,
            byte_end: 18,
        };
        let mut pending = Some((7, path.clone(), search_match));

        assert!(take_pending_workspace_search_navigation(&mut pending, 6, Some(&path)).is_none());
        assert!(
            take_pending_workspace_search_navigation(
                &mut pending,
                7,
                Some(std::path::Path::new("other.md")),
            )
            .is_none()
        );
        assert!(pending.is_some());

        let (opened_path, opened_match) =
            take_pending_workspace_search_navigation(&mut pending, 7, Some(&path)).unwrap();
        assert_eq!(opened_path, path);
        assert_eq!(opened_match.byte_offset, 12);
        assert!(pending.is_none());
    }

    #[test]
    fn no_match_search_yields_after_a_bounded_number_of_paths() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..300 {
            fs::write(directory.path().join(format!("{index}.md")), "no match").unwrap();
        }
        let batch = search_directory_batch(
            directory.path(),
            vec![directory.path().to_path_buf()],
            "needle",
            true,
            false,
            false,
            &SearchPathFilter::default(),
            100,
        )
        .unwrap();
        assert!(batch.results.is_empty());
        assert!(!batch.pending_paths.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlink_cycles_do_not_extend_search() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "needle").unwrap();
        std::os::unix::fs::symlink(directory.path(), directory.path().join("cycle")).unwrap();
        let batch = search_directory_batch(
            directory.path(),
            vec![directory.path().to_path_buf()],
            "needle",
            true,
            false,
            false,
            &SearchPathFilter::default(),
            100,
        )
        .unwrap();
        assert_eq!(batch.results.len(), 1);
        assert!(batch.pending_paths.is_empty());
    }

    #[test]
    fn missing_workspace_is_an_error_not_a_successful_empty_search() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("unavailable");
        assert!(
            search_directory_batch(
                &root,
                vec![root.clone()],
                "needle",
                true,
                false,
                false,
                &SearchPathFilter::default(),
                100
            )
            .is_err()
        );
    }

    #[test]
    fn selected_root_is_not_subject_to_descendant_exclusions() {
        let directory = tempfile::tempdir().unwrap();
        for name in [".workspace", "target"] {
            let root = directory.path().join(name);
            fs::create_dir(&root).unwrap();
            fs::write(root.join("note.md"), "needle").unwrap();
            fs::write(root.join(".hidden.md"), "needle").unwrap();
            for excluded in [".hidden", "target", "node_modules"] {
                fs::create_dir(root.join(excluded)).unwrap();
                fs::write(root.join(excluded).join("ignored.md"), "needle").unwrap();
            }
            let batch = search_directory_batch(
                &root,
                vec![root.clone()],
                "needle",
                true,
                false,
                false,
                &SearchPathFilter::default(),
                100,
            )
            .unwrap();
            assert_eq!(batch.results.len(), 1);
            assert_eq!(batch.results[0].path, root.join("note.md"));
            assert!(batch.pending_paths.is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicitly_selected_symlink_root_is_searched() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        let alias = directory.path().join(".alias");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("note.md"), "needle").unwrap();
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let batch = search_directory_batch(
            &alias,
            vec![alias.clone()],
            "needle",
            true,
            false,
            false,
            &SearchPathFilter::default(),
            100,
        )
        .unwrap();
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].path, alias.join("note.md"));
    }

    #[test]
    fn workspace_search_batch_preserves_a_cursor_for_later_results() {
        let root = std::env::temp_dir().join(format!(
            "moeka-search-batch-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.md"), "needle").unwrap();
        fs::write(root.join("b.md"), "needle").unwrap();

        let first = search_directory_batch(
            &root,
            vec![root.clone()],
            "needle",
            true,
            false,
            false,
            &SearchPathFilter::default(),
            1,
        )
        .unwrap();
        assert_eq!(first.results.len(), 1);
        assert!(!first.pending_paths.is_empty());

        let second = search_directory_batch(
            &root,
            first.pending_paths,
            "needle",
            true,
            false,
            false,
            &SearchPathFilter::default(),
            100,
        )
        .unwrap();
        assert_eq!(second.results.len(), 1);
        assert!(second.pending_paths.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn path_filters_prune_excluded_directories_and_require_included_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for path in ["docs/a.md", "docs/drafts/b.md", "notes/c.md", "notes/d.txt"] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "needle").unwrap();
        }
        let filter = SearchPathFilter::new(r"\.md$", "^docs/drafts$").unwrap();
        let batch = search_directory_batch(
            root,
            vec![root.to_path_buf()],
            "needle",
            true,
            false,
            false,
            &filter,
            100,
        )
        .unwrap();
        let mut found = batch
            .results
            .iter()
            .map(|result| result.path.strip_prefix(root).unwrap().to_path_buf())
            .collect::<Vec<_>>();
        found.sort();
        assert_eq!(
            found,
            vec![
                std::path::PathBuf::from("docs/a.md"),
                std::path::PathBuf::from("notes/c.md")
            ]
        );

        // Index candidates skip the walk, so ancestors must be checked too.
        let candidate = search_directory_batch(
            root,
            vec![root.join("docs/drafts/b.md")],
            "needle",
            true,
            false,
            false,
            &filter,
            100,
        )
        .unwrap();
        assert!(candidate.results.is_empty());
    }

    #[test]
    fn invalid_path_filter_regex_is_reported() {
        assert!(SearchPathFilter::new("(", "").is_err());
        assert!(SearchPathFilter::new("", "[").is_err());
        assert!(SearchPathFilter::new("  ", "").is_ok());
    }
}
