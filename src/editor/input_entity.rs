use std::{collections::HashMap, ops::Range};

use gpui::{
    App, Bounds, Context, EntityInputHandler, FocusHandle, Focusable, Pixels, Point,
    UTF16Selection, Window, WrappedLine, point, px, size,
};

use crate::markdown::{BlockType, TableLineInfo};

use super::input::{InlineLineDisplayInfo, ime_debug_enabled, ime_debug_log};
use super::*;

fn uses_table_ime_geometry(active_tab_is_diff: bool, table_info: &TableLineInfo) -> bool {
    !active_tab_is_diff && !table_info.is_separator && !table_info.is_front_matter
}

#[cfg(test)]
#[test]
fn front_matter_uses_inline_ime_geometry() {
    let front_matter = crate::markdown::parse_decorations("---\ntitle: Moeka\n---");
    let front_matter_info = front_matter[1].table_info.as_ref().unwrap();
    let table = crate::markdown::parse_decorations("| Key | Value |\n| --- | --- |\n| A | B |");
    let table_info = table[0].table_info.as_ref().unwrap();

    assert!(!uses_table_ime_geometry(false, front_matter_info));
    assert!(uses_table_ime_geometry(false, table_info));
    assert!(!uses_table_ime_geometry(true, table_info));
}

