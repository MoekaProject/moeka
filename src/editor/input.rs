use std::ops::Range;

use gpui::{
    App, Bounds, Context, EntityInputHandler, Focusable, FontFallbacks, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, SharedString,
    TextRun, UTF16Selection, Window, WrappedLine, point, px, size,
};

use crate::markdown::{BlockType, InlineKind};

use super::*;

struct TableCellDisplayInfo {
    wrapped: WrappedLine,
    display_text: String,
    display_to_buffer: Vec<usize>,
    buffer_to_display: Vec<usize>,
    cell_range: Range<usize>,
    cell_content_len: usize,
}

struct InlineLineDisplayInfo {
    buffer_to_display: Vec<usize>,
    font_size: f32,
    line_height: f32,
    wrapped_line: WrappedLine,
    indent_px: f32,
    content_x: f32,
}

struct MultiCursorImeEdit {
    start: usize,
    selection_start: usize,
    selection_end: usize,
    marked_range: Option<Range<usize>>,
    is_primary: bool,
    delta: isize,
}

fn paired_delimiter(input: &str) -> Option<(&'static str, &'static str)> {
    match input {
        "[" => Some(("[", "]")),
        "{" => Some(("{", "}")),
        "(" => Some(("(", ")")),
        "<" => Some(("<", ">")),
        "\"" => Some(("\"", "\"")),
        "'" => Some(("'", "'")),
        "`" => Some(("`", "`")),
        "【" => Some(("【", "】")),
        "｛" => Some(("｛", "｝")),
        "（" => Some(("（", "）")),
        "《" => Some(("《", "》")),
        "〈" => Some(("〈", "〉")),
        "「" => Some(("「", "」")),
        "『" => Some(("『", "』")),
        "“" => Some(("“", "”")),
        "‘" => Some(("‘", "’")),
        _ => None,
    }
}

fn is_auto_closing_delimiter(input: &str) -> bool {
    matches!(
        input,
        "]" | "}"
            | ")"
            | ">"
            | "\""
            | "'"
            | "`"
            | "】"
            | "｝"
            | "）"
            | "》"
            | "〉"
            | "」"
            | "』"
            | "”"
            | "’"
    )
}

fn auto_paired_delimiter_matches(open: char, close: char) -> bool {
    matches!(
        (open, close),
        ('[', ']')
            | ('{', '}')
            | ('(', ')')
            | ('<', '>')
            | ('"', '"')
            | ('\'', '\'')
            | ('`', '`')
            | ('【', '】')
            | ('｛', '｝')
            | ('（', '）')
            | ('《', '》')
            | ('〈', '〉')
            | ('「', '」')
            | ('『', '』')
            | ('“', '”')
            | ('‘', '’')
    )
}

fn code_block_autocomplete_range(
    text: &str,
    cursor_offset: usize,
    new_text: &str,
) -> Option<Range<usize>> {
    if new_text != "`" || cursor_offset < 2 {
        return None;
    }

    if text.get(cursor_offset - 2..cursor_offset) != Some("``") {
        return None;
    }

    if cursor_offset >= 3 && text.get(cursor_offset - 3..cursor_offset - 2) == Some("`") {
        return None;
    }

    Some(cursor_offset - 2..cursor_offset)
}

fn ime_debug_enabled() -> bool {
    false
}

fn ime_debug_log(_message: impl AsRef<str>) {
    if ime_debug_enabled() {
        eprintln!("[ime-debug] logging enabled");
    }
}

impl EditorView {
    fn should_skip_auto_closing_delimiter(&self, new_text: &str) -> bool {
        if !is_auto_closing_delimiter(new_text) {
            return false;
        }

        self.buffer
            .text()
            .get(self.cursor.offset..self.cursor.offset + new_text.len())
            == Some(new_text)
    }

    pub(crate) fn auto_paired_backspace_range(&self) -> Option<Range<usize>> {
        if self.cursor.has_selection() {
            return None;
        }

        let text = self.buffer.text();
        let offset = self.cursor.offset;

        if offset >= 2
            && text.get(offset - 2..offset) == Some("[[")
            && text.get(offset..offset + 2) == Some("]]")
        {
            return Some(offset - 2..offset + 2);
        }

        let (prev_start, prev_char) = text[..offset].char_indices().next_back()?;
        let next_char = text[offset..].chars().next()?;
        let next_end = offset + next_char.len_utf8();
        if auto_paired_delimiter_matches(prev_char, next_char) {
            Some(prev_start..next_end)
        } else {
            None
        }
    }

    fn begin_ime_composition(&mut self) {
        if self.ime_composition_snapshot.is_none() {
            self.ime_composition_snapshot = Some(self.current_snapshot());
        }
    }

    fn collect_multi_cursor_ime_targets_desc(
        &self,
        primary_override_range: Option<Range<usize>>,
    ) -> Vec<(Range<usize>, bool)> {
        let primary_range = primary_override_range
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());

        let mut all = Vec::with_capacity(1 + self.extra_cursors.len());
        all.push((
            primary_range.start.min(primary_range.end)..primary_range.end.max(primary_range.start),
            true,
        ));

        for (index, cursor) in self.extra_cursors.iter().enumerate() {
            let range = self
                .extra_marked_ranges
                .get(index)
                .cloned()
                .unwrap_or_else(|| {
                    cursor.offset.min(cursor.anchor)..cursor.offset.max(cursor.anchor)
                });
            all.push((
                range.start.min(range.end)..range.end.max(range.start),
                false,
            ));
        }

