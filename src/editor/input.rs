use std::ops::Range;

use gpui::{
    App, Bounds, Context, EntityInputHandler, Focusable, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, UTF16Selection,
    Window, point, px, size,
};

use crate::markdown::{BlockType, InlineKind};

use super::*;

impl EditorView {
    // ── Mouse handlers ──────────────────────────────────────────────

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu.is_some() {
            return;
        }
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

        if let Some(line_idx) = self
            .image_click_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
        {
            if let Some(dec) = self.decorations.get(line_idx) {
                if let Some(span) = dec
                    .inline_spans
                    .iter()
                    .find(|s| s.kind == InlineKind::Image)
                {
                    let line_start = self.buffer.line_start(line_idx);
                    let line_content = self.buffer.line_content(line_idx);
                    let full_start = span.range.start.saturating_sub(2);
                    let full_end = dec
                        .syntax_ranges
                        .iter()
                        .filter(|sr| sr.range.start >= span.range.end)
                        .map(|sr| sr.range.end)
                        .max()
                        .unwrap_or(span.range.end)
                        .min(line_content.len());
                    self.cursor.offset = line_start + full_start;
                    self.cursor.anchor = line_start + full_end;
                    cx.notify();
                    return;
                }
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

        if event.button == MouseButton::Left && event.click_count >= 2 {
            self.clear_extra_cursors();
            let offset = self.offset_for_position(event.position);
            self.select_word_at_offset(offset);
            self.is_selecting = false;
            cx.notify();
            return;
        }

        self.is_selecting = true;
        let prev_offset = self.cursor.offset;
        let offset = self.offset_for_position(event.position);
        if event.modifiers.platform {
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
            self.maybe_auto_select_link_url(prev_offset, true);
        }
        cx.notify();
    }

    pub(crate) fn maybe_auto_select_link_url(
        &mut self,
        prev_offset: usize,
        also_select_desc: bool,
    ) {
        let new_offset = self.cursor.offset;
        let (line, _) = self.buffer.offset_to_line_col(new_offset);
        let line_start = self.buffer.line_start(line);
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
                        break 'link_check;
                    }
                }
            }
        }
    }

    pub(crate) fn on_editor_context_menu(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu.is_some() {
            return;
        }
        self.is_selecting = false;
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
        _: &MouseUpEvent,
        _: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.is_selecting = false;
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu.is_some() {
            return;
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

            let offset = self.offset_for_position(event.position);
            self.cursor.select_to(offset, &self.buffer);
            cx.notify();
        }
    }

    pub(crate) fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu.is_some() {
            return;
        }
        let delta_y: f32 = event
            .delta
            .pixel_delta(px(
                self.base_font_size() * self.styles.editor.editor_paragraph_line_height_factor,
            ))
            .y
            .into();
        self.scroll_offset -= delta_y;
        self.clamp_scroll();
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
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
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
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.cursor.selection_range()),
            reversed: self.cursor.offset < self.cursor.anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();

        // Multi-cursor text input
        if self.has_extra_cursors() && self.marked_range.is_none() && range_utf16.is_none() {
            self.multi_cursor_insert(new_text);
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        let range = if let Some(marked) = self.marked_range.take() {
            marked
        } else {
            range_utf16
                .as_ref()
                .map(|r| self.range_from_utf16(r))
                .unwrap_or_else(|| self.cursor.selection_range())
        };

        self.buffer.replace(range.clone(), new_text);
        let new_offset = range.start + new_text.len();
        self.cursor.move_to(new_offset, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);

        if self.slash_menu_start.is_some() {
            self.update_slash_filter(cx);
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
        self.checkpoint();
        let range = if let Some(marked) = self.marked_range.take() {
            marked
        } else {
            range_utf16
                .as_ref()
                .map(|r| self.range_from_utf16(r))
                .unwrap_or_else(|| self.cursor.selection_range())
        };

        self.buffer.replace(range.clone(), new_text);

        let mark_start = range.start;
        let mark_end = range.start + new_text.len();
        if !new_text.is_empty() {
            self.marked_range = Some(mark_start..mark_end);
        }

        let new_offset = new_selected_range_utf16
            .as_ref()
            .map(|r| {
                let mut utf8_pos = 0usize;
                let mut utf16_count = 0usize;
                for ch in new_text.chars() {
                    if utf16_count >= r.end {
                        break;
                    }
                    utf16_count += ch.len_utf16();
                    utf8_pos += ch.len_utf8();
                }
                mark_start + utf8_pos.min(new_text.len())
            })
            .unwrap_or(mark_end);
        self.cursor.move_to(new_offset, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let (line, col) = self.buffer.offset_to_line_col(range.start);
        let origin_y: f32 = self.content_origin.y.into();
        let line_top_y = origin_y + self.line_y_offset(line) - self.scroll_offset;

        if let Some(Some(info)) = self.line_display_info.get(line) {
            let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
            let pos = info
                .wrapped_line
                .position_for_index(disp_col, px(info.line_height))
                .unwrap_or_default();
            let indent_adjust = helpers::indent_for_y(info.indent_px, pos.y, info.line_height);
            let x = info.content_x + indent_adjust + f32::from(pos.x);
            let y = line_top_y + f32::from(pos.y);
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
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