impl EditorView {
    fn input_bounds_for_display_line(
        &self,
        anchor_offset: usize,
        line: usize,
        col: usize,
        text_top_y: f32,
        display_text: &str,
        buffer_to_display: &[usize],
        wrapped_line: &WrappedLine,
        font_size: f32,
        line_height: f32,
        indent_px: f32,
        wrap_boundary_upstream_xs: &HashMap<usize, f32>,
        content_x: f32,
    ) -> Bounds<Pixels> {
        let disp_col = helpers::buffer_to_display_offset(buffer_to_display, col);
        let affinity = self
            .wrap_boundary_affinity
            .and_then(|current| (current.offset == anchor_offset).then_some(current.kind));
        let pos = Self::visual_position_for_wrapped_display(
            wrapped_line,
            display_text,
            disp_col,
            line_height,
            indent_px,
            wrap_boundary_upstream_xs,
            affinity,
        )
        .unwrap_or_default();
        let x = content_x + f32::from(pos.x);
        let y = text_top_y + f32::from(pos.y);
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "bounds_for_range normal line={} col={} anchor_utf8={} -> x={} y={} h={}",
                line, col, anchor_offset, x, y, line_height
            ));
        }
        Bounds::new(
            point(px(x), px(y)),
            size(px(font_size * 0.6), px(line_height)),
        )
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
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "selected_text_range -> {:?}, cursor=({},{}) marked={:?}",
                selection.range, self.cursor.anchor, self.cursor.offset, self.marked_range
            ));
        }
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
        let marked = self
            .marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range));
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "marked_text_range -> {:?} (utf8={:?})",
                marked, self.marked_range
            ));
        }
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
        if !self.active_tab_supports_text_buffer() || self.active_tab_is_read_only() {
            return;
        }
        self.last_scroll_cursor_visual_x = None;
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        // Typing into the buffer dismisses any read-only phantom-row selection.
        self.clear_phantom_selection();
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
        self.capture_typing_scroll_guard();
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_text_in_range start range_utf16={:?} new_text={:?} marked_before={:?} selection_before={:?}",
                range_utf16,
                new_text,
                self.marked_range,
                self.cursor.selection_range()
            ));
        }
        let had_ime_composition = self.ime_composition_snapshot.is_some();
        if !had_ime_composition {
            self.checkpoint_for_typing(new_text);
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
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "replace_text_in_range multi-cursor end cursor={} marked_after={:?} extra_marked_count={}",
                    self.cursor.offset,
                    self.marked_range,
                    self.extra_marked_ranges.len()
                ));
            }
            return;
        }

        if self.cursor.has_selection() && self.marked_range.is_none() && range_utf16.is_none() {
            if let Some((prefix, suffix)) = input_ime::paired_delimiter(new_text) {
                self.apply_wrap(prefix, suffix);
                self.pause_blinking(cx);
                return;
            }
            let in_code = self.offset_in_code_context(self.cursor.selection_range().start);
            let wrap = match new_text {
                "*" if !in_code => Some(("**", "**")),
                "~" if !in_code => Some(("~~", "~~")),
                "+" if !in_code => Some(("++", "++")),
                "`" => Some(("`", "`")),
                _ => None,
            };
            if let Some((prefix, suffix)) = wrap {
                self.apply_wrap(prefix, suffix);
                self.pause_blinking(cx);
                return;
            }
        }

        if self.has_extra_cursors() && self.marked_range.is_none() && range_utf16.is_none() {
            self.multi_cursor_insert(new_text);
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_text_in_range resolved range_utf8={:?}",
                range
            ));
        }
        let preserve_preview_scroll = self
            .decorations
            .get(cursor_line_before)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .is_some_and(|preview| {
                preview.source_range.start <= self.cursor.offset
                    && self.cursor.offset <= preview.source_range.end
            });
        if preserve_preview_scroll {
            self.pending_preview_reveal = Some(PendingPreviewReveal {
                scroll_offset: self.scroll_offset,
            });
        }
        let (range_start_line, _) = self.buffer.offset_to_line_col(range.start);
        let (range_end_line, _) = self.buffer.offset_to_line_col(range.end);
        let range_spans_multiple_lines = range_start_line != range_end_line;
        let touched_ordered_prefix = self.ordered_list_prefix_touched_by_range(&range);
        let markdown_syntax_edit = self.active_tab_is_markdown()
            && (new_text.contains(['=', '*', '_', '~', '`', '+', '[', ']', '(', ')'])
                || self
                    .buffer
                    .text()
                    .get(range.clone())
                    .is_some_and(|text| text.contains(['=', '*', '_', '~', '`', '+'])));

        if range.is_empty() && self.marked_range.is_none() && range_utf16.is_none() {
            if let Some(code_block_range) =
                input_ime::code_block_autocomplete_range(self.buffer.text(), range.start, new_text)
            {
                let snippet = "```\n\n```";
                let cursor_offset = code_block_range.start + "```\n".len();
                self.queue_non_markdown_line_display_transfer(&code_block_range, snippet);
                self.buffer.replace(code_block_range, snippet);
                self.cursor.move_to(cursor_offset, &self.buffer);
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }

            if !(matches!(new_text, "*" | "_" | "~" | "+" | "=")
                && self.offset_in_code_context(range.start))
                && let Some((open, close)) =
                    input_ime::paired_delimiter_at(self.buffer.text(), range.start, new_text)
            {
                let mut paired_text = String::with_capacity(open.len() + close.len());
                paired_text.push_str(open);
                paired_text.push_str(close);
                self.queue_non_markdown_line_display_transfer(&range, &paired_text);
                self.buffer.replace(range.clone(), &paired_text);
                self.cursor.move_to(range.start + open.len(), &self.buffer);
                if markdown_syntax_edit {
                    self.invalidate_line_display_info_for_line(range_start_line);
                }
                self.reparse();
                if !preserve_preview_scroll
                    && (!cursor_had_inline_headroom || self.cursor_line() != cursor_line_before)
                {
                    self.scroll_to_cursor();
                }
                self.pause_blinking(cx);
                return;
            }
        }

        self.queue_non_markdown_line_display_transfer(&range, new_text);
        self.buffer.replace(range.clone(), new_text);
        let new_offset = range.start + new_text.len();
        self.cursor.move_to(new_offset, &self.buffer);
        self.clear_marked_ranges();
        if markdown_syntax_edit {
            self.invalidate_line_display_info_for_line(range_start_line);
        }
        self.reparse();
        if let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        }
        if !preserve_preview_scroll
            && (!cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n'))
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        if had_ime_composition {
            self.finish_ime_composition();
        }
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_text_in_range end cursor={} marked_after={:?}",
                self.cursor.offset, self.marked_range
            ));
        }

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
                self.ensure_wiki_link_menu_for_cursor(cx);
            }
        } else if self.maybe_open_wiki_link_menu_after_edit(cx) {
        } else if new_text == "/" && self.active_tab_is_markdown() && !self.embedded_editor {
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
        if !self.active_tab_supports_text_buffer() || self.active_tab_is_read_only() {
            return;
        }
        self.last_scroll_cursor_visual_x = None;
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        self.capture_typing_scroll_guard();
        self.clear_phantom_selection();
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_and_mark_text_in_range start range_utf16={:?} new_text={:?} new_selected_range_utf16={:?} marked_before={:?} selection_before={:?}",
                range_utf16,
                new_text,
                new_selected_range_utf16,
                self.marked_range,
                self.cursor.selection_range()
            ));
        }
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
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "replace_and_mark_text_in_range multi-cursor end selection=({},{}) marked_after={:?} extra_marked_count={}",
                    self.cursor.anchor,
                    self.cursor.offset,
                    self.marked_range,
                    self.extra_marked_ranges.len()
                ));
            }
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_and_mark_text_in_range resolved range_utf8={:?}",
                range
            ));
        }
        let preserve_preview_scroll = self
            .decorations
            .get(cursor_line_before)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .is_some_and(|preview| {
                preview.source_range.start <= self.cursor.offset
                    && self.cursor.offset <= preview.source_range.end
            });
        if preserve_preview_scroll {
            self.pending_preview_reveal = Some(PendingPreviewReveal {
                scroll_offset: self.scroll_offset,
            });
        }
        let (range_start_line, _) = self.buffer.offset_to_line_col(range.start);
        let (range_end_line, _) = self.buffer.offset_to_line_col(range.end);
        let range_spans_multiple_lines = range_start_line != range_end_line;
        let touched_ordered_prefix = self.ordered_list_prefix_touched_by_range(&range);

        self.queue_non_markdown_line_display_transfer(&range, new_text);
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
            .map(|range| {
                let start = mark_start + Self::utf8_offset_in_text_for_utf16(new_text, range.start);
                let end = mark_start + Self::utf8_offset_in_text_for_utf16(new_text, range.end);
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
        if !preserve_preview_scroll
            && (!cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n'))
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_and_mark_text_in_range end selection=({},{}) marked_after={:?}",
                self.cursor.anchor, self.cursor.offset, self.marked_range
            ));
        }
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
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "bounds_for_range start range_utf16={:?}",
                range_utf16
            ));
        }
        let queried_range = self.range_from_utf16(&range_utf16);
        let anchor_offset = if self.marked_range.as_ref().is_some_and(|marked| {
            queried_range.start >= marked.start && queried_range.end <= marked.end
        }) {
            self.buffer.clamp_offset(self.cursor.offset)
        } else {
            queried_range.start
        };
        let editor_bounds = if self.last_viewport_width > 0.0 && self.last_viewport_height > 0.0 {
            Bounds::new(
                self.content_origin,
                size(px(self.last_viewport_width), px(self.last_viewport_height)),
            )
        } else {
            bounds
        };
        if self.layout_dirty
            && let Some(bounds) = self.last_stable_input_bounds
        {
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "bounds_for_range using cached stable bounds during layout_dirty anchor_utf8={} -> x={} y={} w={} h={}",
                    anchor_offset,
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height)
                ));
            }
            return Some(bounds);
        }
        if self.layout_dirty
            && let Some(bounds) = self.pending_input_bounds
        {
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "bounds_for_range using pending input bounds during layout_dirty anchor_utf8={} -> x={} y={} w={} h={}",
                    anchor_offset,
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height)
                ));
            }
            return Some(bounds);
        }
        let (line, col) = self.buffer.offset_to_line_col(anchor_offset);
        let origin_y: f32 = editor_bounds.origin.y.into();
        let text_top_y = origin_y + self.text_y_offset_for_line(line) - self.scroll_offset;

        if let Some(table_info) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
            && uses_table_ime_geometry(self.active_tab_is_diff(), table_info)
        {
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
            let cell_offset =
                anchor_offset.saturating_sub(self.buffer.line_start(line) + info.cell_range.start);
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
            if ime_debug_enabled() {
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
            }
            let bounds = Bounds::new(
                point(px(x), px(y)),
                size(px(self.base_font_size() * 0.6), px(line_height)),
            );
            self.pending_input_bounds = None;
            self.last_stable_input_bounds = Some(bounds);
            return Some(bounds);
        }

        let cached_line_bounds = |this: &Self| {
            this.line_display_info
                .get(line)
                .and_then(|info| info.as_deref())
                .map(|info| {
                    this.input_bounds_for_display_line(
                        anchor_offset,
                        line,
                        col,
                        text_top_y,
                        &info.display_text,
                        &info.buffer_to_display,
                        &info.wrapped_line,
                        info.font_size,
                        info.line_height,
                        info.indent_px,
                        &info.wrap_boundary_upstream_xs,
                        info.content_x,
                    )
                })
        };
        let inline_line_bounds = |this: &Self, window: &mut Window| -> Option<Bounds<Pixels>> {
            this.inline_line_display_info(window, line, editor_bounds)
                .map(|info: InlineLineDisplayInfo| {
                    this.input_bounds_for_display_line(
                        anchor_offset,
                        line,
                        col,
                        text_top_y,
                        &info.display_text,
                        &info.buffer_to_display,
                        &info.wrapped_line,
                        info.font_size,
                        info.line_height,
                        info.indent_px,
                        &info.wrap_boundary_upstream_xs,
                        info.content_x,
                    )
                })
        };
        let line_bounds = if self.layout_dirty {
            inline_line_bounds(self, window).or_else(|| cached_line_bounds(self))
        } else {
            cached_line_bounds(self).or_else(|| inline_line_bounds(self, window))
        };

        if let Some(bounds) = line_bounds {
            self.pending_input_bounds = None;
            self.last_stable_input_bounds = Some(bounds);
            Some(bounds)
        } else {
            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            let line_height = self.line_height_for_block(block_type);
            let x = self.last_content_left + col as f32 * self.base_font_size() * 0.6;
            let bounds = Bounds::new(
                point(px(x), px(text_top_y)),
                size(px(self.base_font_size() * 0.6), px(line_height)),
            );
            self.pending_input_bounds = None;
            self.last_stable_input_bounds = Some(bounds);
            Some(bounds)
        }
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if let Some((target_point, target_offset)) = self.last_mouse_down_target.take() {
            let dx = (f32::from(target_point.x) - f32::from(point.x)).abs();
            let dy = (f32::from(target_point.y) - f32::from(point.y)).abs();
            let utf16_offset = self.offset_to_utf16(target_offset);
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "character_index_for_point consumed_latch point=({}, {}) target=({}, {}) dx={} dy={} -> utf8={} utf16={}",
                    f32::from(point.x),
                    f32::from(point.y),
                    f32::from(target_point.x),
                    f32::from(target_point.y),
                    dx,
                    dy,
                    target_offset,
                    utf16_offset
                ));
            }
            return Some(utf16_offset);
        }

        let utf8_offset = self.offset_for_position(point);
        let utf16_offset = self.offset_to_utf16(utf8_offset);
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "character_index_for_point point=({}, {}) -> utf8={} utf16={}",
                f32::from(point.x),
                f32::from(point.y),
                utf8_offset,
                utf16_offset
            ));
        }
        Some(utf16_offset)
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
