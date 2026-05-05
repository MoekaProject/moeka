use gpui::Context;

use crate::markdown::BlockType;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UtilityPanelToggleOutcome {
    next_panel: Option<RightUtilityPanel>,
    clear_resizing: bool,
    invalidate_info: bool,
    invalidate_git_history: bool,
}

fn utility_panel_toggle_outcome(
    current_panel: Option<RightUtilityPanel>,
    requested_panel: RightUtilityPanel,
) -> UtilityPanelToggleOutcome {
    if current_panel == Some(requested_panel) {
        return UtilityPanelToggleOutcome {
            next_panel: None,
            clear_resizing: true,
            invalidate_info: false,
            invalidate_git_history: false,
        };
    }

    UtilityPanelToggleOutcome {
        next_panel: Some(requested_panel),
        clear_resizing: false,
        invalidate_info: requested_panel == RightUtilityPanel::Info,
        invalidate_git_history: requested_panel == RightUtilityPanel::GitHistory,
    }
}

impl EditorView {
    pub(crate) fn scaled(&self, value: f32) -> f32 {
        value * self.ui_scale
    }

    pub(crate) fn base_font_size(&self) -> f32 {
        self.scaled(self.styles.editor.editor_font_size_px)
    }

    pub(crate) fn padding_left(&self) -> f32 {
        self.scaled(self.styles.editor.editor_padding_left_px)
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

    pub(crate) fn sidebar_width(&self) -> f32 {
        self.sidebar_custom_width
            .unwrap_or_else(|| self.scaled(self.styles.sidebar.sidebar_width_px))
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
            cx.notify();
            return;
        }
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

    pub(crate) fn utility_panel_width(&self) -> f32 {
        self.utility_panel_custom_width
            .unwrap_or_else(|| self.scaled(self.styles.utility_panel.utility_panel_width_px))
    }

    pub(crate) fn toggle_outline_visibility(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::Outline, cx);
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
        let outcome = utility_panel_toggle_outcome(self.utility_panel, panel);
        self.utility_panel = outcome.next_panel;
        if outcome.clear_resizing {
            self.utility_panel_resizing = false;
        }
        if outcome.invalidate_info {
            self.invalidate_info_panel_cache();
        }
        if outcome.invalidate_git_history {
            self.invalidate_git_history_cache();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{RightUtilityPanel, UtilityPanelToggleOutcome, utility_panel_toggle_outcome};

    #[test]
    fn toggling_same_panel_closes_and_clears_resizing() {
        let outcome =
            utility_panel_toggle_outcome(Some(RightUtilityPanel::Info), RightUtilityPanel::Info);

        assert_eq!(
            outcome,
            UtilityPanelToggleOutcome {
                next_panel: None,
                clear_resizing: true,
                invalidate_info: false,
                invalidate_git_history: false,
            }
        );
    }

    #[test]
    fn opening_info_panel_requests_info_invalidation_only() {
        let outcome = utility_panel_toggle_outcome(None, RightUtilityPanel::Info);

        assert_eq!(
            outcome,
            UtilityPanelToggleOutcome {
                next_panel: Some(RightUtilityPanel::Info),
                clear_resizing: false,
                invalidate_info: true,
                invalidate_git_history: false,
            }
        );
    }

    #[test]
    fn switching_to_git_history_requests_git_invalidation_only() {
        let outcome = utility_panel_toggle_outcome(
            Some(RightUtilityPanel::Info),
            RightUtilityPanel::GitHistory,
        );

        assert_eq!(
            outcome,
            UtilityPanelToggleOutcome {
                next_panel: Some(RightUtilityPanel::GitHistory),
                clear_resizing: false,
                invalidate_info: false,
                invalidate_git_history: true,
            }
        );
    }

    #[test]
    fn opening_outline_keeps_cache_invalidations_off() {
        let outcome = utility_panel_toggle_outcome(None, RightUtilityPanel::Outline);

        assert_eq!(
            outcome,
            UtilityPanelToggleOutcome {
                next_panel: Some(RightUtilityPanel::Outline),
                clear_resizing: false,
                invalidate_info: false,
                invalidate_git_history: false,
            }
        );
    }
}
