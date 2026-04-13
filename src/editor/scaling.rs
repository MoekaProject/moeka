use gpui::Context;

use crate::markdown::BlockType;

use super::*;

impl EditorView {
    pub(crate) fn scaled(&self, value: f32) -> f32 {
        value * self.ui_scale
    }

    pub(crate) fn base_font_size(&self) -> f32 {
        self.scaled(BASE_FONT_SIZE)
    }

    pub(crate) fn padding_left(&self) -> f32 {
        self.scaled(PADDING_LEFT)
    }

    pub(crate) fn padding_top(&self) -> f32 {
        self.scaled(PADDING_TOP)
    }

    pub(crate) fn padding_right(&self) -> f32 {
        self.scaled(PADDING_RIGHT)
    }

    pub(crate) fn padding_bottom(&self) -> f32 {
        self.scaled(PADDING_BOTTOM)
    }

    pub(crate) fn max_editor_width(&self) -> f32 {
        self.scaled(MAX_EDITOR_WIDTH)
    }

    pub(crate) fn sidebar_width(&self) -> f32 {
        self.sidebar_custom_width
            .unwrap_or_else(|| self.scaled(SIDEBAR_WIDTH))
    }

    pub(crate) fn tab_bar_height(&self) -> f32 {
        self.scaled(TAB_BAR_HEIGHT)
    }

    pub(crate) fn sidebar_row_height(&self) -> f32 {
        self.scaled(SIDEBAR_ROW_HEIGHT)
    }

    pub(crate) fn font_size_for_block(&self, block_type: &BlockType) -> f32 {
        let base = self.base_font_size();
        match block_type {
            BlockType::Heading(1) => base * 1.8,
            BlockType::Heading(2) => base * 1.5,
            BlockType::Heading(3) => base * 1.3,
            BlockType::Heading(4) => base * 1.15,
            BlockType::Heading(5) => base * 1.05,
            BlockType::Heading(6) => base * 1.0,
            _ => base,
        }
    }

    pub(crate) fn line_height_for_block(&self, block_type: &BlockType) -> f32 {
        self.font_size_for_block(block_type) * LINE_HEIGHT_FACTOR
    }

    pub(crate) fn visual_height_for_line(&self, line: usize) -> f32 {
        if let Some(Some(info)) = self.line_display_info.get(line) {
            info.line_height * info.visual_line_count as f32 + info.extra_height
        } else {
            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            self.line_height_for_block(block_type)
        }
    }

    pub(crate) fn line_y_offset(&self, target_line: usize) -> f32 {
        let mut y = self.padding_top();
        for line in 0..target_line.min(self.buffer.line_count()) {
            y += self.visual_height_for_line(line);
        }
        y
    }

    pub(crate) fn total_content_height(&self) -> f32 {
        self.line_y_offset(self.buffer.line_count()) + self.padding_bottom()
    }

    pub(crate) fn line_at_y(&self, y: f32) -> usize {
        let mut current_y = self.padding_top();
        for line in 0..self.buffer.line_count() {
            let lh = self.visual_height_for_line(line);
            if y < current_y + lh {
                return line;
            }
            current_y += lh;
        }
        self.buffer.line_count().saturating_sub(1)
    }

    pub(crate) fn scroll_to_cursor(&mut self) {
        let cursor_line = self.cursor_line();
        let cursor_y = self.line_y_offset(cursor_line);
        let cursor_line_height = self.visual_height_for_line(cursor_line);

        if cursor_y < self.scroll_offset + self.padding_top() {
            self.scroll_offset = (cursor_y - self.padding_top()).max(0.0);
        }
        if cursor_y + cursor_line_height
            > self.scroll_offset + self.last_viewport_height - self.padding_top()
        {
            self.scroll_offset =
                cursor_y + cursor_line_height - self.last_viewport_height + self.padding_top();
        }
    }

    pub(crate) fn clamp_scroll(&mut self) {
        let max_scroll = (self.total_content_height() - self.last_viewport_height).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);
    }

    // ── Visual wrap-line cursor movement helpers ─────────────────────

    pub(crate) fn disp_to_buffer_offset(
        &self,
        info: &LineDisplayInfo,
        line: usize,
        disp_idx: usize,
    ) -> usize {
        let mut dc = disp_idx.min(info.display_text.len());
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
        self.buffer.line_start(line) + buffer_col
    }

    pub(crate) fn offset_for_visual_up(&self, current_offset: usize) -> Option<usize> {
        let (line, col) = self.buffer.offset_to_line_col(current_offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let pos = info
            .wrapped_line
            .position_for_index(disp_col, gpui::px(info.line_height))
            .unwrap_or_default();
        let cur_visual = (f32::from(pos.y) / info.line_height) as usize;
        let cur_x = pos.x;

        if cur_visual > 0 {
            let target_y = gpui::px((cur_visual as f32 - 0.5) * info.line_height);
            let new_disp = match info.wrapped_line.closest_index_for_position(
                gpui::point(cur_x, target_y),
                gpui::px(info.line_height),
            ) {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line > 0 {
            let prev_info = self.line_display_info.get(line - 1)?.as_ref()?;
            let last_visual = prev_info.visual_line_count.saturating_sub(1);
            let target_y = gpui::px((last_visual as f32 + 0.5) * prev_info.line_height);
            let new_disp = match prev_info.wrapped_line.closest_index_for_position(
                gpui::point(cur_x, target_y),
                gpui::px(prev_info.line_height),
            ) {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(prev_info, line - 1, new_disp))
        } else {
            Some(0)
        }
    }

    pub(crate) fn offset_for_visual_down(&self, current_offset: usize) -> Option<usize> {
        let (line, col) = self.buffer.offset_to_line_col(current_offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let pos = info
            .wrapped_line
            .position_for_index(disp_col, gpui::px(info.line_height))
            .unwrap_or_default();
        let cur_visual = (f32::from(pos.y) / info.line_height) as usize;
        let cur_x = pos.x;

        if cur_visual + 1 < info.visual_line_count {
            let target_y = gpui::px((cur_visual as f32 + 1.5) * info.line_height);
            let new_disp = match info.wrapped_line.closest_index_for_position(
                gpui::point(cur_x, target_y),
                gpui::px(info.line_height),
            ) {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line + 1 < self.buffer.line_count() {
            let next_info = self.line_display_info.get(line + 1)?.as_ref()?;
            let target_y = gpui::px(0.5 * next_info.line_height);
            let new_disp = match next_info.wrapped_line.closest_index_for_position(
                gpui::point(cur_x, target_y),
                gpui::px(next_info.line_height),
            ) {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(next_info, line + 1, new_disp))
        } else {
            Some(self.buffer.len())
        }
    }

    pub(crate) fn toggle_sidebar_visibility(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        if !self.sidebar_visible {
            self.sidebar_resizing = false;
        }
        cx.notify();
    }

    pub(crate) fn toggle_editor_alignment(&mut self, cx: &mut Context<Self>) {
        self.editor_left_aligned = !self.editor_left_aligned;
        cx.notify();
    }
}
