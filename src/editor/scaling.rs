use gpui::Context;

use crate::markdown::BlockType;

use super::*;

impl EditorView {
    pub(crate) fn set_visible_utility_panel(
        &mut self,
        next_panel: Option<RightUtilityPanel>,
        cx: &mut Context<Self>,
    ) {
        let previous_panel = self.utility_panel;
        if next_panel != Some(RightUtilityPanel::Chat) {
            self.chat_panel_visible = false;
        }
        self.utility_panel = next_panel;

        if next_panel == Some(RightUtilityPanel::Info) {
            self.invalidate_info_panel_cache();
        }
        if next_panel == Some(RightUtilityPanel::GitHistory) {
            self.invalidate_git_history_cache();
        }
        if previous_panel != Some(RightUtilityPanel::Chat)
            && next_panel == Some(RightUtilityPanel::Chat)
        {
            self.open_chat_terminal(cx);
        }

        self.sync_utility_panel_width_animation(cx);
        cx.notify();
    }

    pub(crate) fn reveal_chat_panel(&mut self, cx: &mut Context<Self>) {
        self.chat_panel_visible = true;
        if let Some(current_panel) = self
            .utility_panel
            .filter(|panel| *panel != RightUtilityPanel::Chat)
        {
            self.last_non_chat_utility_panel = Some(current_panel);
        }

        if self.utility_panel != Some(RightUtilityPanel::Chat) {
            self.set_visible_utility_panel(Some(RightUtilityPanel::Chat), cx);
        } else {
            self.open_chat_terminal(cx);
        }
    }

