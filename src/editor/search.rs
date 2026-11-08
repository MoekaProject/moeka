use std::ops::Range;

use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*, px, svg};

use super::*;

pub(crate) const SEARCH_ICON_SVG_PATH: &str = "assets/icons/filetree/search.svg";
pub(crate) const SEARCH_CASE_ICON_SVG_PATH: &str = "assets/icons/search/case_sensitive.svg";
pub(crate) const SEARCH_WHOLE_WORD_ICON_SVG_PATH: &str = "assets/icons/search/whole_word.svg";
pub(crate) const SEARCH_REGEX_ICON_SVG_PATH: &str = "assets/icons/search/regex.svg";
pub(crate) const SEARCH_FILTER_ICON_SVG_PATH: &str = "assets/icons/search/filter.svg";
pub(crate) const SEARCH_COLLAPSE_ALL_ICON_SVG_PATH: &str = "assets/icons/search/collapse-all.svg";
pub(crate) const SEARCH_EXPAND_ALL_ICON_SVG_PATH: &str = "assets/icons/search/expand-all.svg";
pub(crate) const SEARCH_CLOSE_ICON_SVG_PATH: &str = "assets/icons/filetree/close.svg";

/// Buffers at or below this size are scanned in one go; the streaming path only
/// pays off once a full scan stops fitting in a frame.
const SEARCH_SYNC_SCAN_LIMIT: usize = 256 * 1024;
/// Bytes consumed per slice before the scan yields back to the run loop.
const SEARCH_SCAN_SLICE_BYTES: usize = 512 * 1024;
/// How far a regex match is allowed to reach past the slice it started in.
const SEARCH_SCAN_REGEX_OVERLAP_BYTES: usize = 64 * 1024;

enum SearchScanStep {
    Continue(usize),
    Done,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct OccurrenceReveal {
    pub tab_id: Option<String>,
    pub range: Range<usize>,
}

fn current_occurrence_source_range(
    text: &str,
    tab_id: Option<&str>,
    cursor: Cursor,
    reveal: Option<&OccurrenceReveal>,
) -> Option<Range<usize>> {
    let reveal = reveal?;
    (reveal.tab_id.as_deref() == tab_id
        && !reveal.range.is_empty()
        && cursor.selection_range() == reveal.range
        && text.get(reveal.range.clone()).is_some())
    .then(|| reveal.range.clone())
}

fn current_search_source_range(
    text: &str,
    visible: bool,
    matches: &[Range<usize>],
    current: Option<usize>,
) -> Option<Range<usize>> {
    let range = matches.get(current?)?;
    (visible && range.start < range.end && text.get(range.clone()).is_some()).then(|| range.clone())
}

fn search_match_lines(buffer: &Buffer, range: &Range<usize>) -> Range<usize> {
    let start = buffer.offset_to_line_col(range.start).0;
    let end = buffer.offset_to_line_col(range.end.saturating_sub(1)).0 + 1;
    start..end
}

fn search_reveal_range_on_line(
    buffer: &Buffer,
    range: &Range<usize>,
    line: usize,
) -> Option<Range<usize>> {
    if line >= buffer.line_count() || !search_match_lines(buffer, range).contains(&line) {
        return None;
    }
    let start = buffer.line_start(line);
    let len = buffer.line_content(line).len();
    Some(range.start.saturating_sub(start).min(len)..range.end.saturating_sub(start).min(len))
}

fn search_reveals_code_fence_group(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    range: &Range<usize>,
    line: usize,
) -> bool {
    let is_code = |line: usize| {
        decorations.get(line).is_some_and(|decoration| {
            matches!(
                decoration.block_type,
                BlockType::CodeBlock | BlockType::CodeBlockFence
            )
        })
    };
    if !is_code(line) {
        return false;
    }
    let lines = search_match_lines(buffer, range);
    if lines.contains(&line) {
        return true;
    }
    // Walk only toward the match, stopping at the first non-code row. Avoid
    // scanning a huge fence to both ends for every visible source line.
    if line < lines.start {
        (line + 1..=lines.start).all(is_code)
    } else {
        (lines.end - 1..line).rev().all(is_code)
    }
}

fn search_replacements(
    text: &str,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> Result<Vec<(Range<usize>, String)>, regex::Error> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    if !use_regex {
        return Ok(
            find_search_matches(text, query, case_sensitive, whole_word, false)
                .into_iter()
                .map(|range| (range, replacement.to_string()))
                .collect(),
        );
    }
    let regex = regex::RegexBuilder::new(query)
        .case_insensitive(!case_sensitive)
        .build()?;
    Ok(regex
        .captures_iter(text)
        .filter_map(|captures| {
            let found = captures.get(0)?;
            if found.is_empty()
                || (whole_word
                    && (!is_word_boundary(text, found.start())
                        || !is_word_boundary(text, found.end())))
            {
                return None;
            }
            let mut expanded = String::new();
            captures.expand(replacement, &mut expanded);
            Some((found.range(), expanded))
        })
        .collect())
}

/// Rounds `offset` up to the next line start so slices never split a line.
/// Keeping slices line-aligned is also what makes whole-word matching correct at
/// a slice's leading edge: the byte before it is always a newline.
fn next_search_slice_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    // The requested offset is a raw byte count and can land inside a multi-byte
    // character, so walk forward to the next boundary before slicing.
    let mut offset = offset;
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    if offset >= text.len() {
        return text.len();
    }
    match text[offset..].find('\n') {
        Some(position) => offset + position + 1,
        None => text.len(),
    }
}

