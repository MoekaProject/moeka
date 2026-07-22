use gpui::Context;
#[cfg(target_os = "macos")]
use gpui::{Window, point, px};

use crate::markdown::BlockType;

use super::*;

fn caret_vertical_overflow(
    cursor_top: f32,
    cursor_bottom: f32,
    viewport_top: f32,
    viewport_bottom: f32,
) -> std::cmp::Ordering {
    if cursor_top < viewport_top {
        std::cmp::Ordering::Less
    } else if cursor_bottom > viewport_bottom {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

impl EditorView {
    fn actual_visual_line_count(info: &LineDisplayInfo) -> usize {
        info.wrapped_line.wrap_boundaries().len() + 1
    }

    pub(crate) fn set_visible_utility_panel(
        &mut self,
        next_panel: Option<RightUtilityPanel>,
        cx: &mut Context<Self>,
    ) {
        let previous_panel = self.utility_panel;
        self.utility_panel = next_panel;

        if next_panel == Some(RightUtilityPanel::Info) {
            self.invalidate_info_panel_cache();
        }
        if next_panel == Some(RightUtilityPanel::GitHistory) {
            self.invalidate_git_history_cache();
        }

        // Switching directly between two visible utility panels is a panel-type
        // switch: apply the width change instantly. Only animate when a panel is
        // appearing (none -> some) or disappearing (some -> none).
        let is_type_switch =
            previous_panel.is_some() && next_panel.is_some() && previous_panel != next_panel;
        if is_type_switch {
            self.displayed_utility_panel = next_panel;
            self.set_utility_panel_width_immediately(self.utility_panel_target_width());
        } else {
            self.sync_utility_panel_width_animation(cx);
        }
        cx.notify();
    }

    pub(crate) fn scaled(&self, value: f32) -> f32 {
        value * self.ui_scale
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn traffic_light_position_for_ui_scale(ui_scale: f32) -> gpui::Point<gpui::Pixels> {
        const STANDARD_BUTTON_HEIGHT_PX: f32 = 14.0;
        traffic_light_offsets_for_layout(
            ui_scale,
            crate::style::TOP_BAR_HEIGHT_PX * ui_scale,
            STANDARD_BUTTON_HEIGHT_PX,
        )
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn sync_macos_traffic_light_position(&mut self, window: &mut Window) {
        if self
            .traffic_light_ui_scale
            .is_some_and(|scale| (scale - self.ui_scale).abs() < f32::EPSILON)
        {
            return;
        }
        if apply_macos_traffic_light_position(window, self.ui_scale, self.tab_bar_height()) {
            self.traffic_light_ui_scale = Some(self.ui_scale);
        }
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
        let padding = if self.app_settings.simple_mode {
            self.styles.editor.editor_non_markdown_padding_top_px
        } else {
            self.styles.editor.editor_padding_top_px
        };
        self.scaled(padding)
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

    pub(crate) fn editor_uses_expanded_layout(&self) -> bool {
        self.active_tab_is_non_markdown_text() || self.editor_left_aligned
    }

    pub(crate) fn editor_wraps_to_available_width(&self) -> bool {
        !self.active_tab_is_non_markdown_text() || !self.editor_left_aligned
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
        let (base_top, base_bottom) = self.base_block_padding_for_line(line);
        // Expanded inline-diff hunks reserve space for their read-only removed
        // rows by growing the anchor line's top padding (or the last line's
        // bottom padding for a trailing deletion). Routing this through
        // `block_padding_for_line` keeps every consumer — the geometry cache,
        // `text_y_offset_for_line`, and the per-line layout — in agreement. The
        // removed rows soft-wrap, so the reserved height comes from the measured
        // (or estimated) phantom heights rather than a fixed row count.
        let above = self.inline_diff_phantom_reserved_above(line);
        let below = self.inline_diff_phantom_reserved_below(line);
        (base_top + above, base_bottom + below)
    }

    fn base_block_padding_for_line(&self, line: usize) -> (f32, f32) {
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

        if self.is_quote_line(line) {
            // Callout blocks (`> [!KIND]`) get extra inner vertical padding on
            // top of the plain quote padding; the callout kind is only tagged on
            // the group's first line.
            let group_start = self.quote_group_start(line);
            let is_callout = self
                .decorations
                .get(group_start)
                .is_some_and(|decoration| decoration.callout_kind.is_some());
            let callout_top = if is_callout {
                self.scaled(self.styles.quote_block.quote_block_callout_padding_top_px)
            } else {
                0.0
            };
            let callout_bottom = if is_callout {
                self.scaled(
                    self.styles
                        .quote_block
                        .quote_block_callout_padding_bottom_px,
                )
            } else {
                0.0
            };
            let top = self
                .is_quote_group_start(line)
                .then(|| {
                    self.scaled(self.styles.quote_block.quote_block_padding_top_px) + callout_top
                })
                .unwrap_or(0.0);
            let bottom = self
                .is_quote_group_end(line)
                .then(|| {
                    self.scaled(self.styles.quote_block.quote_block_padding_bottom_px)
                        + callout_bottom
                })
                .unwrap_or(0.0);
            return (top, bottom);
        }

        match block_type {
            BlockType::Heading(_) if self.is_heading_line(line) => (
                self.scaled(self.styles.editor.editor_heading_padding_top_px),
                self.scaled(self.styles.editor.editor_heading_padding_bottom_px),
            ),
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

    pub(crate) fn is_quote_line(&self, line: usize) -> bool {
        self.decorations.get(line).is_some_and(|decoration| {
            decoration.block_type == BlockType::Blockquote
                || decoration
                    .block_contexts
                    .contains(&crate::markdown::BlockContext::Blockquote)
        })
    }

    pub(crate) fn is_quote_group_start(&self, line: usize) -> bool {
        self.is_quote_line(line) && (line == 0 || !self.is_quote_line(line - 1))
    }

    pub(crate) fn is_quote_group_end(&self, line: usize) -> bool {
        self.is_quote_line(line)
            && (line + 1 >= self.buffer.line_count() || !self.is_quote_line(line + 1))
    }

    pub(crate) fn quote_group_start(&self, line: usize) -> usize {
        let mut start = line;
        while start > 0 && self.is_quote_line(start - 1) {
            start -= 1;
        }
        start
    }

    fn current_visual_height_for_line(&self, line: usize) -> f32 {
        if let Some(Some(info)) = self.line_display_info.get(line) {
            info.line_height * info.visual_line_count as f32 + info.extra_height
        } else {
            if !self.folded_sections.is_empty()
                && self.folded_sections.iter().any(|&fold_line| {
                    self.fold_range_for_line(fold_line)
                        .is_some_and(|range| range.contains(&line))
                })
            {
                return 0.0;
            }

            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            let (padding_top, padding_bottom) = self.block_padding_for_line(line);
            self.line_height_for_block(block_type) + padding_top + padding_bottom
        }
    }

    fn ensure_line_geometry_cache(&self) {
        let line_count = self.buffer.line_count();
        if !self.line_geometry_cache_dirty.get()
            && self.line_geometry_cache.borrow().prefix_y_offsets.len() == line_count + 1
        {
            return;
        }

        let mut cache = self.line_geometry_cache.borrow_mut();
        cache.prefix_y_offsets.clear();
        cache.prefix_y_offsets.reserve(line_count + 1);

        let mut y = self.padding_top();
        cache.prefix_y_offsets.push(y);
        for line in 0..line_count {
            y += self.current_visual_height_for_line(line);
            cache.prefix_y_offsets.push(y);
        }

        self.line_geometry_cache_dirty.set(false);
    }

    pub(crate) fn visual_height_for_line(&self, line: usize) -> f32 {
        self.current_visual_height_for_line(line)
    }

    pub(crate) fn line_y_offset(&self, target_line: usize) -> f32 {
        self.ensure_line_geometry_cache();
        let target_line = target_line.min(self.buffer.line_count());
        self.line_geometry_cache.borrow().prefix_y_offsets[target_line]
    }

    /// Extra height reserved above an image line's syntax text by its inline
    /// preview (image plus top/bottom gaps). The preview is painted above the
    /// syntax text, so this offset shifts the text row down within the line
    /// box. Keep in sync with the per-line layout in `element.rs`.
    pub(crate) fn image_preview_extra_height_for_line(&self, line: usize) -> f32 {
        if self.folded_image_previews.contains(&line) {
            return 0.0;
        }
        let Some(path) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.image_path.as_ref())
        else {
            return 0.0;
        };
        let Some(Some(img_data)) = self.image_cache.get(path) else {
            return 0.0;
        };
        let img_size = img_data.size(0);
        let img_w = img_size.width.0 as f32;
        let img_h = img_size.height.0 as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return 0.0;
        }
        let (_, display_h) = helpers::compute_image_preview_size(
            img_w,
            img_h,
            self.last_text_width,
            self.last_window_scale_factor,
        );
        display_h
            + self.scaled(self.styles.image_preview.image_gap_top_px)
            + self.scaled(self.styles.image_preview.image_gap_bottom_px)
    }

    pub(crate) fn text_y_offset_for_line(&self, line: usize) -> f32 {
        self.line_y_offset(line)
            + self.block_padding_for_line(line).0
            + self.image_preview_extra_height_for_line(line)
    }

    pub(crate) fn total_content_height(&self) -> f32 {
        self.ensure_line_geometry_cache();
        self.line_geometry_cache.borrow().prefix_y_offsets[self.buffer.line_count()]
            + self.padding_bottom()
    }

    pub(crate) fn line_at_y(&self, y: f32) -> usize {
        let line_count = self.buffer.line_count();
        if line_count == 0 {
            return 0;
        }

        self.ensure_line_geometry_cache();
        let cache = self.line_geometry_cache.borrow();
        let prefix = &cache.prefix_y_offsets;
        let target_y = y.max(prefix[0]);

        let mut low = 0usize;
        let mut high = line_count;
        while low < high {
            let mid = (low + high) / 2;
            if prefix[mid + 1] <= target_y {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        low.min(line_count.saturating_sub(1))
    }

    pub(crate) fn scroll_to_cursor(&mut self) {
        if self.layout_dirty {
            self.pending_scroll_to_cursor = true;
            return;
        }

        self.pending_scroll_to_cursor = false;
        // While the cursor sits inside a generated preview's source (code view),
        // keep the scroll fixed: any reflow-driven scroll adjustment here makes
        // the right-side render bounce up/down on every keystroke. The preview
        // source is already visible while editing, so no scroll is needed.
        if self.focused_code_mode_preview_key().is_some() {
            return;
        }
        let cursor_line = self.cursor_line();
        let (cursor_visual_row, cursor_visual_x) = self
            .cursor_visual_position(self.cursor.offset)
            .unwrap_or((0, 0.0));
        let cursor_y = self.text_y_offset_for_line(cursor_line)
            + cursor_visual_row as f32
                * self
                    .line_display_info
                    .get(cursor_line)
                    .and_then(|info| info.as_ref())
                    .map(|info| info.line_height)
                    .unwrap_or_else(|| {
                        let block_type = self
                            .decorations
                            .get(cursor_line)
                            .map(|decoration| &decoration.block_type)
                            .unwrap_or(&BlockType::Normal);
                        self.line_height_for_block(block_type)
                    });
        let cursor_line_height = self
            .line_display_info
            .get(cursor_line)
            .and_then(|info| info.as_ref())
            .map(|info| info.line_height)
            .unwrap_or_else(|| {
                let block_type = self
                    .decorations
                    .get(cursor_line)
                    .map(|decoration| &decoration.block_type)
                    .unwrap_or(&BlockType::Normal);
                self.line_height_for_block(block_type)
            });
        let viewport_height = self.editor_visible_height();
        let viewport_top = self.scroll_offset + self.padding_top();
        let viewport_bottom = self.scroll_offset + viewport_height - self.padding_bottom();
        let cursor_top = cursor_y;
        let cursor_bottom = cursor_y + cursor_line_height;
        let is_visible = cursor_top >= viewport_top && cursor_bottom <= viewport_bottom;

        if is_visible
            && self
                .last_scroll_cursor_visual_x
                .is_some_and(|previous_x| (previous_x - cursor_visual_x).abs() < 0.5)
        {
            return;
        }

        match caret_vertical_overflow(cursor_top, cursor_bottom, viewport_top, viewport_bottom) {
            std::cmp::Ordering::Less => {
                self.scroll_offset = (cursor_y - self.padding_top()).max(0.0);
                self.clamp_scroll();
                self.last_scroll_cursor_visual_x = Some(cursor_visual_x);
            }
            std::cmp::Ordering::Greater => {
                self.scroll_offset =
                    cursor_y + cursor_line_height - viewport_height + self.padding_bottom();
                self.clamp_scroll();
                self.last_scroll_cursor_visual_x = Some(cursor_visual_x);
            }
            std::cmp::Ordering::Equal => {}
        }
    }

    fn animate_scroll_to(&mut self, target_scroll: f32, cx: &mut Context<Self>) {
        let start_scroll = self.scroll_offset;
        self.scroll_offset = target_scroll;
        self.clamp_scroll();
        let final_scroll = self.scroll_offset;

        if (final_scroll - start_scroll).abs() < 1.0 {
            self.scroll_offset = final_scroll;
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
        if self.embedded_editor && self.embedded_read_only {
            self.embedded_stick_to_bottom = false;
        }
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

    pub(crate) fn center_cursor_in_view_immediately(&mut self, cx: &mut Context<Self>) {
        if self.embedded_editor && self.embedded_read_only {
            self.embedded_stick_to_bottom = false;
        }
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
        self.scroll_offset =
            (cursor_y + cursor_line_height * 0.5 - self.last_viewport_height * 0.5).max(0.0);
        self.clamp_scroll();
        cx.notify();
    }

    pub(crate) fn max_scroll_offset(&self) -> f32 {
        if self.embedded_editor {
            // Embedded read-only editors pin the scroll
            // range to the exact content height: no scroll-beyond-last-line,
            // so no blank gap can appear below the content. Prefer the
            // layout-accurate painted height over mid-frame estimates.
            (self
                .total_content_height()
                .max(self.embedded_content_height.get())
                - self.last_viewport_height)
                .max(0.0)
        } else {
            // No scrolling (and no scrollbar) when the whole document
            // already fits inside the viewport.
            if self.total_content_height() <= self.last_viewport_height {
                return 0.0;
            }
            // Allow scrolling the last line to the top (like Monaco's
            // scrollBeyondLastLine).
            let last_line_height =
                self.visual_height_for_line(self.buffer.line_count().saturating_sub(1));
            (self.total_content_height() - last_line_height - self.padding_bottom()).max(0.0)
        }
    }

    pub(crate) fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll_offset());
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
            let last_visual = Self::actual_visual_line_count(prev_info).saturating_sub(1);
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

        if cur_visual + 1 < Self::actual_visual_line_count(info) {
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
        self.layout_dirty = true;
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].editor_left_aligned = self.editor_left_aligned;
            session_store::persist_session(self);
        }
        cx.notify();
    }

    pub(crate) fn toggle_outline_visibility(&mut self, cx: &mut Context<Self>) {
        self.toggle_outline_panel(cx);
    }

    pub(crate) fn toggle_info_panel(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::Info, cx);
    }

    pub(crate) fn toggle_git_history_panel(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::GitHistory, cx);
    }

    pub(crate) fn toggle_outline_panel(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::Outline, cx);
    }

    pub(crate) fn toggle_utility_panel(
        &mut self,
        panel: RightUtilityPanel,
        cx: &mut Context<Self>,
    ) {
        if self.utility_panel == Some(panel) {
            self.utility_panel_resizing = false;
            self.set_visible_utility_panel(None, cx);
            return;
        }

        self.set_visible_utility_panel(Some(panel), cx);
    }
}

#[cfg(target_os = "macos")]
fn traffic_light_offsets_for_layout(
    ui_scale: f32,
    tab_bar_height: f32,
    button_height: f32,
) -> gpui::Point<gpui::Pixels> {
    const BASE_LEFT_PX: f32 = 10.0;
    point(
        px(BASE_LEFT_PX * ui_scale),
        px(((tab_bar_height - button_height) * 0.5).max(0.0)),
    )
}

#[cfg(target_os = "macos")]
fn apply_macos_traffic_light_position(
    window: &mut Window,
    ui_scale: f32,
    tab_bar_height: f32,
) -> bool {
    use core_graphics::geometry::{CGPoint, CGRect};
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() else {
        return false;
    };

    unsafe {
        let ns_view = appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
        let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return false;
        }
        let style_mask: u64 = msg_send![ns_window, styleMask];
        const NS_WINDOW_STYLE_MASK_FULL_SCREEN: u64 = 1 << 14;
        if style_mask & NS_WINDOW_STYLE_MASK_FULL_SCREEN != 0 {
            return false;
        }

        let close_button: *mut objc::runtime::Object =
            msg_send![ns_window, standardWindowButton: 0u64];
        let minimize_button: *mut objc::runtime::Object =
            msg_send![ns_window, standardWindowButton: 1u64];
        let zoom_button: *mut objc::runtime::Object =
            msg_send![ns_window, standardWindowButton: 2u64];
        if close_button.is_null() || minimize_button.is_null() || zoom_button.is_null() {
            return false;
        }

        let window_frame: CGRect = msg_send![ns_window, frame];
        let content_layout_rect: CGRect = msg_send![ns_window, contentLayoutRect];
        let close_frame: CGRect = msg_send![close_button, frame];
        let minimize_frame: CGRect = msg_send![minimize_button, frame];
        let titlebar_height = window_frame.size.height - content_layout_rect.size.height;
        let offsets = traffic_light_offsets_for_layout(
            ui_scale,
            tab_bar_height,
            close_frame.size.height as f32,
        );
        let left = f32::from(offsets.x) as f64;
        let top = f32::from(offsets.y) as f64;
        let y = titlebar_height - top - close_frame.size.height;
        let spacing = minimize_frame.origin.x - close_frame.origin.x;

        let _: () = msg_send![close_button, setFrameOrigin: CGPoint::new(left, y)];
        let _: () = msg_send![minimize_button, setFrameOrigin: CGPoint::new(left + spacing, y)];
        let _: () = msg_send![zoom_button, setFrameOrigin: CGPoint::new(left + spacing * 2.0, y)];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::caret_vertical_overflow;
    #[cfg(target_os = "macos")]
    use super::{EditorView, traffic_light_offsets_for_layout};

    #[cfg(target_os = "macos")]
    #[test]
    fn traffic_light_stays_centered_in_scaled_tab_bar() {
        let button_height = 14.0;
        for ui_scale in [0.8, 1.0, 1.4] {
            let tab_bar_height = crate::style::TOP_BAR_HEIGHT_PX * ui_scale;
            let offsets = traffic_light_offsets_for_layout(ui_scale, tab_bar_height, button_height);
            assert_eq!(f32::from(offsets.x), 10.0 * ui_scale);
            assert_eq!(
                f32::from(offsets.y) + button_height * 0.5,
                tab_bar_height * 0.5
            );

            let initial = EditorView::traffic_light_position_for_ui_scale(ui_scale);
            assert_eq!(initial, offsets);
        }
    }

    #[test]
    fn caret_scrolls_only_after_leaving_the_viewport() {
        use std::cmp::Ordering::{Equal, Greater, Less};

        assert_eq!(caret_vertical_overflow(100.0, 120.0, 100.0, 200.0), Equal);
        assert_eq!(caret_vertical_overflow(180.0, 200.0, 100.0, 200.0), Equal);
        assert_eq!(caret_vertical_overflow(99.9, 119.9, 100.0, 200.0), Less);
        assert_eq!(caret_vertical_overflow(180.1, 200.1, 100.0, 200.0), Greater);
    }
}