    pub(crate) fn hide_chat_panel(&mut self, cx: &mut Context<Self>) {
        self.chat_panel_visible = false;
        if self.utility_panel == Some(RightUtilityPanel::Chat) {
            self.set_visible_utility_panel(self.last_non_chat_utility_panel, cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn scaled(&self, value: f32) -> f32 {
        value * self.ui_scale
    }

    pub(crate) fn base_font_size(&self) -> f32 {
        self.scaled(self.styles.editor.editor_font_size_px)
    }

    pub(crate) fn gutter_left_inset(&self) -> f32 {
        if self.active_tab_is_non_markdown_text() {
            0.0
        } else {
            self.scaled(self.styles.editor.editor_padding_left_px)
        }
    }

    pub(crate) fn gutter_line_number_width(&self) -> f32 {
        if !self.active_tab_is_non_markdown_text() {
            return 0.0;
        }
        let gutter_styles = &self.styles.gutter;
        let line_number_digits = self.buffer.line_count().max(1).to_string().len() as f32;
        let estimated_text_width =
            line_number_digits * self.scaled(gutter_styles.gutter_line_number_font_size_px) * 0.6;
        estimated_text_width.clamp(
            self.scaled(gutter_styles.gutter_line_number_min_width_px),
            self.scaled(gutter_styles.gutter_line_number_max_width_px),
        )
    }

    pub(crate) fn gutter_column_gap(&self) -> f32 {
        if self.active_tab_is_non_markdown_text() {
            self.scaled(self.styles.gutter.gutter_column_gap_px)
        } else {
            0.0
        }
    }

    pub(crate) fn gutter_icon_column_width(&self) -> f32 {
        let gutter_styles = &self.styles.gutter;
        let max_heading_icon_size = gutter_styles
            .gutter_heading_h1_icon_size_px
            .max(gutter_styles.gutter_heading_h2_icon_size_px)
            .max(gutter_styles.gutter_heading_h3_icon_size_px)
            .max(gutter_styles.gutter_heading_h4_icon_size_px);
        self.scaled(
            (gutter_styles.gutter_fold_icon_size_px + gutter_styles.gutter_fold_icon_offset_px)
                .max(
                    gutter_styles.gutter_blockquote_icon_size_px
                        + gutter_styles.gutter_blockquote_icon_offset_px,
                )
                .max(max_heading_icon_size + gutter_styles.gutter_heading_icon_offset_px),
        )
    }

    pub(crate) fn gutter_line_number_right_x(&self, editor_left: f32) -> f32 {
        editor_left + self.gutter_left_inset() + self.gutter_line_number_width()
    }

    pub(crate) fn gutter_icon_column_left(&self, editor_left: f32) -> f32 {
        self.gutter_line_number_right_x(editor_left) + self.gutter_column_gap()
    }

    pub(crate) fn gutter_icon_x(&self, editor_left: f32, icon_size: f32) -> f32 {
        self.gutter_icon_column_left(editor_left)
            + ((self.gutter_icon_column_width() - icon_size) / 2.0).max(0.0)
    }

    pub(crate) fn padding_left(&self) -> f32 {
        if self.active_tab_is_non_markdown_text() {
            self.gutter_left_inset()
                + self.gutter_line_number_width()
                + self.gutter_column_gap()
                + self.gutter_icon_column_width()
                + self.gutter_column_gap()
        } else {
            self.scaled(self.styles.editor.editor_padding_left_px)
        }
    }

    pub(crate) fn padding_top(&self) -> f32 {
        self.scaled(self.styles.editor.editor_padding_top_px)
    }

    pub(crate) fn padding_right(&self) -> f32 {
        self.scaled(self.styles.editor.editor_padding_right_px)
    }

    pub(crate) fn padding_bottom(&self) -> f32 {
        self.scaled(self.styles.editor.editor_padding_bottom_px)
    }

    pub(crate) fn max_editor_width(&self) -> f32 {
        if self.active_tab_is_non_markdown_text() {
            self.scaled(self.styles.editor.editor_non_markdown_max_width_px)
        } else {
            self.scaled(self.styles.editor.editor_max_width_px)
        }
    }

    pub(crate) fn tab_bar_height(&self) -> f32 {
        self.scaled(self.styles.tab_bar.tab_bar_height_px)
    }

    pub(crate) fn font_size_for_block(&self, block_type: &BlockType) -> f32 {
        match block_type {
            BlockType::Heading(1) => self.scaled(self.styles.editor.editor_h1_font_size_px),
            BlockType::Heading(2) => self.scaled(self.styles.editor.editor_h2_font_size_px),
            BlockType::Heading(3) => self.scaled(self.styles.editor.editor_h3_font_size_px),
            BlockType::Heading(4) => self.scaled(self.styles.editor.editor_h4_font_size_px),
            BlockType::Heading(5) => self.scaled(self.styles.editor.editor_h5_font_size_px),
            BlockType::Heading(6) => self.scaled(self.styles.editor.editor_h6_font_size_px),
            _ => self.base_font_size(),
        }
    }

    pub(crate) fn line_height_for_block(&self, block_type: &BlockType) -> f32 {
        let factor = match block_type {
            BlockType::Heading(_) => self.styles.editor.editor_heading_line_height_factor,
            _ => self.styles.editor.editor_paragraph_line_height_factor,
        };
        self.font_size_for_block(block_type) * factor
    }

    fn is_non_empty_line(&self, line: usize) -> bool {
        if line >= self.buffer.line_count() {
            return false;
        }

        !self.buffer.line_content(line).trim().is_empty()
    }

    fn is_heading_line(&self, line: usize) -> bool {
        if !self.is_non_empty_line(line) {
            return false;
        }

        matches!(
            self.decorations.get(line).map(|d| &d.block_type),
            Some(BlockType::Heading(_))
        )
    }

    fn is_paragraph_style_line(&self, line: usize) -> bool {
        if !self.is_non_empty_line(line) {
            return false;
        }

        matches!(
            self.decorations.get(line).map(|d| &d.block_type),
            Some(block_type) if !matches!(block_type, BlockType::Heading(_))
        )
    }

    pub(crate) fn block_padding_for_line(&self, line: usize) -> (f32, f32) {
        if line >= self.buffer.line_count() {
            return (0.0, 0.0);
        }

        let line_content = self.buffer.line_content(line);
        if line_content.trim().is_empty() {
            return (0.0, 0.0);
        }

        let Some(block_type) = self.decorations.get(line).map(|d| &d.block_type) else {
            return (0.0, 0.0);
        };

        match block_type {
            BlockType::Heading(_) if self.is_heading_line(line) => {
                let top = if line == 0 || self.is_non_empty_line(line - 1) {
                    self.scaled(self.styles.editor.editor_heading_padding_top_px)
                } else {
                    0.0
                };
                let bottom =
                    if line + 1 >= self.buffer.line_count() || self.is_non_empty_line(line + 1) {
                        self.scaled(self.styles.editor.editor_heading_padding_bottom_px)
                    } else {
                        0.0
                    };
                (top, bottom)
            }
            _ if self.is_paragraph_style_line(line) => {
                let top = if line == 0 || self.is_heading_line(line - 1) {
                    self.scaled(self.styles.editor.editor_paragraph_padding_top_px)
                } else {
                    0.0
                };
                let bottom =
                    if line + 1 >= self.buffer.line_count() || self.is_heading_line(line + 1) {
                        self.scaled(self.styles.editor.editor_paragraph_padding_bottom_px)
                    } else {
                        0.0
                    };
                (top, bottom)
            }
            _ => (0.0, 0.0),
        }
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
            let (padding_top, padding_bottom) = self.block_padding_for_line(line);
            self.line_height_for_block(block_type) + padding_top + padding_bottom
        }
    }

    pub(crate) fn line_y_offset(&self, target_line: usize) -> f32 {
        let mut y = self.padding_top();
        for line in 0..target_line.min(self.buffer.line_count()) {
            y += self.visual_height_for_line(line);
        }
        y
    }

    pub(crate) fn text_y_offset_for_line(&self, line: usize) -> f32 {
        self.line_y_offset(line) + self.block_padding_for_line(line).0
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
        if self.layout_dirty {
            self.pending_scroll_to_cursor = true;
            return;
        }

        self.pending_scroll_to_cursor = false;
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

        if self.utility_panel == Some(RightUtilityPanel::Outline)
            && let Some(active_line) = self.active_outline_heading_line()
        {
            let headings = self.collect_outline_headings();
            if let Some(index) = headings
                .iter()
                .position(|heading| heading.line == active_line)
            {
                self.outline_scroll_handle.scroll_to_item(index + 2);
            }
        }
    }

    fn sync_outline_scroll_position(&self) {
        if self.utility_panel == Some(RightUtilityPanel::Outline)
            && let Some(active_line) = self.active_outline_heading_line()
        {
            let headings = self.collect_outline_headings();
            if let Some(index) = headings
                .iter()
                .position(|heading| heading.line == active_line)
            {
                self.outline_scroll_handle.scroll_to_item(index + 2);
            }
        }
    }

    fn animate_scroll_to(&mut self, target_scroll: f32, cx: &mut Context<Self>) {
        let start_scroll = self.scroll_offset;
        self.scroll_offset = target_scroll;
        self.clamp_scroll();
        let final_scroll = self.scroll_offset;

        if (final_scroll - start_scroll).abs() < 1.0 {
            self.scroll_offset = final_scroll;
            self.sync_outline_scroll_position();
            cx.notify();
            return;
        }

        self.scroll_anim_epoch += 1;
        let epoch = self.scroll_anim_epoch;
        self.scroll_offset = start_scroll;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let total_steps = 14u32;
            let step_ms = 16u64;
            for step in 1..=total_steps {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(step_ms))
                    .await;
                let done = this
                    .update(cx, |this, cx| {
                        if this.scroll_anim_epoch != epoch {
                            return true;
                        }

                        let t = step as f32 / total_steps as f32;
                        let ease = 1.0 - (1.0 - t).powi(3);
                        this.scroll_offset = start_scroll + (final_scroll - start_scroll) * ease;
                        this.clamp_scroll();
                        this.sync_outline_scroll_position();
                        cx.notify();
                        step == total_steps
                    })
                    .unwrap_or(true);
                if done {
                    return;
                }
            }
        })
        .detach();
    }

    pub(crate) fn center_cursor_in_view(&mut self, cx: &mut Context<Self>) {
        if self.layout_dirty {
            self.pending_center_cursor_in_view = true;
            self.pending_scroll_to_cursor = false;
            return;
        }

        self.pending_center_cursor_in_view = false;
        self.pending_scroll_to_cursor = false;
        let cursor_line = self.cursor_line();
        let cursor_y = self.line_y_offset(cursor_line);
        let cursor_line_height = self.visual_height_for_line(cursor_line);
        let target_scroll =
            (cursor_y + cursor_line_height * 0.5 - self.last_viewport_height * 0.5).max(0.0);
        self.animate_scroll_to(target_scroll, cx);
    }

    pub(crate) fn clamp_scroll(&mut self) {
        // Allow scrolling the last line to the top (like Monaco's scrollBeyondLastLine)
        let last_line_height =
            self.visual_height_for_line(self.buffer.line_count().saturating_sub(1));
        let max_scroll =
            (self.total_content_height() - last_line_height - self.padding_bottom()).max(0.0);
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
        let _disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let (cur_visual, current_x) = self.cursor_visual_position(current_offset)?;
        let goal_x = self.vertical_move_goal_x.unwrap_or(current_x);

        if cur_visual > 0 {
            let target_visual = cur_visual - 1;
            let target_y = gpui::px((cur_visual as f32 - 0.5) * info.line_height);
            let target_indent = if target_visual > 0 {
                info.indent_px
            } else {
                0.0
            };
            let target_x = (goal_x - target_indent).max(0.0);
            let new_disp = if target_x <= 0.5 {
                Self::wrap_boundary_display_index_for_visual_line(info, target_visual)
                    .unwrap_or_default()
            } else {
                match info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line > 0 {
            let prev_info = self.line_display_info.get(line - 1)?.as_ref()?;
            let last_visual = prev_info.visual_line_count.saturating_sub(1);
            let target_y = gpui::px((last_visual as f32 + 0.5) * prev_info.line_height);
            let target_indent = if last_visual > 0 {
                prev_info.indent_px
            } else {
                0.0
            };
            let target_x = (goal_x - target_indent).max(0.0);
            let new_disp = if target_x <= 0.5 {
                Self::wrap_boundary_display_index_for_visual_line(prev_info, last_visual)
                    .unwrap_or_default()
            } else {
                match prev_info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(prev_info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(prev_info, line - 1, new_disp))
        } else {
            Some(0)
        }
    }

    pub(crate) fn offset_for_visual_down(&self, current_offset: usize) -> Option<usize> {
        let (line, col) = self.buffer.offset_to_line_col(current_offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let _disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let (cur_visual, current_x) = self.cursor_visual_position(current_offset)?;
        let goal_x = self.vertical_move_goal_x.unwrap_or(current_x);

        if cur_visual + 1 < info.visual_line_count {
            let target_visual = cur_visual + 1;
            let target_y = gpui::px((cur_visual as f32 + 1.5) * info.line_height);
            let target_indent = if target_visual > 0 {
                info.indent_px
            } else {
                0.0
            };
            let target_x = (goal_x - target_indent).max(0.0);
            let new_disp = if target_x <= 0.5 {
                Self::wrap_boundary_display_index_for_visual_line(info, target_visual)
                    .unwrap_or_default()
            } else {
                match info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line + 1 < self.buffer.line_count() {
            let next_info = self.line_display_info.get(line + 1)?.as_ref()?;
            let target_y = gpui::px(0.5 * next_info.line_height);
            let target_x = goal_x.max(0.0);
            let new_disp = if target_x <= 0.5 {
                0
            } else {
                match next_info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(next_info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(next_info, line + 1, new_disp))
        } else {
            Some(self.buffer.len())
        }
    }

    pub(crate) fn toggle_sidebar_visibility(&mut self, cx: &mut Context<Self>) {
        if self.workspaces.is_empty() {
            self.sidebar_visible = false;
            self.sidebar_resizing = false;
            self.sync_sidebar_width_animation(cx);
            cx.notify();
            return;
        }
        self.sidebar_visible = !self.sidebar_visible;
        if !self.sidebar_visible {
            self.sidebar_resizing = false;
        }
        self.sync_sidebar_width_animation(cx);
        cx.notify();
    }

    pub(crate) fn toggle_editor_alignment(&mut self, cx: &mut Context<Self>) {
        self.editor_left_aligned = !self.editor_left_aligned;
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].editor_left_aligned = self.editor_left_aligned;
            session_store::persist_session(self);
        }
        cx.notify();
    }

    pub(crate) fn toggle_outline_visibility(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::Outline, cx);
    }

    pub(crate) fn toggle_chat_panel(&mut self, cx: &mut Context<Self>) {
        self.utility_panel_resizing = false;
        if self.chat_panel_visible {
            self.hide_chat_panel(cx);
            return;
        }

        self.chat_panel_placement = ChatPanelPlacement::Right;
        self.reveal_chat_panel(cx);
    }

    pub(crate) fn show_chat_panel_bottom(&mut self, cx: &mut Context<Self>) {
        self.utility_panel_resizing = false;
        let should_hide = self.chat_panel_visible
            && self.utility_panel == Some(RightUtilityPanel::Chat)
            && self.chat_panel_placement == ChatPanelPlacement::Bottom;
        if should_hide {
            self.hide_chat_panel(cx);
            return;
        }

        self.chat_panel_placement = ChatPanelPlacement::Bottom;
        self.reveal_chat_panel(cx);
    }

    pub(crate) fn toggle_info_panel(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::Info, cx);
    }

    pub(crate) fn toggle_git_history_panel(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::GitHistory, cx);
    }

    pub(crate) fn toggle_utility_panel(
        &mut self,
        panel: RightUtilityPanel,
        cx: &mut Context<Self>,
    ) {
        if panel == RightUtilityPanel::Chat {
            self.toggle_chat_panel(cx);
            return;
        }

        if self.utility_panel == Some(panel) {
            self.utility_panel_resizing = false;
            if self.last_non_chat_utility_panel == Some(panel) {
                self.last_non_chat_utility_panel = None;
            }
            if self.chat_panel_visible {
                self.set_visible_utility_panel(Some(RightUtilityPanel::Chat), cx);
            } else {
                self.set_visible_utility_panel(None, cx);
            }
            return;
        }

        self.chat_panel_visible = false;
        self.last_non_chat_utility_panel = Some(panel);
        self.set_visible_utility_panel(Some(panel), cx);
    }
}