/// Matches that begin inside `[start, slice_end)`, reading up to `overlap` bytes
/// past the boundary so a match straddling it is still found whole.
#[allow(clippy::too_many_arguments)]
fn search_slice_matches(
    text: &str,
    start: usize,
    slice_end: usize,
    overlap: usize,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> Vec<Range<usize>> {
    let overlap_end = next_search_slice_boundary(text, slice_end.saturating_add(overlap));
    find_search_matches(
        &text[start..overlap_end],
        query,
        case_sensitive,
        whole_word,
        use_regex,
    )
    .into_iter()
    .map(|range| start + range.start..start + range.end)
    .take_while(|range| range.start < slice_end)
    .collect()
}

impl EditorView {
    /// Navigation can reveal source without granting permission to edit it.
    pub(super) fn current_reveal_source_range(&self) -> Option<Range<usize>> {
        current_occurrence_source_range(
            self.buffer.text(),
            self.active_tab_id.as_deref(),
            self.cursor,
            self.occurrence_reveal.as_ref(),
        )
        .or_else(|| {
            current_search_source_range(
                self.buffer.text(),
                self.search_visible,
                &self.search_matches,
                self.search_current_index,
            )
        })
    }

    pub(super) fn append_source_reveal_range(&self, line: usize, active: &mut Vec<Range<usize>>) {
        if let Some(range) = self.current_reveal_source_range()
            && let Some(local) = search_reveal_range_on_line(&self.buffer, &range, line)
        {
            active.push(local);
        }
    }

    pub(super) fn source_reveals_line(&self, line: usize) -> bool {
        self.current_reveal_source_range()
            .is_some_and(|range| search_match_lines(&self.buffer, &range).contains(&line))
    }

    pub(super) fn source_reveals_range(&self, source: &Range<usize>) -> bool {
        self.current_reveal_source_range()
            .is_some_and(|range| range.start < source.end && source.start < range.end)
    }

    pub(super) fn source_reveals_code_fence_group(&self, line: usize) -> bool {
        self.current_reveal_source_range().is_some_and(|range| {
            search_reveals_code_fence_group(&self.buffer, &self.decorations, &range, line)
        })
    }

    pub(super) fn cancel_search_scan(&mut self) {
        self.search_scan_epoch = self.search_scan_epoch.wrapping_add(1);
        self.search_scan_task = None;
        self.search_scan_complete = true;
    }

    pub(super) fn invalidate_search_presentation(&mut self) {
        self.clear_active_tab_layout_cache();
        self.visible_line_geometry.clear();
        self.table_cursor_paint_bounds = None;
        self.last_stable_input_bounds = None;
        self.pending_input_bounds = None;
    }

    pub(super) fn clear_occurrence_reveal(&mut self) {
        if self.occurrence_reveal.take().is_some() {
            self.invalidate_search_presentation();
        }
    }

    pub(super) fn prepare_source_range_for_navigation(&mut self, range: &Range<usize>) {
        let lines = search_match_lines(&self.buffer, range);
        self.ensure_lazy_markdown_decorations(lines.clone());
        self.unfold_search_match(lines);
        self.close_preview_overlay();
        self.reset_table_handles();
        self.is_selecting = false;
        self.selection_drag_has_moved = false;
        self.suppress_fold_for_empty_selection_drag = false;
        self.table_boundary_caret = None;
        self.table_boundary_drag_origin = None;
        self.pending_click_anchor = None;
        self.pending_typing_scroll_guard = None;
        self.pending_preview_reveal = None;
        self.invalidate_search_presentation();
    }

    fn search_can_replace(&self) -> bool {
        self.active_tab_supports_text_search()
            && self.active_tab_can_start_content_edit()
            && !self.active_tab_is_terminal()
    }

    fn focus_search_input(&self, replacement: bool, window: &mut Window, cx: &gpui::App) {
        let editor = if replacement {
            &self.search_replace_editor
        } else {
            &self.search_query_editor
        };
        if let Some(editor) = editor {
            editor.focus_handle(cx).focus(window);
        }
    }

    pub(super) fn switch_search_input(
        &mut self,
        _: &SwitchSearchInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query_focused = self
            .search_query_editor
            .as_ref()
            .is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
        self.focus_search_input(
            query_focused && self.search_replace_visible && self.search_can_replace(),
            window,
            cx,
        );
    }

    pub(crate) fn with_search_tooltip(
        &self,
        child: impl IntoElement,
        label_key: &'static str,
        shortcut: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        tooltip::with_tooltip(
            child,
            cx.entity().clone(),
            crate::i18n::search_option_label(self.ui_language, label_key),
            shortcut,
        )
    }

    pub(crate) fn on_find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab_supports_text_search() && !self.active_tab_is_terminal() {
            return;
        }
        self.open_search(false, window, cx);
    }

    pub(crate) fn on_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.search_navigate(true, cx);
    }

    pub(crate) fn on_find_previous(
        &mut self,
        _: &FindPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_navigate(false, cx);
    }

    pub(crate) fn on_dismiss_search(
        &mut self,
        _: &DismissSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_search(window, cx);
    }

    pub(crate) fn on_toggle_replace(
        &mut self,
        _: &ToggleReplace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_can_replace() {
            return;
        }
        if !self.search_visible {
            self.open_search(true, window, cx);
        } else {
            self.search_replace_visible = !self.search_replace_visible;
            self.focus_search_input(self.search_replace_visible, window, cx);
            cx.notify();
        }
    }

    pub(crate) fn on_replace_next(
        &mut self,
        _: &ReplaceNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search_visible && self.search_replace_visible {
            self.replace_current_match(cx);
        }
    }

    pub(crate) fn on_replace_all(
        &mut self,
        _: &ReplaceAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search_visible && self.search_replace_visible {
            self.replace_all_matches(cx);
        }
    }

    pub(crate) fn open_search(
        &mut self,
        with_replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_terminal = self.active_tab_is_terminal();
        if !self.active_tab_supports_text_search() && !is_terminal {
            return;
        }
        self.pending_typing_scroll_guard = None;
        let with_replace = with_replace && self.search_can_replace();
        let was_visible = self.search_visible;
        self.search_visible = true;
        self.search_replace_visible = with_replace;

        if self.search_query_editor.is_none() {
            let styles = self.search_query_text_input_styles();
            let initial_query = self.search_query.clone();
            let placeholder = self.t(crate::i18n::TextKey::SearchPlaceholder).to_string();
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
                            .search_query_editor
                            .as_ref()
                            .map(|entity| entity.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.search_query = query;
                        this.run_search(cx);
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        this.search_navigate(true, cx);
                    }
                    crate::text_input::TextInputEvent::SubmitBackward => {
                        this.search_navigate(false, cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    _ => {}
                },
            )
            .detach();
            self.search_query_editor = Some(editor);
        }

        if !was_visible {
            if let Some(tab_id) = self.active_terminal_tab_id() {
                if let Some(selected_text) = self
                    .chat_terminal_selection_text(&tab_id)
                    .filter(|text| !text.is_empty() && text.len() < 200 && !text.contains('\n'))
                {
                    self.search_query = selected_text;
                    self.run_search(cx);
                }
            } else {
                let selection = self.cursor.selection_range();
                let selection = self.buffer.clamp_offset(selection.start)
                    ..self.buffer.clamp_offset(selection.end);
                if !selection.is_empty() && selection.len() < 200 {
                    let selected_text = self.buffer.text_for_range(selection).to_string();
                    if !selected_text.contains('\n') {
                        self.search_query = selected_text;
                        self.run_search(cx);
                    }
                }
            }
        }

        if self.search_replace_editor.is_none() {
            let styles = self.search_replace_text_input_styles();
            let initial_replace = self.search_replace.clone();
            let placeholder = self.t(crate::i18n::TextKey::ReplacePlaceholder).to_string();
            let editor = cx.new(move |cx| {
                let mut input = crate::text_input::TextInput::new(styles, &placeholder, cx);
                if !initial_replace.is_empty() {
                    input.set_text(&initial_replace, cx);
                }
                input
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let replace = this
                            .search_replace_editor
                            .as_ref()
                            .map(|entity| entity.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.search_replace = replace;
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.replace_current_match(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    _ => {}
                },
            )
            .detach();
            self.search_replace_editor = Some(editor);
        }

        let query_styles = self.search_query_text_input_styles();
        let query = self.search_query.clone();
        if let Some(editor) = &self.search_query_editor {
            editor.update(cx, |input, cx| {
                input.styles = query_styles;
                input.set_text(&query, cx);
                input.select_all(cx);
            });
            editor.focus_handle(cx).focus(window);
        }

        let replace_styles = self.search_replace_text_input_styles();
        let replace = self.search_replace.clone();
        if let Some(editor) = &self.search_replace_editor {
            editor.update(cx, |input, cx| {
                input.styles = replace_styles;
                input.set_text(&replace, cx);
            });
        }

        self.focus_search_input(with_replace, window, cx);
        self.run_search(cx);

        cx.notify();
    }

    pub(super) fn clear_search_inputs(&mut self, cx: &mut Context<Self>) {
        self.clear_sidebar_filter_text(cx);
        for pane_id in self.pane_ids() {
            self.with_pane(pane_id, |this| {
                if !this.search_query.is_empty()
                    || this
                        .search_query_editor
                        .as_ref()
                        .is_some_and(|input| !input.read(cx).text().is_empty())
                {
                    this.search_query.clear();
                    if let Some(input) = &this.search_query_editor {
                        input.update(cx, |input, cx| input.set_text("", cx));
                    }
                    this.run_search(cx);
                }
            });
        }
        let workspace_changed = !self.workspace_search_query.is_empty()
            || !self.workspace_search_include.is_empty()
            || !self.workspace_search_exclude.is_empty();
        self.workspace_search_query.clear();
        self.workspace_search_include.clear();
        self.workspace_search_exclude.clear();
        for input in [
            &self.workspace_search_editor,
            &self.workspace_search_include_editor,
            &self.workspace_search_exclude_editor,
        ]
        .into_iter()
        .flatten()
        {
            if !input.read(cx).text().is_empty() {
                input.update(cx, |input, cx| input.set_text("", cx));
            }
        }
        if workspace_changed {
            self.run_workspace_search(cx);
        }
        if !self.quick_open_query.is_empty()
            || self
                .quick_open_editor
                .as_ref()
                .is_some_and(|input| !input.read(cx).text().is_empty())
        {
            self.quick_open_epoch = self.quick_open_epoch.wrapping_add(1);
            self.reset_quick_open_state(cx);
        }
        for input in [&self.command_palette_editor, &self.language_picker_editor]
            .into_iter()
            .flatten()
        {
            if !input.read(cx).text().is_empty() {
                input.update(cx, |input, cx| {
                    input.set_text("", cx);
                    cx.emit(crate::text_input::TextInputEvent::Changed);
                });
            }
        }
    }

    pub(crate) fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_visible = false;
        self.cancel_search_scan();
        self.search_matches.clear();
        self.search_current_index = None;
        self.cancel_outline_navigation();
        self.invalidate_search_presentation();
        if let Some(tab_id) = self.active_terminal_tab_id() {
            self.clear_terminal_search(&tab_id);
            self.focus_terminal_tab(&tab_id, window);
        } else {
            window.focus(&self.focus_handle(cx));
        }
        cx.notify();
    }

    pub(crate) fn run_search(&mut self, cx: &mut Context<Self>) {
        if let Some(tab_id) = self.active_terminal_tab_id() {
            self.run_terminal_search(&tab_id, cx);
            return;
        }

        // Any in-flight scan belongs to the previous query.
        self.cancel_search_scan();
        self.search_matches.clear();
        self.search_current_index = None;
        self.search_scan_buffer_len = self.buffer.len();
        self.cancel_outline_navigation();
        self.invalidate_search_presentation();

        if self.search_query.is_empty() {
            cx.notify();
            return;
        }

        if self.buffer.len() <= SEARCH_SYNC_SCAN_LIMIT {
            self.search_matches = find_search_matches(
                self.buffer.text(),
                &self.search_query,
                self.search_case_sensitive,
                self.search_whole_word,
                self.search_use_regex,
            );
            self.adopt_new_search_matches(0, cx);
            if self.search_current_index.is_none() && !self.search_matches.is_empty() {
                self.search_current_index = Some(0);
                self.reveal_current_search_match(cx);
            }
            cx.notify();
            return;
        }

        // Large buffers are scanned in slices so typing stays responsive; the
        // counter fills in as slices land.
        self.search_scan_complete = false;
        let epoch = self.search_scan_epoch;
        let pane_id = self.current_pane_id();
        let tab_id = self.active_tab_id.clone();
        self.search_scan_task = Some(cx.spawn(async move |this, cx| {
            let mut scan_start = 0usize;
            loop {
                let step = this
                    .update(cx, |this, cx| {
                        if this.search_scan_epoch != epoch
                            || this.current_pane_id() != pane_id
                            || this.active_tab_id != tab_id
                        {
                            return SearchScanStep::Cancelled;
                        }
                        this.scan_search_slice(scan_start, cx)
                    })
                    .unwrap_or(SearchScanStep::Cancelled);
                match step {
                    SearchScanStep::Continue(next) => scan_start = next,
                    SearchScanStep::Done | SearchScanStep::Cancelled => break,
                }
                // Hand the run loop a frame before taking the next slice.
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1))
                    .await;
            }
        }));
        cx.notify();
    }

    /// Scans one slice starting at `start`, appending whatever it finds.
    fn scan_search_slice(&mut self, start: usize, cx: &mut Context<Self>) -> SearchScanStep {
        if self.buffer.len() != self.search_scan_buffer_len {
            // The document changed underneath the scan; the offsets collected so
            // far no longer describe it.
            self.search_scan_complete = true;
            cx.notify();
            return SearchScanStep::Cancelled;
        }

        let text = self.buffer.text();
        if start >= text.len() {
            return self.finish_search_scan(cx);
        }

        let text_len = text.len();
        let slice_end = next_search_slice_boundary(text, start + SEARCH_SCAN_SLICE_BYTES);
        let found = search_slice_matches(
            text,
            start,
            slice_end,
            self.search_scan_overlap_bytes(),
            &self.search_query,
            self.search_case_sensitive,
            self.search_whole_word,
            self.search_use_regex,
        );

        let previous_len = self.search_matches.len();
        self.search_matches.extend(found);
        self.adopt_new_search_matches(previous_len, cx);

        if slice_end >= text_len {
            self.finish_search_scan(cx)
        } else {
            cx.notify();
            SearchScanStep::Continue(slice_end)
        }
    }

    fn finish_search_scan(&mut self, cx: &mut Context<Self>) -> SearchScanStep {
        self.search_scan_complete = true;
        if self.search_current_index.is_none() && !self.search_matches.is_empty() {
            self.search_current_index = Some(0);
            self.reveal_current_search_match(cx);
        }
        cx.notify();
        SearchScanStep::Done
    }

    /// Selects the first match at or after the caret as soon as it shows up, so
    /// navigation works before the whole document has been scanned.
    fn adopt_new_search_matches(&mut self, previous_len: usize, cx: &mut Context<Self>) {
        if self.search_current_index.is_some() {
            return;
        }
        let cursor_offset = self.cursor.offset;
        let Some(offset) = self.search_matches[previous_len..]
            .iter()
            .position(|range| range.start >= cursor_offset)
        else {
            return;
        };
        self.search_current_index = Some(previous_len + offset);
        self.reveal_current_search_match(cx);
    }

    fn reveal_current_search_match(&mut self, cx: &mut Context<Self>) {
        if self.pending_typing_scroll_guard.is_some() {
            return;
        }
        if self.marked_ranges().next().is_some() || self.ime_composition_snapshot.is_some() {
            // Delayed native callbacks belong to the old composition, not to
            // the destination. Reuse the pane-local native cancellation barrier.
            self.fold_ime_cancellation.arm();
            self.clear_marked_ranges();
            self.finish_ime_composition();
            self.schedule_fold_ime_cancellation(cx);
            if self.search_current_index.is_none() {
                self.run_search(cx);
                return;
            }
        }
        let Some(index) = self.search_current_index else {
            return;
        };
        let Some(range) = self.search_matches.get(index).cloned() else {
            return;
        };
        if range.is_empty() || self.buffer.text().get(range.clone()).is_none() {
            return;
        }
        self.prepare_source_range_for_navigation(&range);
        self.clear_extra_cursors();
        self.cursor.move_to(range.start, &self.buffer);
        self.reset_cursor_visual_targeting();
        self.center_cursor_in_view_once(cx);
    }

    /// Longest distance a match can reach past the slice it starts in.
    fn search_scan_overlap_bytes(&self) -> usize {
        if self.search_use_regex {
            // A pattern can match arbitrarily far; this covers realistic ones.
            SEARCH_SCAN_REGEX_OVERLAP_BYTES
        } else {
            self.search_query.len().saturating_sub(1)
        }
    }

    pub(crate) fn search_navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.pending_typing_scroll_guard = None;
        if let Some(tab_id) = self.active_terminal_tab_id() {
            self.terminal_search_navigate(&tab_id, forward, cx);
            return;
        }
        if !self.search_visible && !self.search_query.is_empty() {
            self.search_visible = true;
        }
        let reran_search = if self.search_matches.is_empty()
            && !self.search_query.is_empty()
            && self.search_scan_complete
        {
            self.run_search(cx);
            true
        } else {
            false
        };

        if self.search_matches.is_empty() {
            return;
        }

        let count = self.search_matches.len();
        let index = match (reran_search, self.search_current_index) {
            (true, Some(current)) => {
                if forward {
                    current
                } else {
                    (current + count - 1) % count
                }
            }
            (true, None) => {
                if forward {
                    0
                } else {
                    count - 1
                }
            }
            (false, Some(current)) => {
                if forward {
                    (current + 1) % count
                } else {
                    (current + count - 1) % count
                }
            }
            (false, None) => 0,
        };

        self.search_current_index = Some(index);
        self.reveal_current_search_match(cx);
        cx.notify();
    }

    pub(crate) fn replace_current_match(&mut self, cx: &mut Context<Self>) {
        if !self.active_tab_can_start_content_edit() || !self.active_tab_supports_text_search() {
            return;
        }
        let Ok(replacements) = search_replacements(
            self.buffer.text(),
            &self.search_query,
            &self.search_replace,
            self.search_case_sensitive,
            self.search_whole_word,
            self.search_use_regex,
        ) else {
            return;
        };
        let start = self
            .search_current_index
            .and_then(|index| self.search_matches.get(index))
            .map_or(self.cursor.offset, |range| range.start);
        let Some((range, replacement)) = replacements
            .iter()
            .find(|(range, _)| range.start >= start)
            .or_else(|| replacements.first())
        else {
            return;
        };
        if !self.begin_content_edit(cx) {
            return;
        }
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(range, replacement);
        self.buffer.replace(range.clone(), replacement);
        self.cursor
            .move_to(range.start + replacement.len(), &self.buffer);
        self.clear_extra_cursors();
        self.clear_marked_ranges();
        self.selected_handle = None;
        self.reset_cursor_visual_targeting();
        self.reparse();
        self.run_search(cx);
        self.sync_table_boundary_caret_after_reparse();
        if self.search_current_index.is_none() {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn replace_all_matches(&mut self, cx: &mut Context<Self>) {
        if !self.active_tab_can_start_content_edit() || !self.active_tab_supports_text_search() {
            return;
        }
        let Ok(replacements) = search_replacements(
            self.buffer.text(),
            &self.search_query,
            &self.search_replace,
            self.search_case_sensitive,
            self.search_whole_word,
            self.search_use_regex,
        ) else {
            return;
        };
        let Some((first, replacement)) = replacements.first() else {
            return;
        };
        if !self.begin_content_edit(cx) {
            return;
        }
        let cursor_offset = first.start + replacement.len();
        self.checkpoint();
        let old_len = self.buffer.len();
        let mut replaced_text = self.buffer.text().to_string();
        for (range, replacement) in replacements.iter().rev() {
            replaced_text.replace_range(range.clone(), replacement);
        }
        self.queue_non_markdown_line_display_transfer(&(0..old_len), &replaced_text);
        self.buffer.replace(0..old_len, &replaced_text);
        self.cursor.move_to(cursor_offset, &self.buffer);
        self.clear_extra_cursors();
        self.clear_marked_ranges();
        self.selected_handle = None;
        self.reset_cursor_visual_targeting();
        self.reparse();
        self.run_search(cx);
        if self.search_current_index.is_none() {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn search_query_text_input_styles(&self) -> crate::style::TextInputStyles {
        self.styles
            .input_styles(crate::style::InputRole::Search, self.ui_scale)
    }

    pub(crate) fn search_replace_text_input_styles(&self) -> crate::style::TextInputStyles {
        self.styles
            .input_styles(crate::style::InputRole::Replace, self.ui_scale)
    }

    pub(crate) fn render_search_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let search_styles = &self.styles.search;
        let can_replace = self.search_can_replace();
        let replace_visible = can_replace && self.search_replace_visible;
        let regex_error = if self.search_use_regex && !self.search_query.is_empty() {
            regex::RegexBuilder::new(&self.search_query)
                .case_insensitive(!self.search_case_sensitive)
                .build()
                .err()
                .map(|error| error.to_string())
        } else {
            None
        };
        let can_apply_replace = can_replace
            && regex_error.is_none()
            && !self.search_query.is_empty()
            && !self.search_matches.is_empty();

        let match_count = if let Some(tab_id) = self.active_terminal_tab_id() {
            self.terminal_search_match_count(&tab_id)
        } else {
            self.search_matches.len()
        };
        let current_idx = self.search_current_index;
        // A trailing ellipsis marks a total that is still climbing.
        let scanning = !self.search_scan_complete;
        let counter_text = if match_count > 0 {
            format!(
                "{} / {}{}",
                current_idx.map(|index| index + 1).unwrap_or(0),
                match_count,
                if scanning { "…" } else { "" }
            )
        } else if scanning {
            "0 / 0…".to_string()
        } else if !self.search_query.is_empty() {
            self.t(crate::i18n::TextKey::SearchNoResults).to_string()
        } else {
            String::new()
        };

        let input_height = self.scaled(search_styles.search_input_height_px);
        let button_icon_size = self.scaled(search_styles.search_option_btn_icon_size_px);
        let button_gap = self.scaled(search_styles.search_input_icon_gap_px);
        let icon_size = self.scaled(search_styles.search_input_icon_size_px);
        let input_left_inset = self.scaled(search_styles.search_input_left_inset_px);
        let input_left = (input_left_inset - icon_size).max(0.0) / 2.0;
        let icon_top = (input_height - icon_size).max(0.0) / 2.0;
        let fold_button_size = self.scaled(18.0);
        let fold_icon_size = self.scaled(10.0);
        let fold_button_left = (input_left_inset - fold_button_size).max(0.0) / 2.0;
        let fold_button_top = (input_height - fold_button_size).max(0.0) / 2.0;

        let button_icon_color = search_styles.search_option_btn_icon_color;
        let button_active_icon_color = search_styles.search_option_btn_active_icon_color;
        let nav_btn_icon_size = self.scaled(search_styles.search_nav_btn_icon_size_px);
        let nav_icon_color = search_styles.search_nav_btn_icon_color;
        let bar_width = self.scaled(search_styles.search_bar_width_px).min(
            (self.last_viewport_width
                - self.scaled(search_styles.search_bar_offset_right_px) * 2.0)
                .max(0.0),
        );

        let query_row = div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(self.scaled(search_styles.search_input_gap_px)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(self
                        .scaled(96.0)
                        .min((bar_width - self.scaled(16.0)).max(0.0))))
                    .h(px(input_height))
                    .when(regex_error.is_some(), |this| {
                        this.border_b(px(1.0))
                            .border_color(self.styles.ui.colors.danger)
                    })
                    .when_some(self.search_query_editor.clone(), |this, editor| {
                        this.child(editor)
                    })
                    .when(!can_replace, |this| {
                        this.child(
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
                                        .text_color(search_styles.search_input_icon_color),
                                ),
                        )
                    })
                    .when(can_replace, |this| {
                        this.child(tooltip::with_tooltip(
                            crate::ui::button::icon_button(
                                "search-toggle-replace",
                                self.styles
                                    .search_fold_button(fold_button_size, self.ui_scale),
                                Default::default(),
                                self.ui_scale,
                            )
                            .absolute()
                            .left(px(fold_button_left))
                            .top(px(fold_button_top))
                            .child(
                                svg()
                                    .path(if replace_visible {
                                        FOLD_EXPANDED_SVG_PATH
                                    } else {
                                        FOLD_SVG_PATH
                                    })
                                    .size(px(fold_icon_size))
                                    .text_color(if replace_visible {
                                        button_active_icon_color
                                    } else {
                                        button_icon_color
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.on_toggle_replace(&ToggleReplace, window, cx);
                                }),
                            ),
                            cx.entity().clone(),
                            self.t(crate::i18n::TextKey::Replace),
                            Some(crate::editor::shortcut::hint("secondary-alt-f")),
                        ))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(self.scaled(search_styles.search_counter_font_size_px)))
                    .text_color(search_styles.search_counter_text_color)
                    .min_w(px(
                        self.scaled(search_styles.search_input_counter_min_width_px)
                    ))
                    .max_w(px(self.scaled(72.0)))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(counter_text),
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
                                "search-case-btn",
                                self.styles.search_toggle(),
                                crate::style::ControlState {
                                    selected: self.search_case_sensitive,
                                    ..Default::default()
                                },
                                self.ui_scale,
                            )
                            .child(
                                svg()
                                    .path(SEARCH_CASE_ICON_SVG_PATH)
                                    .size(px(button_icon_size))
                                    .text_color(if self.search_case_sensitive {
                                        button_active_icon_color
                                    } else {
                                        button_icon_color
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.search_case_sensitive = !this.search_case_sensitive;
                                    this.run_search(cx);
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
                                "search-word-btn",
                                self.styles.search_toggle(),
                                crate::style::ControlState {
                                    selected: self.search_whole_word,
                                    ..Default::default()
                                },
                                self.ui_scale,
                            )
                            .child(
                                svg()
                                    .path(SEARCH_WHOLE_WORD_ICON_SVG_PATH)
                                    .size(px(button_icon_size))
                                    .text_color(if self.search_whole_word {
                                        button_active_icon_color
                                    } else {
                                        button_icon_color
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.search_whole_word = !this.search_whole_word;
                                    this.run_search(cx);
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
                                "search-regex-btn",
                                self.styles.search_toggle(),
                                crate::style::ControlState {
                                    selected: self.search_use_regex,
                                    ..Default::default()
                                },
                                self.ui_scale,
                            )
                            .child(
                                svg()
                                    .path(SEARCH_REGEX_ICON_SVG_PATH)
                                    .size(px(button_icon_size))
                                    .text_color(if self.search_use_regex {
                                        button_active_icon_color
                                    } else {
                                        button_icon_color
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.search_use_regex = !this.search_use_regex;
                                    this.run_search(cx);
                                }),
                            ),
                            "regex",
                            Some(crate::editor::shortcut::hint("secondary-alt-r")),
                            cx,
                        ),
                    ),
            )
            .child(
                self.with_search_tooltip(
                    crate::ui::button::icon_button(
                        "search-prev-btn",
                        self.styles.search_navigation(false),
                        crate::style::ControlState {
                            disabled: match_count == 0,
                            ..Default::default()
                        },
                        self.ui_scale,
                    )
                    .child(
                        svg()
                            .path("assets/icons/navigation/arrow-up.svg")
                            .size(px(nav_btn_icon_size))
                            .text_color(nav_icon_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.search_navigate(false, cx);
                        }),
                    ),
                    "previous",
                    Some(crate::editor::shortcut::hint("secondary-shift-g")),
                    cx,
                ),
            )
            .child(
                self.with_search_tooltip(
                    crate::ui::button::icon_button(
                        "search-next-btn",
                        self.styles.search_navigation(false),
                        crate::style::ControlState {
                            disabled: match_count == 0,
                            ..Default::default()
                        },
                        self.ui_scale,
                    )
                    .child(
                        svg()
                            .path("assets/icons/navigation/arrow-down.svg")
                            .size(px(nav_btn_icon_size))
                            .text_color(nav_icon_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.search_navigate(true, cx);
                        }),
                    ),
                    "next",
                    Some(crate::editor::shortcut::hint("secondary-g")),
                    cx,
                ),
            )
            .child(
                self.with_search_tooltip(
                    crate::ui::button::icon_button(
                        "search-close-btn",
                        self.styles.search_navigation(true),
                        Default::default(),
                        self.ui_scale,
                    )
                    .child(
                        svg()
                            .path(SEARCH_CLOSE_ICON_SVG_PATH)
                            .size(px(button_icon_size))
                            .text_color(search_styles.search_close_btn_icon_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.dismiss_search(window, cx);
                        }),
                    ),
                    "close",
                    None,
                    cx,
                ),
            );

        let mut container =
            crate::ui::surface::surface(&self.styles.search_surface(), self.ui_scale)
                .id("search-bar")
                .key_context("SearchBar EditorSearchBar")
                .on_action(cx.listener(Self::switch_search_input))
                .occlude()
                .absolute()
                .top(px(self.scaled(search_styles.search_bar_offset_top_px)))
                .right(px(self.scaled(search_styles.search_bar_offset_right_px)))
                .w(px(bar_width))
                .max_w_full()
                .p(px(self.scaled(search_styles.search_bar_padding_px)))
                .flex()
                .flex_col()
                .gap(px(self.scaled(search_styles.search_bar_gap_px)))
                .child(query_row);

        if replace_visible {
            let replace_btn_color = search_styles.search_replace_btn_text_color;
            let replace_icon_size = self.scaled(self.styles.search_replace_button().icon_size_px);

            let replace_button = |all: bool, cx: &mut Context<Self>| {
                tooltip::with_tooltip(
                    crate::ui::button::icon_button(
                        if all {
                            "search-replace-all-btn"
                        } else {
                            "search-replace-btn"
                        },
                        self.styles.search_replace_button(),
                        crate::style::ControlState {
                            disabled: !can_apply_replace,
                            ..Default::default()
                        },
                        self.ui_scale,
                    )
                    .child(
                        svg()
                            .path(if all {
                                "assets/icons/search/replace-all.svg"
                            } else {
                                "assets/icons/search/replace.svg"
                            })
                            .size(px(replace_icon_size))
                            .text_color(replace_btn_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            if can_apply_replace {
                                if all {
                                    this.replace_all_matches(cx);
                                } else {
                                    this.replace_current_match(cx);
                                }
                            }
                        }),
                    ),
                    cx.entity().clone(),
                    self.t(if all {
                        crate::i18n::TextKey::ReplaceAll
                    } else {
                        crate::i18n::TextKey::Replace
                    }),
                    Some(crate::editor::shortcut::hint(if all {
                        "secondary-alt-enter"
                    } else {
                        "alt-enter"
                    })),
                )
            };

            container = container.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(search_styles.search_bar_gap_px)))
                    .w_full()
                    .when_some(self.search_replace_editor.clone(), |this, editor| {
                        this.child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w(px(0.0))
                                .h(px(input_height))
                                .child(editor)
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(input_left))
                                        .top(px(icon_top))
                                        .size(px(icon_size))
                                        .child(
                                            svg()
                                                .path("assets/icons/search/replace.svg")
                                                .size(px(self.scaled(10.0)))
                                                .text_color(button_icon_color),
                                        ),
                                ),
                        )
                    })
                    .child(replace_button(false, cx))
                    .child(replace_button(true, cx)),
            );
        }

        if let Some(error) = regex_error {
            container = container.child(tooltip::with_tooltip(
                div()
                    .w_full()
                    .px(px(
                        self.scaled(self.styles.ui.space(crate::style::SpaceStep::One))
                    ))
                    .text_size(px(self.scaled(search_styles.search_counter_font_size_px)))
                    .text_color(self.styles.ui.colors.danger)
                    .whitespace_normal()
                    .child(crate::i18n::search_option_label(
                        self.ui_language,
                        "invalid_regex",
                    )),
                cx.entity().clone(),
                error,
                None::<SharedString>,
            ));
        }

        container.into_any_element()
    }
}

