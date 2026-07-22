use std::path::PathBuf;
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, Window, div, prelude::*, px, svg,
};

use super::search::{
    SEARCH_CASE_ICON_SVG_PATH, SEARCH_CLOSE_ICON_SVG_PATH, SEARCH_ICON_SVG_PATH,
    SEARCH_REGEX_ICON_SVG_PATH, SEARCH_WHOLE_WORD_ICON_SVG_PATH, find_search_matches,
};
use super::*;

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
    pub(crate) fn sort_workspace_search_results(&mut self) {
        let sort_mode = self.sidebar_sort_mode;
        self.workspace_search_results.sort_by(|left, right| {
            compare_workspace_search_paths(sort_mode, &left.path, &right.path)
        });
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

        // If the sidebar was already visible (e.g. file tree), switching to the
        // search panel is a panel-type switch: change width instantly with no
        // animation. Only animate when the sidebar is appearing from hidden.
        let was_sidebar_visible = self.sidebar_visible;
        self.sidebar_visible = true;
        if was_sidebar_visible {
            self.set_sidebar_width_immediately(self.sidebar_target_width());
        } else {
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
        self.workspace_search_blur_sub = None;
        self.workspace_search_input_focused = false;
        self.clear_workspace_search_preview_state();
        // If the sidebar remains visible (switching back to the file tree), this
        // is a panel-type switch: change width instantly. Only animate when the
        // sidebar is actually being hidden.
        if self.sidebar_visible {
            self.set_sidebar_width_immediately(self.sidebar_target_width());
        } else {
            self.sync_sidebar_width_animation(cx);
        }
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    pub(crate) fn clear_workspace_search_preview_state(&mut self) {
        self.workspace_search_selected_match = None;
        self.workspace_search_highlight_path = None;
        self.workspace_search_highlight_ranges.clear();
        self.workspace_search_highlight_current_index = None;
    }

    fn workspace_search_total_match_count(&self) -> usize {
        self.workspace_search_results
            .iter()
            .map(|result| result.matches.iter().take(50).count())
            .sum()
    }

    fn workspace_search_match_at(
        &self,
        target_index: usize,
    ) -> Option<(PathBuf, WorkspaceSearchMatch)> {
        let mut current_index = 0usize;
        for result in &self.workspace_search_results {
            for search_match in result.matches.iter().take(50) {
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
            for _ in result.matches.iter().take(50) {
                if current_index == target_index {
                    return Some(child_index);
                }
                current_index += 1;
                child_index += 1;
            }
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

        self.reveal_workspace_search_match(path, search_match, cx);
    }

    fn reveal_workspace_search_match(
        &mut self,
        path: PathBuf,
        search_match: WorkspaceSearchMatch,
        cx: &mut Context<Self>,
    ) {
        self.workspace_search_highlight_path = Some(path);
        self.workspace_search_highlight_ranges =
            vec![search_match.byte_offset..search_match.byte_end];
        self.workspace_search_highlight_current_index = Some(0);
        self.cursor.offset = search_match.byte_offset.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.center_cursor_in_view_immediately(cx);
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

    fn workspace_search_total_row_count(&self) -> usize {
        self.workspace_search_results
            .iter()
            .map(|result| 1 + result.matches.len().min(50))
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

    pub(crate) fn run_workspace_search(&mut self, cx: &mut Context<Self>) {
        let query = self.workspace_search_query.trim().to_string();
        self.clear_workspace_search_preview_state();
        if query.is_empty() {
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
        if !root.exists() {
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
            let seed = cx
                .background_executor()
                .spawn(async move {
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
                this.workspace_search_pending_paths = seed.unwrap_or_else(|| vec![fallback_root]);
                this.workspace_search_has_more = !this.workspace_search_pending_paths.is_empty();
                this.schedule_workspace_search_batch(cx);
                cx.notify();
            })
            .ok();
        });
        self.workspace_search_task = Some(task);
    }

    /// Build (or rebuild) the workspace search index in the background. A
    /// rebuild is triggered when there is no index for the current workspace
    /// or when enough files changed since the last snapshot.
    pub(crate) fn ensure_workspace_search_index(&mut self, cx: &mut Context<Self>) {
        const REBUILD_DIRTY_THRESHOLD: usize = 256;

        let root = self.sidebar_root.clone();
        if root.as_os_str().is_empty() || !root.exists() {
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
            let built = cx
                .background_executor()
                .spawn(async move { super::search_index::WorkspaceSearchIndex::build(&root).ok() })
                .await;
            this.update(cx, |this, _cx| {
                this.workspace_search_index_task = None;
                if this.workspace_search_index_epoch != epoch {
                    return;
                }
                if let Some(index) = built {
                    this.workspace_search_index = Some(std::sync::Arc::new(index));
                    for path in &dirty_snapshot {
                        this.workspace_search_index_dirty_paths.remove(path);
                    }
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
        let pending_paths = std::mem::take(&mut self.workspace_search_pending_paths);
        let epoch = self.workspace_search_epoch;
        let task = cx.spawn(async move |this, cx| {
            let batch = cx
                .background_executor()
                .spawn(async move {
                    search_directory_batch(
                        pending_paths,
                        &query,
                        case_sensitive,
                        whole_word,
                        use_regex,
                        super::list_viewport::SEARCH_LIST_POLICY.batch_size,
                    )
                })
                .await;

            this.update(cx, |this, cx| {
                this.workspace_search_task = None;
                if this.workspace_search_epoch != epoch {
                    return;
                }
                this.workspace_search_staging_results.extend(batch.results);
                this.workspace_search_pending_paths = batch.pending_paths;
                this.workspace_search_has_more = !this.workspace_search_pending_paths.is_empty();
                if this.workspace_search_has_more {
                    this.schedule_workspace_search_batch(cx);
                } else {
                    let mut results = std::mem::take(&mut this.workspace_search_staging_results);
                    results.sort_by(|left, right| {
                        compare_workspace_search_paths(
                            this.sidebar_sort_mode,
                            &left.path,
                            &right.path,
                        )
                    });
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

    pub(crate) fn workspace_search_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        let workspace_styles = &self.styles.workspace_search;
        styles.text_input_height_px = self.scaled(
            workspace_styles.ws_search_input_height_px
                - 2.0 * workspace_styles.ws_search_input_border_width_px,
        );
        styles.text_input_padding_left_px =
            self.scaled(workspace_styles.ws_search_input_left_inset_px);
        styles.text_input_padding_right_px =
            self.scaled(self.styles.text_input.text_input_padding_right_px);
        styles.text_input_font_size_px =
            self.scaled(self.styles.text_input.text_input_font_size_px);
        styles.text_input_bg = gpui::transparent_black();
        styles.text_input_focus_bg = gpui::transparent_black();
        styles.text_input_text_color = self.styles.sidebar.sidebar_item_text_color;
        styles.text_input_placeholder_color = self.styles.sidebar.sidebar_empty_text_color;
        styles.text_input_radius_px = self.scaled(workspace_styles.ws_search_input_radius_px);
        styles.text_input_border_width_px = 0.0;
        styles.text_input_cursor_color = self.styles.sidebar.sidebar_item_selected_text_color;
        styles.text_input_cursor_width_px =
            self.scaled(self.styles.text_input.text_input_cursor_width_px);
        styles.text_input_selection_color = self.styles.sidebar.sidebar_item_selected_bg;
        styles.text_input_cursor_height_ratio =
            workspace_styles.ws_search_input_cursor_height_ratio;
        styles.text_input_cursor_height_adjust_px =
            workspace_styles.ws_search_input_cursor_height_adjust_px;
        styles
    }

    pub(crate) fn render_workspace_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let trimmed_query = self.workspace_search_query.trim();
        let workspace_styles = &self.styles.workspace_search;
        let input_height = self.scaled(workspace_styles.ws_search_input_height_px);
        let button_size = self.scaled(workspace_styles.ws_search_btn_size_px);
        let button_icon_size = self.scaled(workspace_styles.ws_search_btn_icon_size_px);
        let button_gap = self.scaled(workspace_styles.ws_search_input_icon_gap_px);
        let input_gap = self.scaled(workspace_styles.ws_search_input_gap_px);
        let icon_size = self.scaled(workspace_styles.ws_search_input_icon_size_px);
        let chrome_padding_x = ((input_height - button_size).max(0.0) / 2.0).round();
        let input_left_inset = self.scaled(workspace_styles.ws_search_input_left_inset_px);
        let input_left = (input_left_inset - icon_size).max(0.0) / 2.0;
        let icon_top = (input_height - icon_size).max(0.0) / 2.0;
        let button_radius = self.scaled(4.0);
        let button_hover_bg = workspace_styles.ws_search_btn_hover_bg;
        let button_active_bg = workspace_styles.ws_search_btn_active_bg;
        let button_icon_color = workspace_styles.ws_search_btn_icon_color;
        let button_active_icon_color = workspace_styles.ws_search_btn_active_icon_color;
        let input_icon_color = workspace_styles.ws_search_input_icon_color;
        let search_border_width = self.scaled(workspace_styles.ws_search_input_border_width_px);
        let search_border_color = workspace_styles.ws_search_input_border_color;
        let search_bg = if self.workspace_search_input_focused {
            workspace_styles.ws_search_input_focus_bg
        } else {
            workspace_styles.ws_search_input_bg
        };
        let search_hover_bg = if self.workspace_search_input_focused {
            workspace_styles.ws_search_input_focus_bg
        } else {
            workspace_styles.ws_search_input_hover_bg
        };
        let result_selected_bg = self.styles.sidebar.sidebar_item_selected_bg;

        let input = div()
            .w_full()
            .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
            .pt(px(self.scaled(workspace_styles.ws_search_padding_px)))
            .pb(px(self.scaled(2.0)))
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
                                    div()
                                        .id("workspace-search-case-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .when(
                                            self.workspace_search_case_sensitive,
                                            |this: gpui::Stateful<gpui::Div>| {
                                                this.bg(button_active_bg)
                                            },
                                        )
                                        .hover(move |style| style.bg(button_hover_bg))
                                        .child(
                                            svg()
                                                .path(SEARCH_CASE_ICON_SVG_PATH)
                                                .size(px(button_icon_size))
                                                .text_color(
                                                    if self.workspace_search_case_sensitive {
                                                        button_active_icon_color
                                                    } else {
                                                        button_icon_color
                                                    },
                                                ),
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
                                    Some("⌘⌥C"),
                                    cx,
                                ),
                            )
                            .child(
                                self.with_search_tooltip(
                                    div()
                                        .id("workspace-search-word-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .when(
                                            self.workspace_search_whole_word,
                                            |this: gpui::Stateful<gpui::Div>| {
                                                this.bg(button_active_bg)
                                            },
                                        )
                                        .hover(move |style| style.bg(button_hover_bg))
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
                                    Some("⌘⌥W"),
                                    cx,
                                ),
                            )
                            .child(
                                self.with_search_tooltip(
                                    div()
                                        .id("workspace-search-regex-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .when(
                                            self.workspace_search_use_regex,
                                            |this: gpui::Stateful<gpui::Div>| {
                                                this.bg(button_active_bg)
                                            },
                                        )
                                        .hover(move |style| style.bg(button_hover_bg))
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
                                    Some("⌘⌥R"),
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
        let status_text = match workspace_search_status(
            trimmed_query.is_empty(),
            self.workspace_search_task.is_some(),
            file_count,
        ) {
            WorkspaceSearchStatus::Empty => String::new(),
            WorkspaceSearchStatus::Searching => self.t(crate::i18n::TextKey::Searching).to_string(),
            WorkspaceSearchStatus::NoResults => {
                self.t(crate::i18n::TextKey::SearchNoResults).to_string()
            }
            WorkspaceSearchStatus::Summary => {
                crate::i18n::workspace_search_summary(self.ui_language, total_matches, file_count)
            }
        };
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
            'files: for result in &self.workspace_search_results {
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

                result_children.push(
                    div()
                        .w_full()
                        .h(px(
                            self.scaled(workspace_styles.ws_search_file_header_height_px)
                        ))
                        .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
                        .flex()
                        .items_center()
                        .gap(px(self.scaled(4.0)))
                        .bg(workspace_styles.ws_search_file_header_bg)
                        .border_t(px(workspace_styles.ws_search_file_header_border_width_px))
                        .border_b(px(workspace_styles.ws_search_file_header_border_width_px))
                        .border_color(workspace_styles.ws_search_file_header_border_color)
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
                        .into_any_element(),
                );
                remaining_rows = remaining_rows.saturating_sub(1);
                if remaining_rows == 0 {
                    break;
                }

                let hover_bg = workspace_styles.ws_search_match_hover_bg;
                let hl_bg = workspace_styles.ws_search_match_highlight_bg;
                let hl_color = workspace_styles.ws_search_match_highlight_text_color;
                for search_match in result.matches.iter().take(50) {
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
                        div()
                            .id(gpui::SharedString::from(format!(
                                "ws-match-{}-{}-{}",
                                result.path.display(),
                                line_num,
                                start
                            )))
                            .w_full()
                            .h(px(self.scaled(workspace_styles.ws_search_match_height_px)))
                            .px(px(
                                self.scaled(workspace_styles.ws_search_match_padding_x_px)
                            ))
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(4.0)))
                            .cursor(CursorStyle::Arrow)
                            .when(is_selected, |this| this.bg(result_selected_bg))
                            .hover(move |style| style.bg(hover_bg))
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
                                    this.workspace_search_selected_match =
                                        Some(current_match_index);
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
            }
        }

        div()
            .key_context("SearchBar")
            .id("workspace-search-panel")
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(input)
            .child(status)
            .child(
                div()
                    .id("workspace-search-results")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        this.reveal_panel_scrollbar(PanelScrollbarKind::Sidebar, cx);
                        this.maybe_extend_workspace_search_results(cx);
                    }))
                    .track_scroll(&self.workspace_search_scroll_handle)
                    .children(result_children),
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
        SidebarSortMode::CreatedDesc | SidebarSortMode::CreatedAsc => {
            let created = |path: &std::path::Path| {
                std::fs::metadata(path)
                    .and_then(|meta| meta.created().or_else(|_| meta.modified()))
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            };
            let order = created(left).cmp(&created(right));
            let order = if sort_mode == SidebarSortMode::CreatedDesc {
                order.reverse()
            } else {
                order
            };
            order.then_with(|| file_name(left).cmp(&file_name(right)))
        }
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
        }];
        let mut staging = vec![WorkspaceSearchResult {
            path: PathBuf::from("partial.md"),
            matches: Vec::new(),
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

fn search_directory_batch(
    mut pending_paths: Vec<std::path::PathBuf>,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
    row_budget: usize,
) -> WorkspaceSearchBatch {
    let mut results = Vec::new();
    let mut loaded_rows = 0usize;
    while let Some(path) = pending_paths.pop() {
        let is_dir = path.is_dir();
        if should_skip_path(&path, is_dir) {
            continue;
        }
        if is_dir {
            if let Ok(entries) = std::fs::read_dir(&path) {
                let mut children = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .collect::<Vec<_>>();
                children.sort();
                pending_paths.extend(children.into_iter().rev());
            }
        } else if let Ok(bytes) = std::fs::read(&path) {
            let file_kind = classify_file_kind_for_bytes(&path, &bytes);
            if file_kind.supports_text_search() {
                let content = String::from_utf8_lossy(&bytes).into_owned();
                let match_ranges =
                    find_search_matches(&content, query, case_sensitive, whole_word, use_regex);
                if match_ranges.is_empty() {
                    continue;
                }

                let mut matches = Vec::new();
                for range in match_ranges {
                    let byte_offset = range.start;
                    let line_start = content[..byte_offset]
                        .rfind('\n')
                        .map(|position| position + 1)
                        .unwrap_or(0);
                    let line_end = content[byte_offset..]
                        .find('\n')
                        .map(|position| byte_offset + position)
                        .unwrap_or(content.len());
                    let line_number = content[..byte_offset].matches('\n').count() + 1;
                    let line_text = content[line_start..line_end].to_string();
                    let match_start_in_line = byte_offset - line_start;
                    let match_end_in_line = match_start_in_line + (range.end - range.start);

                    matches.push(WorkspaceSearchMatch {
                        line_number,
                        line_text,
                        match_start: match_start_in_line,
                        match_end: match_end_in_line,
                        byte_offset,
                        byte_end: range.end,
                    });
                }

                loaded_rows += 1 + matches.len().min(50);
                results.push(WorkspaceSearchResult { path, matches });
                if loaded_rows >= row_budget {
                    break;
                }
            }
        }
    }
    WorkspaceSearchBatch {
        results,
        pending_paths,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::search_directory_batch;

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

        let first = search_directory_batch(vec![root.clone()], "needle", true, false, false, 1);
        assert_eq!(first.results.len(), 1);
        assert!(!first.pending_paths.is_empty());

        let second = search_directory_batch(first.pending_paths, "needle", true, false, false, 100);
        assert_eq!(second.results.len(), 1);
        assert!(second.pending_paths.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
