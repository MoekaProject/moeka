use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, SharedString, div, prelude::*,
    px,
};

use crate::markdown::BlockType;

use super::*;

pub(crate) struct OutlineHeading {
    pub line: usize,
    pub level: u8,
    pub text: SharedString,
}

impl EditorView {
    pub(crate) fn flash_outline_heading_line(
        &mut self,
        target_line: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.current_file_path.clone() else {
            return;
        };

        let start = self.buffer.line_start(target_line);
        let end = self.buffer.line_end(target_line);
        if start >= end {
            return;
        }

        let range = start..end;
        let outline_styles = &self.styles.outline;
        let initial_delay_ms =
            std::time::Duration::from_millis(outline_styles.outline_flash_initial_delay_ms);
        let show_ms =
            std::time::Duration::from_millis(outline_styles.outline_flash_show_duration_ms);
        let gap_ms = std::time::Duration::from_millis(outline_styles.outline_flash_gap_duration_ms);
        self.outline_flash_epoch += 1;
        let epoch = self.outline_flash_epoch;
        self.outline_flash_highlight_path = Some(path.clone());
        self.outline_flash_highlight_ranges = vec![range.clone()];
        self.outline_flash_highlight_current_index = Some(0);
        cx.notify();

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(initial_delay_ms).await;
            let continue_flash = this
                .update(cx, |this, cx| {
                    if this.outline_flash_epoch != epoch {
                        return false;
                    }
                    this.outline_flash_highlight_path = Some(path.clone());
                    this.outline_flash_highlight_ranges = vec![range.clone()];
                    this.outline_flash_highlight_current_index = Some(0);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !continue_flash {
                return;
            }

            cx.background_executor().timer(show_ms).await;
            let _ = this.update(cx, |this, cx| {
                if this.outline_flash_epoch != epoch {
                    return;
                }
                this.outline_flash_highlight_path = None;
                this.outline_flash_highlight_ranges.clear();
                this.outline_flash_highlight_current_index = None;
                cx.notify();
            });

            cx.background_executor().timer(gap_ms).await;
            let _ = this.update(cx, |this, cx| {
                if this.outline_flash_epoch != epoch {
                    return;
                }
                this.outline_flash_highlight_path = Some(path.clone());
                this.outline_flash_highlight_ranges = vec![range.clone()];
                this.outline_flash_highlight_current_index = Some(0);
                cx.notify();
            });

            cx.background_executor().timer(show_ms).await;
            let _ = this.update(cx, |this, cx| {
                if this.outline_flash_epoch != epoch {
                    return;
                }
                this.outline_flash_highlight_path = None;
                this.outline_flash_highlight_ranges.clear();
                this.outline_flash_highlight_current_index = None;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn collect_outline_headings(&self) -> Vec<OutlineHeading> {
        let mut headings = Vec::new();
        for (line_idx, dec) in self.decorations.iter().enumerate() {
            if let BlockType::Heading(level) = dec.block_type {
                let raw = self.buffer.line_content(line_idx);
                let text = raw.trim_start_matches('#').trim_start();
                headings.push(OutlineHeading {
                    line: line_idx,
                    level,
                    text: SharedString::from(text.to_string()),
                });
            }
        }
        headings
    }

    pub(crate) fn active_outline_heading_line(&self) -> Option<usize> {
        let cursor_line = self.cursor_line();
        let mut active_line = None;
        for (line_idx, dec) in self.decorations.iter().enumerate() {
            if let BlockType::Heading(_) = dec.block_type {
                if line_idx <= cursor_line {
                    active_line = Some(line_idx);
                } else {
                    break;
                }
            }
        }
        active_line
    }

    pub(crate) fn scroll_to_line(&mut self, target_line: usize, cx: &mut Context<Self>) {
        let target_y = self.line_y_offset(target_line);
        let target_scroll = (target_y - self.padding_top()).max(0.0);
        let line_end = self.buffer.line_end(target_line);
        self.cursor.offset = line_end;
        self.cursor.anchor = line_end;

        // Animated smooth scroll
        let start_scroll = self.scroll_offset;
        let delta = target_scroll - start_scroll;
        if delta.abs() < 1.0 {
            self.scroll_offset = target_scroll;
            self.clamp_scroll();
            cx.notify();
            return;
        }
        let total_steps = 20u32;
        let step_ms = 8u64;
        self.scroll_anim_epoch += 1;
        let epoch = self.scroll_anim_epoch;
        self.scroll_offset = target_scroll;
        self.clamp_scroll();
        let final_scroll = self.scroll_offset;
        self.scroll_offset = start_scroll;
        cx.notify();

        cx.spawn(async move |this, cx| {
            for step in 1..=total_steps {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(step_ms))
                    .await;
                let result = this.update(cx, |this, cx| {
                    if this.scroll_anim_epoch != epoch {
                        return false;
                    }
                    let t = step as f32 / total_steps as f32;
                    // ease-out cubic
                    let ease = 1.0 - (1.0 - t).powi(3);
                    this.scroll_offset = start_scroll + (final_scroll - start_scroll) * ease;
                    this.clamp_scroll();
                    cx.notify();
                    true
                });
                match result {
                    Ok(true) => {}
                    _ => break,
                }
            }
        })
        .detach();
    }

    pub(crate) fn render_outline(&self, cx: &mut Context<Self>) -> AnyElement {
        let os = &self.styles.outline;
        let headings = self.collect_outline_headings();
        let active_line = self.active_outline_heading_line();
        let header_height = self.scaled(os.outline_header_height_px);
        let item_height = self.scaled(os.outline_item_height_px);
        let indent_step = self.scaled(os.outline_item_indent_step_px);
        let padding_left = self.scaled(os.outline_item_padding_left_px);
        let padding_right = self.scaled(os.outline_item_padding_right_px);
        let font_size = self.scaled(os.outline_item_font_size_px);
        let item_line_height =
            (font_size * os.outline_item_line_height_factor).max(font_size + self.scaled(1.0));
        let item_padding_y =
            ((item_height - item_line_height).max(0.0) / 2.0).max(self.scaled(2.0));
        let header_font_size = self.scaled(os.outline_header_font_size_px);
        let text_color = os.outline_item_text_color;
        let active_text_color = os.outline_item_active_text_color;
        let active_bg = os.outline_item_active_bg;
        let hover_bg = os.outline_item_hover_bg;
        let header_text_color = os.outline_header_text_color;
        let header_padding_x = self.scaled(os.outline_header_padding_x_px);
        let top_spacer = self.scaled(self.styles.utility_panel.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(os.outline_bottom_spacer_height_px);

        let mut items: Vec<AnyElement> = Vec::with_capacity(headings.len() + 3);

        items.push(div().h(px(top_spacer)).flex_shrink_0().into_any_element());

        items.push(
            div()
                .h(px(header_height))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(px(header_padding_x))
                .text_size(px(header_font_size))
                .text_color(header_text_color)
                .child(self.t(crate::i18n::TextKey::Outline))
                .into_any_element(),
        );

        // Pre-compute which guide levels are active for each heading.
        // A guide at level L is active for heading[i] if there exists a later heading[j]
        // with level <= L+1 (i.e., it connects siblings at level L+1) and all headings
        // between i and j have level > L.
        for (idx, heading) in headings.iter().enumerate() {
            let is_active = active_line == Some(heading.line);
            let indent = padding_left + (heading.level.saturating_sub(1) as f32) * indent_step;
            let target_line = heading.line;
            let title = heading.text.clone();
            let item_text_color = if is_active {
                active_text_color
            } else {
                text_color
            };
            let item_bg = if is_active {
                active_bg
            } else {
                gpui::transparent_black()
            };

            // For each possible guide level (1..heading.level), check if there's a
            // later heading at that level (meaning the guide should continue through this item)
            let mut active_guide_levels = std::collections::HashSet::new();
            for guide_level in 1..heading.level {
                // Check if any later heading has level <= guide_level
                // (which means the guide at guide_level should continue)
                for future in &headings[idx + 1..] {
                    if future.level <= guide_level {
                        // A heading at or above this guide level exists later
                        active_guide_levels.insert(guide_level);
                        break;
                    }
                }
            }

            let guide_color = os.outline_indent_guide_color;
            let guide_width = os.outline_indent_guide_width_px;

            let guides: Vec<AnyElement> = (1..heading.level)
                .filter(|level| active_guide_levels.contains(level))
                .map(|level| {
                    let guide_x = padding_left + (level as f32 - 1.0) * indent_step;
                    div()
                        .absolute()
                        .left(px(guide_x))
                        .top(px(0.0))
                        .bottom(px(0.0))
                        .w(px(guide_width))
                        .bg(guide_color)
                        .into_any_element()
                })
                .collect();

            items.push(
                div()
                    .id(SharedString::from(format!("outline-{}", target_line)))
                    .min_h(px(item_height))
                    .w_full()
                    .flex_shrink_0()
                    .relative()
                    .pl(px(indent))
                    .pr(px(padding_right))
                    .py(px(item_padding_y))
                    .text_size(px(font_size))
                    .text_color(item_text_color)
                    .bg(item_bg)
                    .cursor(CursorStyle::Arrow)
                    .hover(move |s| s.bg(hover_bg))
                    .children(guides)
                    .child(
                        div()
                            .w_full()
                            .line_height(px(item_line_height))
                            .child(title),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.scroll_to_line(target_line, cx);
                            this.flash_outline_heading_line(target_line, cx);
                        }),
                    )
                    .into_any_element(),
            );
        }

        items.push(
            div()
                .h(px(bottom_spacer))
                .flex_shrink_0()
                .into_any_element(),
        );

        div()
            .id("outline-panel")
            .h_full()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.outline_scroll_handle)
            .bg(os.outline_bg)
            .border_l(px(self.scaled(os.outline_border_width_px)))
            .border_color(os.outline_border_color)
            .children(items)
            .into_any_element()
    }
}