pub(crate) fn find_search_matches(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();

    if use_regex {
        let regex = if case_sensitive {
            regex::Regex::new(query)
        } else {
            regex::RegexBuilder::new(query)
                .case_insensitive(true)
                .build()
        };
        if let Ok(regex) = regex {
            for found in regex.find_iter(text) {
                if found.start() != found.end()
                    && (!whole_word
                        || (is_word_boundary(text, found.start())
                            && is_word_boundary(text, found.end())))
                {
                    matches.push(found.start()..found.end());
                }
            }
        }
        return matches;
    }

    // Only a fold that can change byte lengths needs a map back to the original
    // offsets; the other two cases stay 1:1 and skip an allocation per byte.
    let (haystack, byte_map): (std::borrow::Cow<'_, str>, Option<Vec<usize>>) = if case_sensitive {
        (std::borrow::Cow::Borrowed(text), None)
    } else if text.is_ascii() && query.is_ascii() {
        (std::borrow::Cow::Owned(text.to_ascii_lowercase()), None)
    } else {
        let (folded, map) = fold_with_original_byte_map(text);
        (std::borrow::Cow::Owned(folded), Some(map))
    };
    let needle = if case_sensitive {
        std::borrow::Cow::Borrowed(query)
    } else if query.is_ascii() {
        std::borrow::Cow::Owned(query.to_ascii_lowercase())
    } else {
        std::borrow::Cow::Owned(query.to_lowercase())
    };
    let needle = needle.as_ref();

    let mut start = 0;
    while let Some(position) = haystack[start..].find(needle) {
        let folded_match_start = start + position;
        let folded_match_end = folded_match_start + needle.len();
        let (match_start, match_end) = if let Some(byte_map) = &byte_map {
            (byte_map[folded_match_start], byte_map[folded_match_end])
        } else {
            (folded_match_start, folded_match_end)
        };

        if whole_word
            && (!is_word_boundary(text, match_start) || !is_word_boundary(text, match_end))
        {
            start = next_char_boundary(&haystack, folded_match_start);
            continue;
        }

        matches.push(match_start..match_end);
        start = folded_match_end;
    }

    matches
}