        all.sort_by(|a, b| {
            b.0.start
                .cmp(&a.0.start)
                .then_with(|| b.0.end.cmp(&a.0.end))
        });
        all
    }

    fn apply_multi_cursor_ime_replace(
        &mut self,
        primary_override_range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<&Range<usize>>,
        keep_marked_ranges: bool,
    ) -> bool {
        let selection_relative = new_selected_range_utf16
            .map(|range_utf16| {
                (
                    Self::utf8_offset_in_text_for_utf16(new_text, range_utf16.start),
                    Self::utf8_offset_in_text_for_utf16(new_text, range_utf16.end),
                )
            })
            .unwrap_or((new_text.len(), new_text.len()));

        let mut range_spans_multiple_lines = false;
        let mut edits = Vec::with_capacity(1 + self.extra_cursors.len());

        for (range, is_primary) in
            self.collect_multi_cursor_ime_targets_desc(primary_override_range)
        {
            let (start_line, _) = self.buffer.offset_to_line_col(range.start);
            let (end_line, _) = self.buffer.offset_to_line_col(range.end);
            range_spans_multiple_lines |= start_line != end_line;

            let before_len = self.buffer.len();
            self.buffer.replace(range.clone(), new_text);
            let after_len = self.buffer.len();

            let mark_start = range.start;
            let mark_end = mark_start + new_text.len();
            edits.push(MultiCursorImeEdit {
                start: range.start,
                selection_start: mark_start + selection_relative.0,
                selection_end: mark_start + selection_relative.1,
                marked_range: if keep_marked_ranges && !new_text.is_empty() {
                    Some(mark_start..mark_end)
                } else {
                    None
                },
                is_primary,
                delta: after_len as isize - before_len as isize,
            });
        }

        edits.sort_by_key(|edit| edit.start);
        self.clear_marked_ranges();
        self.extra_cursors.clear();

        let mut cumulative_delta = 0isize;
        for edit in edits {
            let anchor = self
                .buffer
                .clamp_offset(apply_offset_delta(edit.selection_start, cumulative_delta));
            let offset = self
                .buffer
                .clamp_offset(apply_offset_delta(edit.selection_end, cumulative_delta));
            let marked_range = edit.marked_range.map(|range| {
                let start = self
                    .buffer
                    .clamp_offset(apply_offset_delta(range.start, cumulative_delta));
                let end = self
                    .buffer
                    .clamp_offset(apply_offset_delta(range.end, cumulative_delta));
                start..end
            });

            if edit.is_primary {
                self.cursor.anchor = anchor;
                self.cursor.offset = offset;
                self.marked_range = marked_range;
            } else {
                self.extra_cursors.push(Cursor { offset, anchor });
                if let Some(marked_range) = marked_range {
                    self.extra_marked_ranges.push(marked_range);
                }
            }

            cumulative_delta += edit.delta;
        }

        range_spans_multiple_lines
    }

    fn finish_ime_composition(&mut self) {
        let Some(snapshot) = self.ime_composition_snapshot.take() else {
            return;
        };

        let changed = snapshot.text != self.buffer.text()
            || snapshot.cursor_offset != self.cursor.offset
            || snapshot.cursor_anchor != self.cursor.anchor
            || snapshot.extra_cursors
                != self
                    .extra_cursors
                    .iter()
                    .map(|cursor| (cursor.offset, cursor.anchor))
                    .collect::<Vec<_>>();
        if changed {
            self.push_undo_snapshot(snapshot);
        }
    }

    pub(crate) fn primary_cursor_has_inline_edit_headroom(&self) -> bool {
        let cursor_line = self.cursor_line();
        let cursor_y = self.line_y_offset(cursor_line);
        let cursor_line_height = self.visual_height_for_line(cursor_line);
        let viewport_top = self.scroll_offset + self.padding_top();
        let viewport_bottom = self.scroll_offset + self.last_viewport_height - self.padding_top();

        if cursor_y < viewport_top || cursor_y + cursor_line_height > viewport_bottom {
            return false;
        }

        let next_visual_line_height = self
            .decorations
            .get(cursor_line)
            .map(|decoration| self.line_height_for_block(&decoration.block_type))
            .unwrap_or_else(|| self.line_height_for_block(&BlockType::Normal));

        cursor_y + cursor_line_height + next_visual_line_height <= viewport_bottom
    }

    // ── Mouse handlers ──────────────────────────────────────────────

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_image() && event.button == MouseButton::Left {
            if event.click_count >= 2 {
                self.reset_active_image_view();
                self.clamp_active_image_view();
                self.image_drag_state = None;
                cx.notify();
                return;
            }

            if let Some(state) = self.active_image_view_state() {
                self.image_drag_state = Some(ImageDragState {
                    start_position: event.position,
                    start_pan_x: state.pan_x,
                    start_pan_y: state.pan_y,
                });
                cx.notify();
                return;
            }
        }

        if self.context_menu.is_some() {
            return;
        }

        if let Some(&(_, start_line)) = self
            .code_block_copy_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.copy_code_block_group_to_clipboard(start_line, cx);
            return;
        }

        self.selected_handle = None;
        if let Some(&(_, line_idx, is_image_fold)) = self
            .gutter_fold_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
        {
            if is_image_fold {
                self.toggle_image_fold(line_idx, cx);
            } else {
                self.toggle_fold(line_idx, cx);
            }
            return;
        }

        if let Some(&(_, line_idx)) = self
            .image_edit_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.pick_image_for_line(line_idx, cx);
            return;
        }

        // Table column handle drag start
        if let Some(&(_, table_start, col)) = self
            .table_col_handle_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
        {
            self.selected_handle = Some((table_start, true, col));
            self.marked_range = None;
            self.clear_extra_cursors();
            self.is_selecting = false;
            if !self.block_is_front_matter(table_start) {
                self.table_drag = Some(TableDragState::DraggingColumn {
                    table_start_line: table_start,
                    source_col: col,
                    current_x: f32::from(event.position.x),
                });
            }
            cx.notify();
            return;
        }

        // Table row handle drag start
        if let Some(&(_, table_start, visual_row)) = self
            .table_row_handle_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
        {
            self.selected_handle = Some((table_start, false, visual_row));
            self.marked_range = None;
            self.clear_extra_cursors();
            self.is_selecting = false;
            self.table_drag = Some(TableDragState::DraggingRow {
                table_start_line: table_start,
                source_row: visual_row,
                current_y: f32::from(event.position.y),
            });
            cx.notify();
            return;
        }

        if let Some(line_idx) = self
            .image_click_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
        {
            if self.select_image_syntax_on_line(line_idx) {
                cx.notify();
                return;
            }
        }

        if let Some(line) = self
            .checkbox_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
        {
            self.checkpoint();
            self.toggle_checkbox(line);
            self.reparse();
            self.hovering_checkbox = true;
            cx.notify();
            return;
        }

        self.clear_vertical_move_goal_x();

        // Table cell click: position cursor at the correct cell offset
        if event.button == MouseButton::Left {
            if let Some(&(bounds, table_start, visual_row, col)) = self
                .table_click_areas
                .iter()
                .find(|(bounds, _, _, _)| bounds.contains(&event.position))
            {
                if let Some(offset) = self.table_cell_offset_for_position(
                    window,
                    table_start,
                    visual_row,
                    col,
                    &bounds,
                    event.position,
                ) {
                    ime_debug_log(format!(
                        "table cell mouse_down table_start={} visual_row={} col={} offset={} shift={} click_count={}",
                        table_start,
                        visual_row,
                        col,
                        offset,
                        event.modifiers.shift,
                        event.click_count
                    ));
                    if event.click_count >= 2 {
                        self.clear_extra_cursors();
                        self.clear_wrap_boundary_affinity();
                        self.select_word_at_offset(offset);
                        self.is_selecting = false;
                    } else if event.modifiers.shift {
                        self.cursor.select_to(offset, &self.buffer);
                        self.clear_wrap_boundary_affinity();
                        self.is_selecting = false;
                    } else {
                        self.cursor.move_to(offset, &self.buffer);
                        self.clear_wrap_boundary_affinity();
                        self.is_selecting = true;
                    }
                    cx.notify();
                    return;
                }
            }
        }

        if event.button == MouseButton::Left && event.click_count >= 2 {
            self.clear_extra_cursors();
            self.clear_wrap_boundary_affinity();
            let offset = self.offset_for_position(event.position);
            self.select_word_at_offset(offset);
            self.is_selecting = false;
            cx.notify();
            return;
        }

        self.is_selecting = true;
        let prev_offset = self.cursor.offset;
        let offset = self.offset_for_position(event.position);
        self.update_wrap_boundary_affinity_for_position(offset, event.position);
        if event.modifiers.platform {
            // Cmd+click: open link if cursor is on a link span
            let (line, col) = self.buffer.offset_to_line_col(offset);
            let wiki_link_path = self.decorations.get(line).and_then(|dec| {
                dec.wiki_links.iter().find_map(|wiki_link| {
                    if col < wiki_link.full_range.start || col > wiki_link.full_range.end {
                        return None;
                    }
                    resolve_wiki_link_target_path(&self.sidebar_root, &wiki_link.target_path)
                })
            });
            if let Some(path) = wiki_link_path {
                self.open_path_in_permanent_tab(path, true, cx);
                self.is_selecting = false;
                cx.notify();
                return;
            }
            let link_url = self.decorations.get(line).and_then(|dec| {
                dec.inline_spans.iter().find_map(|span| {
                    if span.kind != InlineKind::Link {
                        return None;
                    }
                    let prefix_len = 1;
                    let full_start = span.range.start.saturating_sub(prefix_len);
                    let full_end = dec
                        .syntax_ranges
                        .iter()
                        .filter(|sr| sr.range.start >= span.range.end)
                        .map(|sr| sr.range.end)
                        .max()
                        .unwrap_or(span.range.end);
                    if col >= full_start && col <= full_end {
                        let line_content = self.buffer.line_content(line);
                        let source = &line_content[full_start..full_end.min(line_content.len())];
                        source.find("](").and_then(|bp| {
                            let url = &source[bp + 2..source.len().saturating_sub(1)];
                            if url.is_empty() {
                                None
                            } else {
                                Some(url.to_string())
                            }
                        })
                    } else {
                        None
                    }
                })
            });
            if let Some(url) = link_url {
                if url.starts_with("http://")
                    || url.starts_with("https://")
                    || url.starts_with("mailto:")
                {
                    if let Err(e) = std::process::Command::new("open").arg(&url).spawn() {
                        eprintln!("failed to open URL: {e}");
                    }
                }
                self.is_selecting = false;
                cx.notify();
                return;
            }
            // Cmd+click: add a new cursor
            self.add_cursor_at(offset, offset);
            self.is_selecting = false;
            cx.notify();
            return;
        }
        // Clear extra cursors on plain click
        self.clear_extra_cursors();
        if event.modifiers.shift {
            self.cursor.select_to(offset, &self.buffer);
        } else {
            self.cursor.move_to(offset, &self.buffer);
            self.maybe_auto_select_link_url(prev_offset, true, cx);
        }
        cx.notify();
    }

    fn select_image_syntax_on_line(&mut self, line_idx: usize) -> bool {
        let Some(decoration) = self.decorations.get(line_idx) else {
            return false;
        };
        let Some(span) = decoration
            .inline_spans
            .iter()
            .find(|span| span.kind == InlineKind::Image)
        else {
            return false;
        };

        let line_start = self.buffer.line_start(line_idx);
        let line_content = self.buffer.line_content(line_idx);
        let full_start = span.range.start.saturating_sub(2);
        let full_end = decoration
            .syntax_ranges
            .iter()
            .filter(|range| range.range.start >= span.range.end)
            .map(|range| range.range.end)
            .max()
            .unwrap_or(span.range.end)
            .min(line_content.len());
        self.cursor.offset = line_start + full_start;
        self.cursor.anchor = line_start + full_end;
        true
    }

    pub(crate) fn maybe_auto_select_link_url(
        &mut self,
        prev_offset: usize,
        also_select_desc: bool,
        cx: &mut Context<Self>,
    ) {
        let new_offset = self.cursor.offset;
        let (line, _) = self.buffer.offset_to_line_col(new_offset);
        let line_start = self.buffer.line_start(line);
        let mut matched_inline_link = false;
        if let Some(dec) = self.decorations.get(line) {
            let spans: Vec<_> = dec.inline_spans.clone();
            let syntax_ranges: Vec<_> = dec.syntax_ranges.clone();
            'link_check: for span in &spans {
                if !matches!(span.kind, InlineKind::Link | InlineKind::Image) {
                    continue;
                }
                let prefix_len = if span.kind == InlineKind::Image { 2 } else { 1 };
                let _full_link_start = line_start + span.range.start.saturating_sub(prefix_len);
                let url_sr = syntax_ranges.iter().find(|sr| {
                    sr.range.start == span.range.end && sr.range.end > sr.range.start + 1
                });
                let _full_link_end = url_sr
                    .map(|sr| line_start + sr.range.end)
                    .unwrap_or(line_start + span.range.end);

                if let Some(url_sr) = &url_sr {
                    let url_abs_start = line_start + url_sr.range.start + 2;
                    let url_abs_end = line_start + url_sr.range.end.saturating_sub(1);
                    if prev_offset >= url_abs_start && prev_offset <= url_abs_end {
                        continue;
                    }
                }

                if let Some(url_sr) = url_sr {
                    let url_abs_start = line_start + url_sr.range.start + 2;
                    let url_abs_end = line_start + url_sr.range.end.saturating_sub(1);
                    if url_abs_start < url_abs_end
                        && new_offset >= url_abs_start
                        && new_offset <= url_abs_end
                    {
                        self.cursor.anchor = url_abs_start;
                        self.cursor.offset = url_abs_end;
                        self.is_selecting = false;
                        matched_inline_link = true;
                        break 'link_check;
                    }
                }

                if also_select_desc {
                    let desc_abs_start = line_start + span.range.start;
                    let desc_abs_end = line_start + span.range.end;
                    if new_offset >= desc_abs_start && new_offset <= desc_abs_end {
                        self.cursor.anchor = desc_abs_start;
                        self.cursor.offset = desc_abs_end;
                        self.is_selecting = false;
                        matched_inline_link = true;
                        break 'link_check;
                    }
                }
            }

            if !matched_inline_link {
                for wiki_link in &dec.wiki_links {
                    let full_abs_start = line_start + wiki_link.full_range.start;
                    let full_abs_end = line_start + wiki_link.full_range.end;
                    let inner_abs_start = full_abs_start + 2;
                    let inner_abs_end = full_abs_end.saturating_sub(2);

                    if new_offset < inner_abs_start || new_offset > inner_abs_end {
                        continue;
                    }

                    let entered_from_outside =
                        prev_offset < full_abs_start || prev_offset > full_abs_end;
                    if entered_from_outside && inner_abs_start < inner_abs_end {
                        self.cursor.anchor = inner_abs_start;
                        self.cursor.offset = inner_abs_end;
                        self.is_selecting = false;
                    }

                    if self.wiki_link_start != Some(full_abs_start) {
                        self.open_wiki_link_menu(full_abs_start, cx);
                    }
                    self.update_wiki_link_query(cx);
                    return;
                }
            }
        }

        if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        }
    }

    pub(crate) fn on_editor_context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu.is_some() {
            return;
        }
        self.is_selecting = false;

        // Check if right-clicking inside a table cell
        if let Some(&(bounds, table_start, visual_row, col)) = self
            .table_click_areas
            .iter()
            .find(|(bounds, _, _, _)| bounds.contains(&event.position))
        {
            if let Some(offset) = self.table_cell_offset_for_position(
                window,
                table_start,
                visual_row,
                col,
                &bounds,
                event.position,
            ) {
                self.cursor.move_to(offset, &self.buffer);
            }
            let is_front_matter = self
                .decorations
                .get(table_start)
                .and_then(|dec| dec.table_info.as_ref())
                .is_some_and(|ti| ti.is_front_matter);
            self.context_menu = Some(ContextMenu {
                position: event.position,
                kind: ContextMenuKind::Table {
                    table_start_line: table_start,
                    row: visual_row,
                    col,
                },
            });
            if is_front_matter {
                self.selected_handle = Some((table_start, false, visual_row));
            }
            cx.notify();
            return;
        }

        if let Some(line_idx) = self
            .image_click_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
        {
            if self.select_image_syntax_on_line(line_idx) {
                self.open_format_context_menu(event.position, cx);
                cx.notify();
                return;
            }
        }

        let offset = self.offset_for_position(event.position);
        let selection = self.cursor.selection_range();
        let clicked_selected_text =
            self.cursor.has_selection() && offset >= selection.start && offset <= selection.end;
        if !clicked_selected_text {
            self.cursor.move_to(offset, &self.buffer);
        }

        self.open_format_context_menu(event.position, cx);
    }

    pub(crate) fn select_word_at_offset(&mut self, offset: usize) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let line_start = self.buffer.line_start(line);
        let line_content = self.buffer.line_content(line);
        if line_content.is_empty() {
            self.cursor.move_to(line_start, &self.buffer);
            return;
        }

        let mut probe = col.min(line_content.len());
        while probe > 0 && !line_content.is_char_boundary(probe) {
            probe -= 1;
        }
        if probe == line_content.len() {
            probe = line_content
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }

        let Some(ch) = line_content[probe..].chars().next() else {
            self.cursor.move_to(line_start + probe, &self.buffer);
            return;
        };
        let class = classify_word_char(ch);
        if matches!(class, WordCharClass::Whitespace | WordCharClass::Other) {
            self.cursor.move_to(line_start + probe, &self.buffer);
            return;
        }

        let mut start = probe;
        while start > 0 {
            let prev = line_content[..start]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            let Some(prev_ch) = line_content[prev..].chars().next() else {
                break;
            };
            if classify_word_char(prev_ch) != class {
                break;
            }
            start = prev;
            if start == 0 {
                break;
            }
        }

        let mut end = probe + ch.len_utf8();
        while end < line_content.len() {
            let Some(next_ch) = line_content[end..].chars().next() else {
                break;
            };
            if classify_word_char(next_ch) != class {
                break;
            }
            end += next_ch.len_utf8();
        }

        self.cursor.anchor = line_start + start;
        self.cursor.offset = line_start + end;
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.image_drag_state.take().is_some() {
            cx.notify();
            return;
        }

        // Handle table drag completion
        if let Some(drag) = self.table_drag.take() {
            match drag {
                TableDragState::DraggingColumn {
                    table_start_line,
                    source_col,
                    current_x,
                } => {
                    if let Some(insert_at) = self.table_col_insert_at_x(table_start_line, current_x)
                    {
                        if insert_at != source_col && insert_at != source_col + 1 {
                            if let Some((start, end, lines)) =
                                self.table_lines_for_block(table_start_line)
                            {
                                let new_lines =
                                    crate::table_ops::move_column(&lines, source_col, insert_at);
                                self.replace_table_block(start, end, &new_lines);
                                let selected_col = if insert_at > source_col {
                                    insert_at.saturating_sub(1)
                                } else {
                                    insert_at
                                };
                                self.selected_handle = Some((table_start_line, true, selected_col));
                            }
                        } else {
                            self.selected_handle = Some((table_start_line, true, source_col));
                        }
                    }
                }
                TableDragState::DraggingRow {
                    table_start_line,
                    source_row,
                    current_y,
                } => {
                    if let Some(insert_visual) =
                        self.table_row_insert_at_y(table_start_line, current_y)
                    {
                        if insert_visual != source_row && insert_visual != source_row + 1 {
                            if let Some((start, end, lines)) =
                                self.table_lines_for_block(table_start_line)
                            {
                                let new_lines = if self.block_is_front_matter(table_start_line) {
                                    crate::table_ops::move_front_matter_row(
                                        &lines,
                                        source_row,
                                        insert_visual,
                                    )
                                } else {
                                    crate::table_ops::move_row(&lines, source_row, insert_visual)
                                };
                                self.replace_table_block(start, end, &new_lines);
                                let selected_row = if insert_visual > source_row {
                                    insert_visual.saturating_sub(1)
                                } else {
                                    insert_visual
                                };
                                self.selected_handle =
                                    Some((table_start_line, false, selected_row));
                            }
                        } else {
                            self.selected_handle = Some((table_start_line, false, source_row));
                        }
                    }
                }
            }
            cx.notify();
            return;
        }
        self.is_selecting = false;
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let toolbar_hover_changed = self.update_toolbar_corner_hover(event.position);
        if self.active_tab_is_image() {
            if let Some(drag) = self.image_drag_state {
                if let Some(image_view) = self.active_image_view_state_mut() {
                    image_view.pan_x = drag.start_pan_x
                        + (f32::from(event.position.x) - f32::from(drag.start_position.x));
                    image_view.pan_y = drag.start_pan_y
                        + (f32::from(event.position.y) - f32::from(drag.start_position.y));
                }
                self.clamp_active_image_view();
                if toolbar_hover_changed {
                    cx.notify();
                }
                cx.notify();
                return;
            }
        }

        // Track command key from mouse events for link underlines
        let cmd = event.modifiers.platform;
        if cmd != self.command_held {
            self.command_held = cmd;
            cx.notify();
        }

        // Update table drag position
        if let Some(ref mut drag) = self.table_drag {
            match drag {
                TableDragState::DraggingColumn { current_x, .. } => {
                    *current_x = f32::from(event.position.x);
                }
                TableDragState::DraggingRow { current_y, .. } => {
                    *current_y = f32::from(event.position.y);
                }
            }
            cx.notify();
            return;
        }

        if self.context_menu.is_some() {
            if toolbar_hover_changed {
                cx.notify();
            }
            return;
        }

        let hovering_code_block = self
            .code_block_hover_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, start_line)| *start_line);
        if hovering_code_block != self.hovering_code_block_start {
            self.hovering_code_block_start = hovering_code_block;
            cx.notify();
        }

        let hovering_code_block_copy = self
            .code_block_copy_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, start_line)| *start_line);
        if hovering_code_block_copy != self.hovering_code_block_copy_start {
            self.hovering_code_block_copy_start = hovering_code_block_copy;
            cx.notify();
        }

        let hovering_checkbox = self
            .checkbox_areas
            .iter()
            .any(|(bounds, _)| bounds.contains(&event.position));
        if hovering_checkbox != self.hovering_checkbox {
            self.hovering_checkbox = hovering_checkbox;
            cx.notify();
        }

        let hovering_image_edit = self
            .image_edit_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line);
        if hovering_image_edit != self.hovering_image_edit_line {
            self.hovering_image_edit_line = hovering_image_edit;
            cx.notify();
        }

        let hovering_fold = self
            .gutter_fold_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
            .map(|(_, line, _)| *line);
        if hovering_fold != self.hovering_fold_line {
            self.hovering_fold_line = hovering_fold;
            cx.notify();
        }

        let hovering_gutter_fold_area = f32::from(event.position.x) <= self.last_content_left;
        if hovering_gutter_fold_area != self.hovering_gutter_fold_area {
            self.hovering_gutter_fold_area = hovering_gutter_fold_area;
            cx.notify();
        }

        // Track which table the mouse is hovering near (for handle visibility)
        let hovering_table = self
            .table_click_areas
            .iter()
            .find(|(bounds, _, _, _)| {
                // Expand bounds slightly to include handle areas
                let expanded = gpui::Bounds::new(
                    gpui::point(
                        bounds.origin.x - gpui::px(30.0),
                        bounds.origin.y - gpui::px(30.0),
                    ),
                    gpui::size(
                        bounds.size.width + gpui::px(60.0),
                        bounds.size.height + gpui::px(60.0),
                    ),
                );
                expanded.contains(&event.position)
            })
            .map(|(_, table_start, _, _)| *table_start);
        if hovering_table != self.hovering_table_start {
            self.hovering_table_start = hovering_table;
            cx.notify();
        }

        // Track which specific handle the mouse is over
        let hovering_handle = self
            .table_col_handle_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
            .map(|(_, tsl, col)| (*tsl, true, *col))
            .or_else(|| {
                self.table_row_handle_areas
                    .iter()
                    .find(|(bounds, _, _)| bounds.contains(&event.position))
                    .map(|(_, tsl, row)| (*tsl, false, *row))
            });
        if hovering_handle != self.hovering_handle {
            self.hovering_handle = hovering_handle;
            cx.notify();
        }

        if self.is_selecting {
            let pos_y: f32 = event.position.y.into();
            let origin_y: f32 = self.content_origin.y.into();
            let rel_pos_y = pos_y - origin_y;

            let scroll_margin = 30.0;
            if rel_pos_y < scroll_margin {
                let speed = ((scroll_margin - rel_pos_y) / scroll_margin * 15.0).max(2.0);
                self.scroll_offset -= speed;
                self.clamp_scroll();
            } else if rel_pos_y > self.last_viewport_height - scroll_margin {
                let speed = ((rel_pos_y - (self.last_viewport_height - scroll_margin))
                    / scroll_margin
                    * 15.0)
                    .max(2.0);
                self.scroll_offset += speed;
                self.clamp_scroll();
            }

            // If dragging within a table cell, use table-aware positioning
            let offset = if let Some(&(bounds, table_start, visual_row, col)) = self
                .table_click_areas
                .iter()
                .find(|(bounds, _, _, _)| bounds.contains(&event.position))
            {
                self.table_cell_offset_for_position(
                    window,
                    table_start,
                    visual_row,
                    col,
                    &bounds,
                    event.position,
                )
                .unwrap_or_else(|| self.offset_for_position(event.position))
            } else {
                self.offset_for_position(event.position)
            };
            self.update_wrap_boundary_affinity_for_position(offset, event.position);
            self.cursor.select_to(offset, &self.buffer);
            cx.notify();
        } else if toolbar_hover_changed {
            cx.notify();
        }
    }

    fn table_col_insert_at_x(&self, table_start_line: usize, x: f32) -> Option<usize> {
        let mut header_cells: Vec<_> = self
            .table_click_areas
            .iter()
            .filter(|(_, tsl, visual_row, _)| *tsl == table_start_line && *visual_row == 0)
            .map(|(bounds, _, _, col)| (*col, *bounds))
            .collect();
        header_cells.sort_by_key(|(col, _)| *col);
        if header_cells.is_empty() {
            return None;
        }

        let mut boundaries = Vec::with_capacity(header_cells.len() + 1);
        boundaries.push(f32::from(header_cells[0].1.origin.x));
        for (_, bounds) in &header_cells {
            boundaries.push(f32::from(bounds.origin.x) + f32::from(bounds.size.width));
        }

        boundaries
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (*a - x)
                    .abs()
                    .partial_cmp(&(*b - x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
    }

    fn table_row_insert_at_y(&self, table_start_line: usize, y: f32) -> Option<usize> {
        let mut rows: Vec<_> = self
            .table_click_areas
            .iter()
            .filter(|(_, tsl, _, col)| *tsl == table_start_line && *col == 0)
            .map(|(bounds, _, visual_row, _)| (*visual_row, *bounds))
            .collect();
        rows.sort_by_key(|(visual_row, _)| *visual_row);
        if rows.is_empty() {
            return None;
        }

        let mut boundaries = Vec::with_capacity(rows.len() + 1);
        boundaries.push(f32::from(rows[0].1.origin.y));
        for (_, bounds) in &rows {
            boundaries.push(f32::from(bounds.origin.y) + f32::from(bounds.size.height));
        }

        boundaries
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (*a - y)
                    .abs()
                    .partial_cmp(&(*b - y).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
    }

    /// Map a visual row index (skipping separator rows) back to the source buffer line.
    fn table_source_line_for_visual_row(
        &self,
        table_start: usize,
        visual_row: usize,
    ) -> Option<usize> {
        let header_info = self.decorations.get(table_start)?.table_info.as_ref()?;
        let end = header_info.table_end_line;
        let mut vi = 0;
        for line_idx in table_start..end {
            if let Some(dec) = self.decorations.get(line_idx) {
                if let Some(ref ti) = dec.table_info {
                    if !ti.is_separator {
                        if vi == visual_row {
                            return Some(line_idx);
                        }
                        vi += 1;
                    }
                }
            }
        }
        None
    }

    fn table_visual_row_for_source_line(
        &self,
        table_start: usize,
        source_line: usize,
    ) -> Option<usize> {
        let header_info = self.decorations.get(table_start)?.table_info.as_ref()?;
        let mut visual_row = 0;
        for line_idx in table_start..header_info.table_end_line {
            let table_info = self
                .decorations
                .get(line_idx)
                .and_then(|decoration| decoration.table_info.as_ref())?;
            if table_info.is_separator {
                continue;
            }
            if line_idx == source_line {
                return Some(visual_row);
            }
            visual_row += 1;
        }
        None
    }

    fn table_cell_display_info(
        &self,
        window: &mut Window,
        source_line: usize,
        col: usize,
        cell_inner_width: f32,
    ) -> Option<TableCellDisplayInfo> {
        let dec = self.decorations.get(source_line)?;
        let ti = dec.table_info.as_ref()?;
        let cell_content = ti.cells.get(col).cloned().unwrap_or_default();
        let cell_range = ti.cell_ranges.get(col).cloned().unwrap_or(0..0);
        let line_start = self.buffer.line_start(source_line);
        let cursor_offsets = std::iter::once(self.cursor.offset)
            .chain(self.extra_cursors.iter().map(|cursor| cursor.offset))
            .collect::<Vec<_>>();
        let cell_active_ranges = helpers::active_table_cell_ranges(
            line_start,
            &cell_range,
            &cursor_offsets,
            self.marked_range.as_ref(),
        );
        let cell_decoration = crate::markdown::parse_decorations(&cell_content)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (folded_raw, d2b_raw, b2d_raw, _) = helpers::build_display_text(
            &cell_content,
            &cell_decoration,
            &cell_active_ranges,
            false,
            None,
            0,
            0,
            0,
            0,
        );
        let trim_start = folded_raw.len() - folded_raw.trim_start().len();
        let trim_end = folded_raw.len() - folded_raw.trim_end().len();
        let display_text = folded_raw.trim().to_string();
        let display_to_buffer = d2b_raw[trim_start..d2b_raw.len() - trim_end].to_vec();
        let buffer_to_display = b2d_raw
            .iter()
            .map(|&display_index| display_index.saturating_sub(trim_start))
            .collect();

        let text_style = window.text_style();
        let editor_styles = &self.styles.editor;
        let configured_font_family = |configured: &'static str| {
            if configured == ".SystemUIFont" {
                text_style.font().family.clone()
            } else {
                configured.into()
            }
        };
        let body_cjk_fallbacks = |configured: &'static str| {
            (configured != editor_styles.editor_body_cjk_font_family).then(|| {
                FontFallbacks::from_fonts(vec![
                    editor_styles.editor_body_cjk_font_family.to_string(),
                ])
            })
        };

        let mut monospace_font = text_style.font();
        monospace_font.family = configured_font_family(editor_styles.editor_monospace_font_family);
        monospace_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_monospace_font_family);

        let mut body_font = text_style.font();
        body_font.family = configured_font_family(editor_styles.editor_body_latin_font_family);
        body_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_body_latin_font_family);

        let mut body_cjk_font = text_style.font();
        body_cjk_font.family = configured_font_family(editor_styles.editor_body_cjk_font_family);
        body_cjk_font.fallbacks = None;

        let mut heading_font = text_style.font();
        heading_font.family =
            configured_font_family(editor_styles.editor_heading_latin_font_family);
        heading_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_heading_latin_font_family);

        let mut heading_cjk_font = text_style.font();
        heading_cjk_font.family =
            configured_font_family(editor_styles.editor_heading_cjk_font_family);
        heading_cjk_font.fallbacks = None;

        let font_size = self.base_font_size();
        let mut cell_runs = helpers::build_text_runs_for_display(
            &display_text,
            &cell_content,
            &display_to_buffer,
            &cell_decoration,
            &cell_active_ranges,
            false,
            false,
            &body_font,
            &body_cjk_font,
            &monospace_font,
            &heading_font,
            &heading_cjk_font,
            0,
            font_size,
            &self.theme,
            self.styles.wiki_link.wiki_link_text_color,
            self.styles.wiki_link.wiki_link_text_weight,
        );
        if ti.is_header {
            for run in &mut cell_runs {
                run.font.weight = gpui::FontWeight::BOLD;
            }
        }

        let cell_text: SharedString = if display_text.is_empty() {
            " ".into()
        } else {
            display_text.clone().into()
        };
        let wrapped = window
            .text_system()
            .shape_text(
                cell_text,
                px(font_size),
                &cell_runs,
                Some(px(cell_inner_width.max(font_size * 0.5))),
                None,
            )
            .ok()?
            .into_iter()
            .next()?;

        Some(TableCellDisplayInfo {
            wrapped,
            display_text,
            display_to_buffer,
            buffer_to_display,
            cell_range,
            cell_content_len: cell_content.len(),
        })
    }

    fn inline_line_display_info(
        &self,
        window: &mut Window,
        line_idx: usize,
        bounds: Bounds<Pixels>,
    ) -> Option<InlineLineDisplayInfo> {
        let decoration = self.decorations.get(line_idx)?.clone();
        if matches!(decoration.block_type, BlockType::Table) {
            return None;
        }

        let viewport_width: f32 = bounds.size.width.into();
        let origin_x: f32 = bounds.origin.x.into();
        let expanded_layout = self.editor_left_aligned;
        let editor_width = if expanded_layout {
            viewport_width
        } else {
            viewport_width.min(self.max_editor_width())
        };
        let centered_left = origin_x + ((viewport_width - editor_width) / 2.0).max(0.0);
        let editor_left = if expanded_layout {
            origin_x
        } else {
            centered_left
        };
        let text_width =
            (editor_width - self.padding_left() - self.padding_right()).max(self.scaled(120.0));
        let wrap_width = (text_width
            - self.scaled(self.styles.editor.editor_wrap_right_padding_px))
        .max(self.scaled(120.0));

        let text_style = window.text_style();
        let editor_styles = &self.styles.editor;
        let configured_font_family = |configured: &'static str| {
            if configured == ".SystemUIFont" {
                text_style.font().family.clone()
            } else {
                configured.into()
            }
        };
        let body_cjk_fallbacks = |configured: &'static str| {
            (configured != editor_styles.editor_body_cjk_font_family).then(|| {
                FontFallbacks::from_fonts(vec![
                    editor_styles.editor_body_cjk_font_family.to_string(),
                ])
            })
        };

        let mut monospace_font = text_style.font();
        monospace_font.family = configured_font_family(editor_styles.editor_monospace_font_family);
        monospace_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_monospace_font_family);

        let mut body_font = text_style.font();
        body_font.family = configured_font_family(editor_styles.editor_body_latin_font_family);
        body_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_body_latin_font_family);

        let mut body_cjk_font = text_style.font();
        body_cjk_font.family = configured_font_family(editor_styles.editor_body_cjk_font_family);
        body_cjk_font.fallbacks = None;

        let mut heading_font = text_style.font();
        heading_font.family =
            configured_font_family(editor_styles.editor_heading_latin_font_family);
        heading_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_heading_latin_font_family);

        let mut heading_cjk_font = text_style.font();
        heading_cjk_font.family =
            configured_font_family(editor_styles.editor_heading_cjk_font_family);
        heading_cjk_font.fallbacks = None;

        let base_char_width = {
            let test_run = TextRun {
                len: 1,
                font: monospace_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(test_lines) = window.text_system().shape_text(
                "x".into(),
                px(self.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                f32::from(test_lines[0].width())
            } else {
                self.base_font_size() * 0.6
            }
        };

        let table_cell_padding_px = self.scaled(self.styles.table.table_cell_padding_px);
        let table_cell_padding_chars = if table_cell_padding_px <= 0.0 {
            0
        } else {
            (table_cell_padding_px / base_char_width).ceil().max(1.0) as usize
        };
        let collapsed_task_prefix_chars = ((self.base_font_size()
            * self.styles.checkbox.checkbox_size_factor
            + self.scaled(self.styles.checkbox.checkbox_text_gap_px))
            / base_char_width)
            .ceil()
            .max(3.0) as usize;
        let checkbox_indent_step_chars = (self.scaled(self.styles.checkbox.checkbox_indent_step_px)
            / base_char_width)
            .ceil()
            .max(1.0) as usize;

        let is_code_block_type = |index: usize| {
            matches!(
                self.decorations.get(index).map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            )
        };

        let active_code_fence_group = if self.cursor_line() < self.buffer.line_count()
            && matches!(
                self.decorations
                    .get(self.cursor_line())
                    .map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            ) {
            let mut start = self.cursor_line();
            while start > 0 && is_code_block_type(start - 1) {
                start -= 1;
            }

            let mut end = self.cursor_line();
            while end + 1 < self.buffer.line_count() && is_code_block_type(end + 1) {
                end += 1;
            }

            Some((start, end))
        } else {
            None
        };

        let line_content = self.buffer.line_content(line_idx);
        let line_start_offset = self.buffer.line_start(line_idx);
        let line_end_offset = self.buffer.line_end(line_idx);
        let is_cursor_line = line_idx == self.cursor_line();
        let font_size = self.font_size_for_block(&decoration.block_type);
        let line_height = self.line_height_for_block(&decoration.block_type);
        let ime_active_on_line = self
            .marked_range
            .as_ref()
            .is_some_and(|marked| marked.start < line_end_offset && marked.end > line_start_offset);
        let cursor_col = if self.cursor.has_selection()
            && matches!(decoration.block_type, BlockType::Heading(_))
        {
            if is_cursor_line {
                Some(self.cursor.offset.saturating_sub(line_start_offset))
            } else {
                None
            }
        } else if self.is_selecting && self.cursor.has_selection() {
            let sel = self.cursor.selection_range();
            if sel.start >= line_start_offset && sel.end <= line_end_offset {
                let sel_start_col = sel.start - line_start_offset;
                let sel_end_col = sel.end - line_start_offset;
                let within_syntax = decoration.inline_spans.iter().any(|span| {
                    let prefix_len = match span.kind {
                        crate::markdown::InlineKind::Bold => 2,
                        crate::markdown::InlineKind::Italic => 1,
                        crate::markdown::InlineKind::Strikethrough => 2,
                        crate::markdown::InlineKind::InlineCode => 1,
                        crate::markdown::InlineKind::Link | crate::markdown::InlineKind::Image => {
                            if span.kind == crate::markdown::InlineKind::Image {
                                2
                            } else {
                                1
                            }
                        }
                    };
                    let full_start = span.range.start.saturating_sub(prefix_len);
                    let full_end = decoration
                        .syntax_ranges
                        .iter()
                        .filter(|syntax_range| syntax_range.range.end > span.range.end)
                        .map(|syntax_range| syntax_range.range.end)
                        .max()
                        .unwrap_or(span.range.end);
                    sel_start_col >= full_start && sel_end_col <= full_end
                });
                if within_syntax {
                    Some(self.cursor.offset.saturating_sub(line_start_offset))
                } else {
                    None
                }
            } else {
                None
            }
        } else if ime_active_on_line {
            Some(line_content.len())
        } else if is_cursor_line {
            Some(self.cursor.offset.saturating_sub(line_start_offset))
        } else {
            None
        };

        let all_sel_ranges: Vec<Range<usize>> = self
            .all_cursor_ranges()
            .into_iter()
            .map(|(offset, anchor)| offset.min(anchor)..offset.max(anchor))
            .collect();
        let mut active_ranges: Vec<Range<usize>> = all_sel_ranges
            .iter()
            .filter(|range| range.start != range.end)
            .filter_map(|range| {
                if range.end <= line_start_offset || range.start >= line_end_offset {
                    return None;
                }
                let start = range
                    .start
                    .saturating_sub(line_start_offset)
                    .min(line_content.len());
                let end = range
                    .end
                    .saturating_sub(line_start_offset)
                    .min(line_content.len());
                Some(start..end)
            })
            .collect();
        for marked in self.marked_ranges() {
            if marked.end <= line_start_offset || marked.start >= line_end_offset {
                continue;
            }
            if matches!(decoration.block_type, BlockType::Heading(_))
                && marked.end >= line_end_offset
            {
                continue;
            }
            let start = marked
                .start
                .saturating_sub(line_start_offset)
                .min(line_content.len());
            let end = marked
                .end
                .saturating_sub(line_start_offset)
                .min(line_content.len());
            active_ranges.push(start..end);
        }
        if active_ranges.is_empty()
            && let Some(col) = cursor_col
        {
            active_ranges.push(col.min(line_content.len())..col.min(line_content.len()));
        }

        let code_block_syntax_active = active_code_fence_group
            .is_some_and(|(start, end)| line_idx >= start && line_idx <= end);
        let (display_text, display_to_buffer, buffer_to_display, prefix_collapsed) =
            helpers::build_display_text(
                line_content,
                &decoration,
                &active_ranges,
                code_block_syntax_active,
                None,
                table_cell_padding_chars,
                collapsed_task_prefix_chars,
                checkbox_indent_step_chars,
                self.styles.checkbox.checkbox_parent_prefix_chars as usize,
            );

        let runs = helpers::build_text_runs_for_display(
            &display_text,
            line_content,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            code_block_syntax_active,
            prefix_collapsed,
            &body_font,
            &body_cjk_font,
            &monospace_font,
            &heading_font,
            &heading_cjk_font,
            collapsed_task_prefix_chars,
            font_size,
            &self.theme,
            self.styles.wiki_link.wiki_link_text_color,
            self.styles.wiki_link.wiki_link_text_weight,
        );

        let display_shared: SharedString = if display_text.is_empty() {
            " ".into()
        } else {
            display_text.clone().into()
        };

        let is_list_type = matches!(
            decoration.block_type,
            BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
        );
        let prefix_bytes = if is_list_type {
            helpers::compute_display_prefix_bytes(
                &display_text,
                &decoration.block_type,
                prefix_collapsed,
                collapsed_task_prefix_chars,
            )
        } else {
            0
        };
        let letter_spacing = editor_styles.editor_letter_spacing_px;
        let base_wrap_width = if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            (text_width
                - self.scaled(self.styles.code_block.code_block_text_padding_left_px)
                - self.scaled(self.styles.code_block.code_block_wrap_right_padding_px))
            .max(self.scaled(80.0))
        } else {
            wrap_width
        };
        let effective_wrap_width = if letter_spacing > 0.0 {
            base_wrap_width * font_size / (font_size + letter_spacing)
        } else {
            base_wrap_width
        };

        let Ok(mut wrapped_lines) = window.text_system().shape_text(
            display_shared,
            px(font_size),
            &runs,
            Some(px(effective_wrap_width)),
            None,
        ) else {
            return None;
        };

        if letter_spacing != 0.0 {
            for wrapped in &mut wrapped_lines {
                helpers::apply_letter_spacing(wrapped, letter_spacing);
            }
        }

        let wrapped_line = wrapped_lines.into_iter().next()?;
        let indent_px = if is_list_type && prefix_bytes > 0 && prefix_bytes <= display_text.len() {
            wrapped_line
                .position_for_index(prefix_bytes, px(line_height))
                .map(|pos| f32::from(pos.x))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let content_x = if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            editor_left
                + self.padding_left()
                + self.scaled(self.styles.code_block.code_block_text_padding_left_px)
        } else {
            editor_left + self.padding_left()
        };

        Some(InlineLineDisplayInfo {
            buffer_to_display,
            font_size,
            line_height,
            wrapped_line,
            indent_px,
            content_x,
        })
    }

    /// Compute the buffer offset for a mouse position within a table cell.
    fn table_cell_offset_for_position(
        &self,
        window: &mut Window,
        table_start: usize,
        visual_row: usize,
        col: usize,
        bounds: &Bounds<Pixels>,
        position: Point<Pixels>,
    ) -> Option<usize> {
        let source_line = self.table_source_line_for_visual_row(table_start, visual_row)?;
        let line_start = self.buffer.line_start(source_line);
        let padding_x = self.scaled(self.styles.table.table_cell_padding_px);
        let padding_y = self.scaled(self.styles.table.table_cell_padding_px) * 0.6;
        let inner_width =
            (f32::from(bounds.size.width) - padding_x * 2.0).max(self.base_font_size() * 0.5);
        let info = self.table_cell_display_info(window, source_line, col, inner_width)?;
        let line_height =
            self.base_font_size() * self.styles.editor.editor_paragraph_line_height_factor;
        let local_x = (f32::from(position.x) - f32::from(bounds.origin.x) - padding_x).max(0.0);
        let local_y = (f32::from(position.y) - f32::from(bounds.origin.y) - padding_y).max(0.0);
        let display_index = match info
            .wrapped
            .closest_index_for_position(point(px(local_x), px(local_y)), px(line_height))
        {
            Ok(index) | Err(index) => index,
        };
        let mut display_index = display_index.min(info.display_text.len());
        while display_index > 0 && !info.display_text.is_char_boundary(display_index) {
            display_index -= 1;
        }
        let buffer_col = if display_index < info.display_to_buffer.len() {
            info.display_to_buffer[display_index]
        } else {
            info.cell_content_len
        };
        Some(line_start + info.cell_range.start + buffer_col)
    }

    pub(crate) fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_image() {
            let delta_y: f32 = event.delta.pixel_delta(px(40.0)).y.into();
            if delta_y != 0.0 {
                let zoom_ratio = 1.0 + (delta_y.abs() / 300.0).clamp(0.05, 0.35);
                if let Some(image_view) = self.active_image_view_state_mut() {
                    if delta_y < 0.0 {
                        image_view.zoom_factor *= zoom_ratio;
                    } else {
                        image_view.zoom_factor /= zoom_ratio;
                    }
                }
                self.clamp_active_image_view();
                cx.notify();
            }
            return;
        }

        if self.context_menu.is_some() {
            return;
        }
        let delta_y: f32 = event
            .delta
            .pixel_delta(px(
                self.base_font_size() * self.styles.editor.editor_paragraph_line_height_factor
            ))
            .y
            .into();
        self.scroll_offset -= delta_y;
        self.clamp_scroll();
        self.toolbar_hidden_by_scroll = self.scroll_offset > 0.5;
        if self.toolbar_hidden_by_scroll {
            self.toolbar_corner_hovered = false;
        }
        cx.notify();
    }

    pub(crate) fn offset_for_position(&self, position: Point<Pixels>) -> usize {
        let pos_y: f32 = position.y.into();
        let pos_x: f32 = position.x.into();
        let origin_y: f32 = self.content_origin.y.into();
        let y = (pos_y - origin_y) + self.scroll_offset;
        let line = self.line_at_y(y);
        let line_content = self.buffer.line_content(line);
        if line_content.is_empty() {
            return self.buffer.line_start(line);
        }

        if let Some(Some(info)) = self.line_display_info.get(line) {
            let line_top_y = self.line_y_offset(line);
            let local_y = (y - line_top_y).max(0.0);
            let visual_line_idx = (local_y / info.line_height) as usize;
            let indent_adjust = if info.indent_px > 0.0 && visual_line_idx > 0 {
                info.indent_px
            } else {
                0.0
            };
            let local_x = (pos_x - info.content_x - indent_adjust).max(0.0);

            if visual_line_idx > 0 && local_x <= 0.0 {
                let display_index =
                    Self::wrap_boundary_display_index_for_visual_line(info, visual_line_idx)
                        .unwrap_or(info.display_text.len());

                let buffer_col = if display_index < info.display_to_buffer.len() {
                    info.display_to_buffer[display_index]
                } else if let Some(&last) = info.display_to_buffer.last() {
                    last + 1
                } else {
                    0
                };
                return self.buffer.line_start(line) + buffer_col;
            }

            if visual_line_idx == 0
                && let Some(decoration) = self.decorations.get(line)
                && matches!(
                    decoration.block_type,
                    BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
                )
            {
                let prefix_buffer_end = self.line_content_start_col(line).min(line_content.len());
                let prefix_collapsed = info.display_text != line_content;
                let collapsed_task_prefix_chars = ((self.base_font_size()
                    * self.styles.checkbox.checkbox_size_factor
                    + self.scaled(self.styles.checkbox.checkbox_text_gap_px))
                    / (self.base_font_size() * 0.6))
                    .ceil()
                    .max(3.0) as usize;
                let prefix_display_bytes = helpers::compute_display_prefix_bytes(
                    &info.display_text,
                    &decoration.block_type,
                    prefix_collapsed,
                    collapsed_task_prefix_chars,
                );

                if prefix_buffer_end > 0 && prefix_display_bytes > 0 {
                    let prefix_end_x = info
                        .wrapped_line
                        .position_for_index(
                            prefix_display_bytes.min(info.display_text.len()),
                            px(info.line_height),
                        )
                        .map(|pos| f32::from(pos.x))
                        .unwrap_or(0.0);

                    if local_x <= prefix_end_x {
                        let nearest_prefix_boundary = (0..=prefix_buffer_end)
                            .min_by(|left, right| {
                                let left_display = helpers::buffer_to_display_offset(
                                    &info.buffer_to_display,
                                    *left,
                                )
                                .min(info.display_text.len());
                                let right_display = helpers::buffer_to_display_offset(
                                    &info.buffer_to_display,
                                    *right,
                                )
                                .min(info.display_text.len());
                                let left_x = info
                                    .wrapped_line
                                    .position_for_index(left_display, px(info.line_height))
                                    .map(|pos| f32::from(pos.x))
                                    .unwrap_or(prefix_end_x);
                                let right_x = info
                                    .wrapped_line
                                    .position_for_index(right_display, px(info.line_height))
                                    .map(|pos| f32::from(pos.x))
                                    .unwrap_or(prefix_end_x);
                                (left_x - local_x)
                                    .abs()
                                    .partial_cmp(&(right_x - local_x).abs())
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            })
                            .unwrap_or(prefix_buffer_end);

                        return self.buffer.line_start(line) + nearest_prefix_boundary;
                    }
                }
            }

            let line_end_position = info
                .wrapped_line
                .position_for_index(info.display_text.len(), px(info.line_height))
                .unwrap_or_default();
            if visual_line_idx + 1 >= info.visual_line_count
                && local_x >= f32::from(line_end_position.x)
            {
                return self.buffer.line_start(line) + line_content.len();
            }
            let local_pos = point(px(local_x), px(local_y));
            let display_index = match info
                .wrapped_line
                .closest_index_for_position(local_pos, px(info.line_height))
            {
                Ok(idx) | Err(idx) => idx,
            };
            let mut dc = display_index.min(info.display_text.len());
            while dc > 0 && !info.display_text.is_char_boundary(dc) {
                dc -= 1;
            }
            let buffer_col = if dc < info.display_to_buffer.len() {
                info.display_to_buffer[dc]
            } else if let Some(&last) = info.display_to_buffer.last() {
                last + 1
            } else {
                0
            };
            return self.buffer.line_start(line) + buffer_col;
        }

        let x = (pos_x - self.last_content_left).max(0.0);
        let approx_char_width = self.base_font_size() * 0.6;
        let col = (x / approx_char_width) as usize;
        self.buffer.line_col_to_offset(line, col)
    }

    pub(crate) fn wrap_boundary_display_index_for_visual_line(
        info: &LineDisplayInfo,
        visual_line_idx: usize,
    ) -> Option<usize> {
        if visual_line_idx == 0 {
            return Some(0);
        }
        helpers::wrap_boundary_byte_indices(&info.wrapped_line)
            .get(visual_line_idx - 1)
            .copied()
    }

    fn wrap_boundary_visual_indices(
        info: &LineDisplayInfo,
        disp_col: usize,
    ) -> Option<(usize, usize)> {
        helpers::wrap_boundary_byte_indices(&info.wrapped_line)
            .iter()
            .position(|boundary| *boundary == disp_col)
            .map(|upstream_visual_idx| (upstream_visual_idx, upstream_visual_idx + 1))
    }

    fn wrap_boundary_visual_x_pair(info: &LineDisplayInfo, disp_col: usize) -> Option<(f32, f32)> {
        let (upstream_visual_idx, downstream_visual_idx) =
            Self::wrap_boundary_visual_indices(info, disp_col)?;
        let upstream_x = info.wrap_boundary_upstream_xs.get(&disp_col).copied()?
            + if upstream_visual_idx > 0 {
                info.indent_px
            } else {
                0.0
            };
        let downstream_x = if downstream_visual_idx > 0 {
            info.indent_px
        } else {
            0.0
        };
        Some((upstream_x, downstream_x))
    }

    pub(crate) fn cursor_visual_position(&self, offset: usize) -> Option<(usize, f32)> {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);

        if let Some((upstream_visual_idx, downstream_visual_idx)) =
            Self::wrap_boundary_visual_indices(info, disp_col)
        {
            if self.wrap_boundary_affinity
                == Some(WrapBoundaryAffinity {
                    offset,
                    kind: WrapBoundaryAffinityKind::Downstream,
                })
            {
                let downstream_x = if downstream_visual_idx > 0 {
                    info.indent_px
                } else {
                    0.0
                };
                return Some((downstream_visual_idx, downstream_x));
            }

            if let Some((upstream_x, _)) = Self::wrap_boundary_visual_x_pair(info, disp_col) {
                return Some((upstream_visual_idx, upstream_x));
            }
        }

        let pos = info
            .wrapped_line
            .position_for_index(disp_col, px(info.line_height))
            .unwrap_or_default();
        Some((
            (f32::from(pos.y) / info.line_height) as usize,
            f32::from(pos.x) + helpers::indent_for_y(info.indent_px, pos.y, info.line_height),
        ))
    }

    pub(crate) fn cursor_visual_x(&self, offset: usize) -> Option<f32> {
        self.cursor_visual_position(offset).map(|(_, x)| x)
    }

    pub(crate) fn is_wrap_boundary_offset(&self, offset: usize) -> bool {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(Some(info)) = self.line_display_info.get(line) else {
            return false;
        };

        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        Self::wrap_boundary_visual_indices(info, disp_col).is_some()
    }

    fn update_wrap_boundary_affinity_for_position(
        &mut self,
        offset: usize,
        position: Point<Pixels>,
    ) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(Some(info)) = self.line_display_info.get(line) else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let Some((upstream_visual_idx, downstream_visual_idx)) =
            Self::wrap_boundary_visual_indices(info, disp_col)
        else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        let pos_y: f32 = position.y.into();
        let origin_y: f32 = self.content_origin.y.into();
        let y = (pos_y - origin_y) + self.scroll_offset;
        let line_top_y = self.line_y_offset(line);
        let local_y = (y - line_top_y).max(0.0);
        let visual_line_idx = (local_y / info.line_height) as usize;

        if visual_line_idx >= downstream_visual_idx {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Downstream);
        } else if visual_line_idx <= upstream_visual_idx {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Upstream);
        } else {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Downstream);
        }
    }

    pub(crate) fn update_wrap_boundary_affinity_for_goal_x(&mut self, offset: usize, goal_x: f32) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(Some(info)) = self.line_display_info.get(line) else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let Some((upstream_x, downstream_x)) = Self::wrap_boundary_visual_x_pair(info, disp_col)
        else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        if (goal_x - downstream_x).abs() <= (goal_x - upstream_x).abs() {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Downstream);
        } else {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Upstream);
        }
    }

    // ── UTF-16 helpers ──────────────────────────────────────────────

    pub(crate) fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.buffer.text().chars() {
            if utf16_count >= utf16_offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    pub(crate) fn offset_to_utf16(&self, utf8_offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.buffer.text().chars() {
            if utf8_count >= utf8_offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn utf8_offset_in_text_for_utf16(text: &str, utf16_offset: usize) -> usize {
        let mut utf8_offset = 0usize;
        let mut utf16_count = 0usize;

        for ch in text.chars() {
            if utf16_count >= utf16_offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset.min(text.len())
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if !self.active_tab_supports_text_buffer() {
            actual_range.take();
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        let clamped_end = range.end.min(self.buffer.len());
        let clamped_start = range.start.min(clamped_end);
        let text = &self.buffer.text()[clamped_start..clamped_end];
        actual_range.replace(self.range_to_utf16(&(clamped_start..clamped_end)));
        Some(text.to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if !self.active_tab_supports_text_buffer() {
            return None;
        }
        if self.selected_handle.is_some() {
            ime_debug_log("selected_text_range -> None (table handle selected)");
            return None;
        }
        let selection = UTF16Selection {
            range: self.range_to_utf16(&self.cursor.selection_range()),
            reversed: self.cursor.offset < self.cursor.anchor,
        };
        ime_debug_log(format!(
            "selected_text_range -> {:?}, cursor=({},{}) marked={:?}",
            selection.range, self.cursor.anchor, self.cursor.offset, self.marked_range
        ));
        Some(selection)
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if !self.active_tab_supports_text_buffer() {
            return None;
        }
        if self.selected_handle.is_some() {
            ime_debug_log("marked_text_range -> None (table handle selected)");
            return None;
        }
        let marked = self.marked_range.as_ref().map(|r| self.range_to_utf16(r));
        ime_debug_log(format!(
            "marked_text_range -> {:?} (utf8={:?})",
            marked, self.marked_range
        ));
        marked
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        if !self.active_tab_supports_text_buffer() {
            return;
        }
        self.clear_marked_ranges();
        self.finish_ime_composition();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_supports_text_buffer() {
            return;
        }
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        if !self.cursor.has_selection()
            && self.marked_range.is_none()
            && range_utf16.is_none()
            && !self.has_extra_cursors()
            && self.should_skip_auto_closing_delimiter(new_text)
        {
            self.cursor
                .move_to(self.cursor.offset + new_text.len(), &self.buffer);
            self.pause_blinking(cx);
            if self.wiki_link_start.is_some() {
                self.sync_wiki_link_menu(cx);
            }
            return;
        }
        ime_debug_log(format!(
            "replace_text_in_range start range_utf16={:?} new_text={:?} marked_before={:?} selection_before={:?}",
            range_utf16,
            new_text,
            self.marked_range,
            self.cursor.selection_range()
        ));
        let had_ime_composition = self.ime_composition_snapshot.is_some();
        if !had_ime_composition {
            self.checkpoint();
        }

        if self.has_extra_cursors()
            && (had_ime_composition
                || range_utf16.is_some()
                || self.marked_range.is_some()
                || !self.extra_marked_ranges.is_empty())
        {
            let range_spans_multiple_lines = self.apply_multi_cursor_ime_replace(
                range_utf16
                    .as_ref()
                    .map(|range_utf16| self.range_from_utf16(range_utf16)),
                new_text,
                None,
                false,
            );
            self.reparse();
            if !cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n')
            {
                self.scroll_to_cursor();
            }
            self.pause_blinking(cx);
            if had_ime_composition {
                self.finish_ime_composition();
            }
            ime_debug_log(format!(
                "replace_text_in_range multi-cursor end cursor={} marked_after={:?} extra_marked_count={}",
                self.cursor.offset,
                self.marked_range,
                self.extra_marked_ranges.len()
            ));
            return;
        }

        // When text is selected and user types a formatting character, wrap selection
        if self.cursor.has_selection() && self.marked_range.is_none() && range_utf16.is_none() {
            if let Some((prefix, suffix)) = paired_delimiter(new_text) {
                self.apply_wrap(prefix, suffix);
                self.pause_blinking(cx);
                return;
            }
            let wrap = match new_text {
                "*" => Some(("**", "**")),
                "~" => Some(("~~", "~~")),
                "`" => Some(("`", "`")),
                _ => None,
            };
            if let Some((prefix, suffix)) = wrap {
                self.apply_wrap(prefix, suffix);
                self.pause_blinking(cx);
                return;
            }
        }

        // Multi-cursor text input
        if self.has_extra_cursors() && self.marked_range.is_none() && range_utf16.is_none() {
            self.multi_cursor_insert(new_text);
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());
        ime_debug_log(format!(
            "replace_text_in_range resolved range_utf8={:?}",
            range
        ));
        let (range_start_line, _) = self.buffer.offset_to_line_col(range.start);
        let (range_end_line, _) = self.buffer.offset_to_line_col(range.end);
        let range_spans_multiple_lines = range_start_line != range_end_line;
        let touched_ordered_prefix = self.ordered_list_prefix_touched_by_range(&range);

        if range.is_empty() && self.marked_range.is_none() && range_utf16.is_none() {
            if let Some(code_block_range) =
                code_block_autocomplete_range(self.buffer.text(), range.start, new_text)
            {
                let snippet = "```\n\n```";
                let cursor_offset = code_block_range.start + "```\n".len();
                self.buffer.replace(code_block_range, snippet);
                self.cursor.move_to(cursor_offset, &self.buffer);
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }

            if new_text != "["
                && let Some((open, close)) = paired_delimiter(new_text)
            {
                let mut paired_text = String::with_capacity(open.len() + close.len());
                paired_text.push_str(open);
                paired_text.push_str(close);
                self.buffer.replace(range.clone(), &paired_text);
                self.cursor.move_to(range.start + open.len(), &self.buffer);
                self.reparse();
                if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
                    self.scroll_to_cursor();
                }
                self.pause_blinking(cx);
                return;
            }
        }

        self.buffer.replace(range.clone(), new_text);
        let new_offset = range.start + new_text.len();
        self.cursor.move_to(new_offset, &self.buffer);
        self.clear_marked_ranges();
        self.reparse();
        if let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        }
        if !cursor_had_inline_headroom
            || self.cursor_line() != cursor_line_before
            || range_spans_multiple_lines
            || new_text.contains('\n')
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        if had_ime_composition {
            self.finish_ime_composition();
        }
        ime_debug_log(format!(
            "replace_text_in_range end cursor={} marked_after={:?}",
            self.cursor.offset, self.marked_range
        ));

        if self.slash_menu_start.is_some() {
            self.update_slash_filter(cx);
        } else if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        } else if new_text == "[" && new_offset >= 2 {
            let text = self.buffer.text();
            if text.get(new_offset - 2..new_offset) == Some("[[") {
                if text.get(new_offset..new_offset + 2) != Some("]]") {
                    self.buffer.replace(new_offset..new_offset, "]]");
                    self.reparse();
                }
                self.cursor.move_to(new_offset, &self.buffer);
                self.open_wiki_link_menu(new_offset - 2, cx);
            }
        } else if new_text == "/" {
            let (line, col) = self.buffer.offset_to_line_col(new_offset);
            let line_content = self.buffer.line_content(line);
            let trimmed = line_content.trim();
            if trimmed == "/" && col == line_content.len() {
                self.open_slash_menu(cx);
            }
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_supports_text_buffer() {
            return;
        }
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        ime_debug_log(format!(
            "replace_and_mark_text_in_range start range_utf16={:?} new_text={:?} new_selected_range_utf16={:?} marked_before={:?} selection_before={:?}",
            range_utf16,
            new_text,
            new_selected_range_utf16,
            self.marked_range,
            self.cursor.selection_range()
        ));
        self.begin_ime_composition();

        if self.has_extra_cursors() {
            let range_spans_multiple_lines = self.apply_multi_cursor_ime_replace(
                range_utf16
                    .as_ref()
                    .map(|range_utf16| self.range_from_utf16(range_utf16)),
                new_text,
                new_selected_range_utf16.as_ref(),
                true,
            );
            self.reparse();
            if !cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n')
            {
                self.scroll_to_cursor();
            }
            self.pause_blinking(cx);
            ime_debug_log(format!(
                "replace_and_mark_text_in_range multi-cursor end selection=({},{}) marked_after={:?} extra_marked_count={}",
                self.cursor.anchor,
                self.cursor.offset,
                self.marked_range,
                self.extra_marked_ranges.len()
            ));
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());
        ime_debug_log(format!(
            "replace_and_mark_text_in_range resolved range_utf8={:?}",
            range
        ));
        let (range_start_line, _) = self.buffer.offset_to_line_col(range.start);
        let (range_end_line, _) = self.buffer.offset_to_line_col(range.end);
        let range_spans_multiple_lines = range_start_line != range_end_line;
        let touched_ordered_prefix = self.ordered_list_prefix_touched_by_range(&range);

        self.buffer.replace(range.clone(), new_text);

        let mark_start = range.start;
        let mark_end = range.start + new_text.len();
        self.extra_marked_ranges.clear();
        if !new_text.is_empty() {
            self.marked_range = Some(mark_start..mark_end);
        } else {
            self.marked_range = None;
        }

        let (selection_start, selection_end) = new_selected_range_utf16
            .as_ref()
            .map(|r| {
                let start = mark_start + Self::utf8_offset_in_text_for_utf16(new_text, r.start);
                let end = mark_start + Self::utf8_offset_in_text_for_utf16(new_text, r.end);
                (start, end)
            })
            .unwrap_or((mark_end, mark_end));
        self.cursor.anchor = self.buffer.clamp_offset(selection_start);
        self.cursor.offset = self.buffer.clamp_offset(selection_end);
        self.reparse();
        if let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        }
        if !cursor_had_inline_headroom
            || self.cursor_line() != cursor_line_before
            || range_spans_multiple_lines
            || new_text.contains('\n')
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        ime_debug_log(format!(
            "replace_and_mark_text_in_range end selection=({},{}) marked_after={:?}",
            self.cursor.anchor, self.cursor.offset, self.marked_range
        ));
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if !self.active_tab_supports_text_buffer() {
            return None;
        }
        ime_debug_log(format!(
            "bounds_for_range start range_utf16={:?}",
            range_utf16
        ));
        let queried_range = self.range_from_utf16(&range_utf16);
        let anchor_offset = if self.marked_range.as_ref().is_some_and(|marked| {
            queried_range.start >= marked.start && queried_range.end <= marked.end
        }) {
            self.buffer.clamp_offset(self.cursor.offset)
        } else {
            queried_range.start
        };
        let (line, col) = self.buffer.offset_to_line_col(anchor_offset);
        let origin_y: f32 = bounds.origin.y.into();
        let line_top_y = origin_y + self.line_y_offset(line) - self.scroll_offset;

        if let Some(table_info) = self
            .decorations
            .get(line)
            .and_then(|dec| dec.table_info.as_ref())
        {
            if !table_info.is_separator {
                let table_start_line = table_info.table_start_line;
                let visual_row = self.table_visual_row_for_source_line(table_start_line, line)?;
                let cell_col = table_info
                    .cell_ranges
                    .iter()
                    .position(|cell_range| col >= cell_range.start && col <= cell_range.end)
                    .or_else(|| {
                        table_info
                            .cell_ranges
                            .iter()
                            .enumerate()
                            .find(|(_, cell_range)| col == cell_range.end)
                            .map(|(index, _)| index)
                    })?;
                let cell_bounds = self
                    .table_click_areas
                    .iter()
                    .find(|(_, tsl, vr, c)| {
                        *tsl == table_start_line && *vr == visual_row && *c == cell_col
                    })
                    .map(|(bounds, _, _, _)| *bounds)?;

                let padding_x = self.scaled(self.styles.table.table_cell_padding_px);
                let padding_y = self.scaled(self.styles.table.table_cell_padding_px) * 0.6;
                let inner_width = (f32::from(cell_bounds.size.width) - padding_x * 2.0)
                    .max(self.base_font_size() * 0.5);
                let info = self.table_cell_display_info(window, line, cell_col, inner_width)?;
                let cell_offset = anchor_offset
                    .saturating_sub(self.buffer.line_start(line) + info.cell_range.start);
                let disp_col = if cell_offset >= info.buffer_to_display.len() {
                    info.display_text.len()
                } else {
                    helpers::buffer_to_display_offset(&info.buffer_to_display, cell_offset)
                };
                let pos = info
                    .wrapped
                    .position_for_index(
                        disp_col.min(info.display_text.len()),
                        px(self.base_font_size()
                            * self.styles.editor.editor_paragraph_line_height_factor),
                    )
                    .unwrap_or_default();
                let line_height =
                    self.base_font_size() * self.styles.editor.editor_paragraph_line_height_factor;
                let x = f32::from(cell_bounds.origin.x) + padding_x + f32::from(pos.x);
                let y = f32::from(cell_bounds.origin.y) + padding_y + f32::from(pos.y);
                ime_debug_log(format!(
                    "bounds_for_range table line={} col={} cell_col={} visual_row={} queried_utf8={:?} anchor_utf8={} -> x={} y={} h={}",
                    line,
                    col,
                    cell_col,
                    visual_row,
                    queried_range,
                    anchor_offset,
                    x,
                    y,
                    line_height
                ));
                return Some(Bounds::new(
                    point(px(x), px(y)),
                    size(px(self.base_font_size() * 0.6), px(line_height)),
                ));
            }
        }

        let line_info = if self.layout_dirty {
            self.inline_line_display_info(window, line, bounds)
                .or_else(|| {
                    self.line_display_info
                        .get(line)
                        .cloned()
                        .flatten()
                        .map(|info| InlineLineDisplayInfo {
                            buffer_to_display: info.buffer_to_display,
                            font_size: info.font_size,
                            line_height: info.line_height,
                            wrapped_line: info.wrapped_line,
                            indent_px: info.indent_px,
                            content_x: info.content_x,
                        })
                })
        } else {
            self.line_display_info
                .get(line)
                .cloned()
                .flatten()
                .map(|info| InlineLineDisplayInfo {
                    buffer_to_display: info.buffer_to_display,
                    font_size: info.font_size,
                    line_height: info.line_height,
                    wrapped_line: info.wrapped_line,
                    indent_px: info.indent_px,
                    content_x: info.content_x,
                })
                .or_else(|| self.inline_line_display_info(window, line, bounds))
        };

        if let Some(info) = line_info {
            let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
            let pos = info
                .wrapped_line
                .position_for_index(disp_col, px(info.line_height))
                .unwrap_or_default();
            let indent_adjust = helpers::indent_for_y(info.indent_px, pos.y, info.line_height);
            let x = info.content_x + indent_adjust + f32::from(pos.x);
            let y = line_top_y + f32::from(pos.y);
            ime_debug_log(format!(
                "bounds_for_range normal line={} col={} queried_utf8={:?} anchor_utf8={} -> x={} y={} h={}",
                line, col, queried_range, anchor_offset, x, y, info.line_height
            ));
            Some(Bounds::new(
                point(px(x), px(y)),
                size(px(info.font_size * 0.6), px(info.line_height)),
            ))
        } else {
            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            let lh = self.line_height_for_block(block_type);
            let x = self.last_content_left + col as f32 * self.base_font_size() * 0.6;
            Some(Bounds::new(
                point(px(x), px(line_top_y)),
                size(px(self.base_font_size() * 0.6), px(lh)),
            ))
        }
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let utf8_offset = self.offset_for_position(point);
        let utf16_offset = self.offset_to_utf16(utf8_offset);
        ime_debug_log(format!(
            "character_index_for_point point=({}, {}) -> utf8={} utf16={}",
            f32::from(point.x),
            f32::from(point.y),
            utf8_offset,
            utf16_offset
        ));
        Some(utf16_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::code_block_autocomplete_range;

    #[test]
    fn third_backtick_triggers_code_block_autocomplete() {
        assert_eq!(code_block_autocomplete_range("``", 2, "`"), Some(0..2));
    }

    #[test]
    fn fourth_backtick_does_not_retrigger_code_block_autocomplete() {
        assert_eq!(code_block_autocomplete_range("```", 3, "`"), None);
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