fn fold_with_original_byte_map(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::new();
    let mut byte_map = Vec::with_capacity(text.len() + 1);
    byte_map.push(0);

    for (original_start, ch) in text.char_indices() {
        let original_end = original_start + ch.len_utf8();
        for folded_ch in ch.to_lowercase() {
            let folded_len = folded_ch.len_utf8();
            folded.push(folded_ch);
            for _ in 1..folded_len {
                byte_map.push(original_start);
            }
            byte_map.push(original_end);
        }
    }

    (folded, byte_map)
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let next = index + text[index..].chars().next().map_or(1, char::len_utf8);
    next.min(text.len())
}

fn is_word_boundary(text: &str, index: usize) -> bool {
    if index == 0 || index >= text.len() {
        return true;
    }

    let before = text[..index].chars().next_back();
    let after = text[index..].chars().next();
    !before.is_some_and(is_word_char) || !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::{find_search_matches, next_search_slice_boundary, search_slice_matches};
    use crate::{buffer::Buffer, markdown};

    #[test]
    fn occurrence_reveal_tracks_active_selection_and_tab_for_generated_source() {
        let text = "before\n```mermaid\ngraph TD\nA-->B\n```\nafter";
        let buffer = Buffer::from_text(text);
        let decorations = markdown::parse_decorations(text);
        let start = text.find("graph").unwrap();
        let range = start..start + "graph".len();
        let reveal = super::OccurrenceReveal {
            tab_id: Some("document".into()),
            range: range.clone(),
        };
        let cursor = crate::cursor::Cursor {
            anchor: range.start,
            offset: range.end,
        };
        let active =
            super::current_occurrence_source_range(text, Some("document"), cursor, Some(&reveal));
        assert_eq!(active, Some(range.clone()));
        assert_eq!(super::search_match_lines(&buffer, &range), 2..3);
        for line in 1..=4 {
            assert!(super::search_reveals_code_fence_group(
                &buffer,
                &decorations,
                &range,
                line
            ));
        }
        assert_eq!(
            super::current_occurrence_source_range(text, Some("other"), cursor, Some(&reveal)),
            None
        );
        assert_eq!(
            super::current_occurrence_source_range(
                text,
                Some("document"),
                crate::cursor::Cursor {
                    offset: range.end,
                    anchor: range.end
                },
                Some(&reveal)
            ),
            None
        );
        assert_eq!(
            super::current_occurrence_source_range(
                "short",
                Some("document"),
                cursor,
                Some(&reveal)
            ),
            None
        );
    }

    #[test]
    fn search_source_range_tracks_navigation_dismiss_and_invalid_utf8_or_stale_ranges() {
        let text = "前🙂 needle 后 needle";
        let matches = find_search_matches(text, "needle", true, false, false);
        for (index, expected) in matches.iter().enumerate() {
            assert_eq!(
                super::current_search_source_range(text, true, &matches, Some(index)),
                Some(expected.clone())
            );
        }
        assert_eq!(
            super::current_search_source_range(text, false, &matches, Some(0)),
            None
        );
        assert_eq!(
            super::current_search_source_range(text, true, &matches, None),
            None
        );
        assert_eq!(
            super::current_search_source_range(text, true, &matches, Some(matches.len())),
            None
        );
        for range in [0..0, 1..3, 0..text.len() + 1] {
            assert_eq!(
                super::current_search_source_range(text, true, &[range], Some(0)),
                None
            );
        }
    }

    #[test]
    fn search_source_projection_preserves_multiline_utf8_and_half_open_boundaries() {
        let text = "before\n前🙂 needle\n\nneedle 后\nafter";
        let buffer = Buffer::from_text(text);
        let start = text.find("前").unwrap();
        let end = text.find(" 后").unwrap();
        let range = start..end;
        assert_eq!(super::search_match_lines(&buffer, &range), 1..4);
        assert_eq!(super::search_reveal_range_on_line(&buffer, &range, 0), None);
        assert_eq!(
            super::search_reveal_range_on_line(&buffer, &range, 1),
            Some(0.."前🙂 needle".len())
        );
        assert_eq!(
            super::search_reveal_range_on_line(&buffer, &range, 2),
            Some(0..0)
        );
        assert_eq!(
            super::search_reveal_range_on_line(&buffer, &range, 3),
            Some(0..6)
        );
        assert_eq!(super::search_reveal_range_on_line(&buffer, &range, 4), None);
        let ends_at_next_line = start..buffer.line_start(3);
        assert_eq!(
            super::search_reveal_range_on_line(&buffer, &ends_at_next_line, 3),
            None
        );
    }

    #[test]
    fn search_source_code_group_reveals_fences_and_exact_keyword_maps() {
        for source in [
            "before\n\n```mermaid\ngraph TD\nA[秘密🙂] --> B\n```\n\nafter",
            "before\n\n$$\nx + needle\n$$\n\nafter",
            "before\n\n```math\nx + needle\n```\n\nafter",
            "before\n\n\\[\nx + needle\n\\]\n\nafter",
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = markdown::parse_decorations(source);
            let keyword = if source.contains("秘密🙂") {
                "秘密🙂"
            } else {
                "needle"
            };
            let range = find_search_matches(source, keyword, true, false, false).remove(0);
            let line = buffer.offset_to_line_col(range.start).0;
            let decoration = &decorations[line];
            let local = super::search_reveal_range_on_line(&buffer, &range, line).unwrap();
            let group_active =
                super::search_reveals_code_fence_group(&buffer, &decorations, &range, line);
            let (display, d2b, b2d, _) = super::helpers::build_display_text(
                buffer.line_content(line),
                decoration,
                &[local.clone()],
                group_active,
                None,
                0,
                3,
                3,
                2,
                0,
                0.7,
                0.7,
                0.7,
            );
            assert_eq!(
                &display[b2d[local.start]..b2d[local.end]],
                keyword,
                "{source}"
            );
            for (offset, _) in keyword.char_indices() {
                assert_eq!(d2b[b2d[local.start + offset]], local.start + offset);
            }
            if decoration.code_language.is_some() || source.contains("```") {
                let first = decorations
                    .iter()
                    .position(|d| d.block_type == markdown::BlockType::CodeBlockFence)
                    .unwrap();
                assert!(super::search_reveals_code_fence_group(
                    &buffer,
                    &decorations,
                    &range,
                    first
                ));
                assert!(!super::search_reveals_code_fence_group(
                    &buffer,
                    &decorations,
                    &range,
                    0
                ));
            }
        }
    }

    #[test]
    fn search_source_fence_info_and_closer_match_reveal_the_entire_group() {
        let source = "```mermaid\ngraph TD\nA-->B\n```\n\n```rust\nlet x = 1;\n```";
        let buffer = Buffer::from_text(source);
        let decorations = markdown::parse_decorations(source);
        for query in ["mermaid", "A-->B"] {
            let range = find_search_matches(source, query, true, false, false).remove(0);
            for line in 0..4 {
                assert!(super::search_reveals_code_fence_group(
                    &buffer,
                    &decorations,
                    &range,
                    line
                ));
            }
            for line in 5..8 {
                assert!(!super::search_reveals_code_fence_group(
                    &buffer,
                    &decorations,
                    &range,
                    line
                ));
            }
        }
        let closer = buffer.line_start(3)..buffer.line_end(3);
        assert!(super::search_reveals_code_fence_group(
            &buffer,
            &decorations,
            &closer,
            0
        ));
    }

    #[test]
    fn replacement_uses_full_regex_context_and_captures() {
        let text = "foo=12\nFOO=345";
        let edits = super::search_replacements(
            text,
            r"(?m)^(?P<key>foo)=(\d+)$",
            "${key}:$2:$$",
            false,
            false,
            true,
        )
        .unwrap();
        assert_eq!(
            edits,
            vec![(0..6, "foo:12:$".into()), (7..14, "FOO:345:$".into())]
        );
    }

    #[test]
    fn replacement_preserves_literal_dollars_and_utf8_ranges() {
        let text = "中文 foo FOO";
        let edits = super::search_replacements(text, "foo", "$1中文", false, false, false).unwrap();
        let mut buffer = crate::buffer::Buffer::from_text(text);
        for (range, replacement) in edits.iter().rev() {
            buffer.replace(range.clone(), replacement);
        }
        assert_eq!(buffer.text(), "中文 $1中文 $1中文");
    }

    #[test]
    fn replacement_can_delete_and_ignores_invalid_or_empty_queries() {
        assert_eq!(
            super::search_replacements("foo", "foo", "", true, false, false).unwrap(),
            vec![(0..3, String::new())]
        );
        assert!(super::search_replacements("foo", "[", "x", true, false, true).is_err());
        assert!(
            super::search_replacements("foo", "", "x", true, false, false)
                .unwrap()
                .is_empty()
        );
        assert!(
            super::search_replacements("foo", "^", "x", true, false, true)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn replacement_and_search_share_whole_word_regex_ranges() {
        let text = "food foo FOO";
        let edits = super::search_replacements(text, "foo", "bar", false, true, true).unwrap();
        assert_eq!(
            edits
                .iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            find_search_matches(text, "foo", false, true, true)
        );
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn replacement_all_is_one_undoable_transaction() {
        use crate::editor::history_store::{SnapshotViewState, TabHistory};
        let text = "中文 foo\nFOO foo";
        let edits = super::search_replacements(text, "foo", "替换", false, false, false).unwrap();
        let mut buffer = crate::buffer::Buffer::from_text(text);
        buffer.begin_edit_recording();
        for (range, replacement) in edits.iter().rev() {
            buffer.replace(range.clone(), replacement);
        }
        let replaced = buffer.text().to_string();
        assert_eq!(replaced, "中文 替换\n替换 替换");
        let mut history = TabHistory::default();
        history.push_undo_transaction(
            buffer.finish_edit_recording(),
            SnapshotViewState::new(0, 0, 0.0, Vec::new()),
            SnapshotViewState::new(0, 0, 0.0, Vec::new()),
            false,
        );
        assert_eq!(history.undo_stack.len(), 1);
        let (undo, _) = history.take_undo_transaction().unwrap();
        assert!(buffer.apply_edits_inverse(&undo));
        assert_eq!(buffer.text(), text);
        let (redo, _) = history.take_redo_transaction().unwrap();
        assert!(buffer.apply_edits_forward(&redo));
        assert_eq!(buffer.text(), replaced);
    }

    #[test]
    fn replacement_all_does_not_reprocess_inserted_matches() {
        let text = "foo foo";
        let edits = super::search_replacements(text, "foo", "foofoo", true, false, false).unwrap();
        let mut buffer = crate::buffer::Buffer::from_text(text);
        for (range, replacement) in edits.iter().rev() {
            buffer.replace(range.clone(), replacement);
        }
        assert_eq!(buffer.text(), "foofoo foofoo");
    }

    /// Walks `text` the way the streaming scan does, so the result has to match
    /// a single full-document scan.
    fn streamed_matches(
        text: &str,
        query: &str,
        slice_bytes: usize,
        case_sensitive: bool,
        whole_word: bool,
        use_regex: bool,
    ) -> Vec<std::ops::Range<usize>> {
        let overlap = if use_regex {
            64
        } else {
            query.len().saturating_sub(1)
        };
        let mut collected = Vec::new();
        let mut start = 0usize;
        while start < text.len() {
            let slice_end = next_search_slice_boundary(text, start + slice_bytes);
            collected.extend(search_slice_matches(
                text,
                start,
                slice_end,
                overlap,
                query,
                case_sensitive,
                whole_word,
                use_regex,
            ));
            start = slice_end;
        }
        collected
    }

    #[test]
    fn case_insensitive_whole_word_search_keeps_original_utf8_boundaries() {
        let text = "abc选FOO xyz 选 FOO";

        let matches = find_search_matches(text, "foo", false, true, false);

        assert_eq!(matches, vec![18..21]);
        assert_eq!(&text[matches[0].clone()], "FOO");
    }

    #[test]
    fn ascii_and_unicode_folding_agree_on_match_offsets() {
        // The ASCII fast path skips the byte map, so it has to report the same
        // offsets as the general fold that a non-ASCII character forces.
        let expected = vec![0..3, 4..7, 8..11];
        assert_eq!(
            find_search_matches("Foo foo FOO", "foo", false, false, false),
            expected
        );
        assert_eq!(
            find_search_matches("Foo foo FOO 选", "foo", false, false, false),
            expected
        );

        // The Kelvin sign is three bytes and folds to a one-byte `k`, which is
        // exactly what the byte map has to translate back.
        let kelvin = "\u{212A} foo";
        let matches = find_search_matches(kelvin, "k", false, false, false);
        assert_eq!(matches, vec![0..3]);
        assert_eq!(&kelvin[matches[0].clone()], "\u{212A}");
    }

    #[test]
    fn slice_boundaries_land_on_line_starts() {
        let text = "one\ntwo\nthree";
        assert_eq!(next_search_slice_boundary(text, 0), 4);
        assert_eq!(next_search_slice_boundary(text, 4), 8);
        // Past the last newline the remainder is one final slice.
        assert_eq!(next_search_slice_boundary(text, 9), text.len());
        assert_eq!(next_search_slice_boundary(text, 999), text.len());
    }

    #[test]
    fn streaming_a_document_finds_exactly_what_one_pass_finds() {
        let mut text = String::new();
        for line in 0..400 {
            text.push_str(&format!("line {line} needle and more words here\n"));
        }
        // A match sitting right where slices get cut must not be lost or doubled.
        for slice_bytes in [1, 7, 32, 64, 100, 4096] {
            assert_eq!(
                streamed_matches(&text, "needle", slice_bytes, true, false, false),
                find_search_matches(&text, "needle", false, false, false),
                "slice_bytes={slice_bytes}"
            );
        }
    }

    #[test]
    fn streaming_preserves_case_insensitive_and_whole_word_results() {
        let text = "Needle\nneedles NEEDLE\n选 needle 选\nNEEDLES\n".repeat(40);

        for slice_bytes in [1, 9, 40, 256] {
            assert_eq!(
                streamed_matches(&text, "needle", slice_bytes, false, true, false),
                find_search_matches(&text, "needle", false, true, false),
                "slice_bytes={slice_bytes}"
            );
        }
    }

    #[test]
    fn streaming_preserves_regex_results() {
        let text = "alpha 12 beta\ngamma 345 delta\n".repeat(60);

        for slice_bytes in [1, 13, 64, 512] {
            assert_eq!(
                streamed_matches(&text, r"\d+", slice_bytes, true, false, true),
                find_search_matches(&text, r"\d+", true, false, true),
                "slice_bytes={slice_bytes}"
            );
        }
    }
}
