use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, ScrollWheelEvent, SharedString,
    Window, div, linear_color_stop, linear_gradient, prelude::*, px,
};

use crate::markdown::BlockType;

use super::*;

pub(crate) struct OutlineHeading {
    pub line: usize,
    pub level: u8,
    pub text: SharedString,
}

#[derive(Clone, Copy)]
struct OutlineRailRow {
    line: usize,
    level: u8,
}

fn centered_overflow_offset(
    viewport_height: f32,
    total_height: f32,
    active_center: Option<f32>,
    padding: f32,
) -> f32 {
    let padding = padding.min(viewport_height.max(0.0) / 2.0);
    let viewport_inner_height = (viewport_height - padding * 2.0).max(0.0);
    if total_height <= viewport_inner_height {
        return padding + (viewport_inner_height - total_height).max(0.0) / 2.0;
    }

    let desired =
        padding + viewport_inner_height / 2.0 - active_center.unwrap_or(total_height / 2.0);
    desired.clamp(padding + viewport_inner_height - total_height, padding)
}

fn overflow_max_scroll(viewport_height: f32, total_height: f32, padding: f32) -> f32 {
    let padding = padding.min(viewport_height.max(0.0) / 2.0);
    let viewport_inner_height = (viewport_height - padding * 2.0).max(0.0);
    (total_height - viewport_inner_height).max(0.0)
}

fn overflow_offset_from_scroll(scroll_offset: f32, viewport_height: f32, padding: f32) -> f32 {
    let padding = padding.min(viewport_height.max(0.0) / 2.0);
    padding - scroll_offset.max(0.0)
}

fn overflow_offset_from_shared_scroll(
    shared_scroll: Option<f32>,
    shared_max_scroll: f32,
    content_max_scroll: f32,
    viewport_height: f32,
    padding: f32,
) -> Option<f32> {
    if shared_max_scroll <= 0.0 || content_max_scroll <= 0.0 {
        return None;
    }

    let shared_scroll = shared_scroll?.clamp(0.0, shared_max_scroll);
    let content_scroll = shared_scroll * (content_max_scroll / shared_max_scroll);
    Some(overflow_offset_from_scroll(
        content_scroll,
        viewport_height,
        padding,
    ))
}

fn overflow_scroll_from_offset(top_offset: f32, viewport_height: f32, padding: f32) -> f32 {
    let padding = padding.min(viewport_height.max(0.0) / 2.0);
    (padding - top_offset).max(0.0)
}

fn outline_label_line_count(text: &str, chars_per_line: usize, max_lines: usize) -> usize {
    let chars_per_line = chars_per_line.max(1);
    let max_lines = max_lines.max(1);
    let char_count = text.chars().count().max(1);
    char_count.div_ceil(chars_per_line).clamp(1, max_lines)
}

fn outline_label_chars_per_line(max_width: f32, font_size: f32) -> usize {
    ((max_width / (font_size.max(1.0) * 0.82)).floor() as usize).max(1)
}

fn outline_label_metrics(
    headings: &[OutlineHeading],
    active_line: Option<usize>,
    item_height: f32,
    item_gap: f32,
    item_padding_y: f32,
    line_height: f32,
    chars_per_line: usize,
    max_lines: usize,
) -> (f32, Option<f32>) {
    let mut total_height = 0.0;
    let mut active_center = None;
    for (index, heading) in headings.iter().enumerate() {
        if index > 0 {
            total_height += item_gap;
        }
        let line_count = outline_label_line_count(&heading.text, chars_per_line, max_lines);
        let current_height =
            item_height.max(line_count as f32 * line_height + item_padding_y * 2.0);
        if Some(heading.line) == active_line {
            active_center = Some(total_height + current_height / 2.0);
        }
        total_height += current_height;
    }
    (total_height, active_center)
}

fn mix_hsla(from: gpui::Hsla, to: gpui::Hsla, factor: f32) -> gpui::Hsla {
    let factor = factor.clamp(0.0, 1.0);
    let inverse = 1.0 - factor;
    gpui::Hsla {
        h: from.h * inverse + to.h * factor,
        s: from.s * inverse + to.s * factor,
        l: from.l * inverse + to.l * factor,
        a: from.a * inverse + to.a * factor,
    }
}

fn multiply_alpha(color: gpui::Hsla, factor: f32) -> gpui::Hsla {
    gpui::Hsla {
        a: color.a * factor.clamp(0.0, 1.0),
        ..color
    }
}

fn heading_level_opacity(level: u8, level_opacities: [f32; 6]) -> f32 {
    let index = level.saturating_sub(1).min(5) as usize;
    level_opacities[index].clamp(0.0, 1.0)
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

    /// The first heading visible in the editor viewport, falling back to the
    /// section heading scrolled above the viewport top.
    fn first_visible_outline_heading_line(&self, headings: &[OutlineHeading]) -> Option<usize> {
        let top = self.scroll_offset;
        let bottom = top + self.last_viewport_height.max(0.0);
        let mut fallback = None;
        for heading in headings {
            let y = self.line_y_offset(heading.line);
            if y < top {
                fallback = Some(heading.line);
            } else if y < bottom {
                return Some(heading.line);
            } else {
                break;
            }
        }
        fallback.or_else(|| headings.first().map(|heading| heading.line))
    }

    fn outline_rail_rows(&self, headings: &[OutlineHeading]) -> Vec<OutlineRailRow> {
        headings
            .iter()
            .map(|heading| OutlineRailRow {
                line: heading.line,
                level: heading.level,
            })
            .collect()
    }

    fn outline_rail_active_line(&self, headings: &[OutlineHeading]) -> Option<usize> {
        self.outline_rail_clicked_active_line
            .filter(|clicked_line| headings.iter().any(|heading| heading.line == *clicked_line))
            .or_else(|| self.first_visible_outline_heading_line(headings))
    }

    pub(crate) fn sync_outline_rail_label_opacity(&mut self, cx: &mut Context<Self>) {
        let target = if self.outline_rail_hovered
            || (self.editor_scrollbar_hovered && self.outline_rail_label_opacity > 0.01)
        {
            1.0
        } else {
            0.0
        };
        if (self.outline_rail_label_opacity_target - target).abs() <= 0.001 {
            return;
        }

        self.outline_rail_label_opacity_target = target;
        self.outline_rail_label_opacity_epoch += 1;
        let epoch = self.outline_rail_label_opacity_epoch;
        let duration_ms = self
            .styles
            .outline
            .outline_rail_label_opacity_animation_ms
            .max(1);
        let step = (16.0 / duration_ms as f32).clamp(0.08, 1.0);

        self.outline_rail_label_opacity_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        if this.outline_rail_label_opacity_epoch != epoch {
                            return true;
                        }

                        let diff = this.outline_rail_label_opacity_target
                            - this.outline_rail_label_opacity;
                        if diff.abs() <= 0.01 {
                            this.outline_rail_label_opacity =
                                this.outline_rail_label_opacity_target;
                            this.outline_rail_label_opacity_task = None;
                            cx.notify();
                            return true;
                        }

                        this.outline_rail_label_opacity += diff * step;
                        cx.notify();
                        false
                    })
                    .unwrap_or(true);

                if done {
                    return;
                }

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
        }));
    }

    fn set_outline_rail_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.outline_rail_hovered == hovered {
            return;
        }

        self.outline_rail_hovered = hovered;
        if !hovered {
            self.set_outline_rail_hovered_label_line(None, cx);
        }
        self.sync_outline_rail_label_opacity(cx);
        cx.notify();
    }

    fn sync_outline_rail_label_hover_opacity(&mut self, cx: &mut Context<Self>) {
        if (self.outline_rail_hovered_label_opacity_target
            - self.outline_rail_hovered_label_opacity)
            .abs()
            <= 0.001
        {
            return;
        }

        self.outline_rail_hovered_label_opacity_epoch += 1;
        let epoch = self.outline_rail_hovered_label_opacity_epoch;
        let duration_ms = self
            .styles
            .outline
            .outline_rail_label_hover_animation_ms
            .max(1);
        let step = (16.0 / duration_ms as f32).clamp(0.08, 1.0);

        self.outline_rail_hovered_label_opacity_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        if this.outline_rail_hovered_label_opacity_epoch != epoch {
                            return true;
                        }

                        let diff = this.outline_rail_hovered_label_opacity_target
                            - this.outline_rail_hovered_label_opacity;
                        if diff.abs() <= 0.01 {
                            this.outline_rail_hovered_label_opacity =
                                this.outline_rail_hovered_label_opacity_target;
                            if this.outline_rail_hovered_label_opacity <= 0.01 {
                                this.outline_rail_hovered_label_line = None;
                            }
                            this.outline_rail_hovered_label_opacity_task = None;
                            cx.notify();
                            return true;
                        }

                        this.outline_rail_hovered_label_opacity += diff * step;
                        cx.notify();
                        false
                    })
                    .unwrap_or(true);

                if done {
                    return;
                }

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
        }));
    }

    fn set_outline_rail_hovered_label_line(&mut self, line: Option<usize>, cx: &mut Context<Self>) {
        if let Some(line) = line {
            if self.outline_rail_hovered_label_line != Some(line) {
                self.outline_rail_hovered_label_line = Some(line);
                self.outline_rail_hovered_label_opacity = 0.0;
            }
            self.outline_rail_hovered_label_opacity_target = 1.0;
        } else {
            self.outline_rail_hovered_label_opacity_target = 0.0;
        }
        self.sync_outline_rail_label_hover_opacity(cx);
        cx.notify();
    }

    fn on_outline_rail_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let headings = self.collect_outline_headings();
        if headings.is_empty() {
            return;
        }

        let os = &self.styles.outline;
        let active_line = self.outline_rail_active_line(&headings);
        let viewport_height = self.last_viewport_height.max(0.0);
        let vertical_padding = self.scaled(os.outline_rail_vertical_padding_px);
        let row_height = self.scaled(os.outline_rail_row_height_px);
        let tick_total_height = headings.len() as f32 * row_height;
        let tick_max_scroll =
            overflow_max_scroll(viewport_height, tick_total_height, vertical_padding);
        let mut max_scroll = tick_max_scroll;
        let mut label_max_scroll = 0.0;
        let mut label_initial_scroll = None;

        if self.outline_rail_hovered || self.outline_rail_label_opacity > 0.01 {
            let label_wrap_chars_per_line = outline_label_chars_per_line(
                self.scaled(os.outline_rail_label_max_width_px),
                self.scaled(os.outline_rail_label_font_size_px),
            )
            .min(os.outline_rail_label_wrap_chars_per_line.max(1));
            let (label_total_height, active_label_center) = outline_label_metrics(
                &headings,
                active_line,
                self.scaled(os.outline_rail_label_item_height_px),
                self.scaled(os.outline_rail_label_item_gap_px),
                self.scaled(os.outline_rail_label_item_padding_y_px),
                self.scaled(os.outline_rail_label_line_height_px),
                label_wrap_chars_per_line,
                os.outline_rail_label_max_lines,
            );
            label_max_scroll =
                overflow_max_scroll(viewport_height, label_total_height, vertical_padding);
            if label_max_scroll > 0.0 {
                let label_top_offset = centered_overflow_offset(
                    viewport_height,
                    label_total_height,
                    active_label_center,
                    vertical_padding,
                );
                label_initial_scroll = Some(overflow_scroll_from_offset(
                    label_top_offset,
                    viewport_height,
                    vertical_padding,
                ));
            }
            max_scroll = max_scroll.max(label_max_scroll);
        }

        if max_scroll <= 0.0 {
            return;
        }

        let current_scroll = self.outline_rail_scroll_offset.unwrap_or_else(|| {
            if let Some(label_initial_scroll) = label_initial_scroll {
                return if label_max_scroll > 0.0 {
                    label_initial_scroll / label_max_scroll * max_scroll
                } else {
                    label_initial_scroll
                };
            }
            let active_index = active_line
                .and_then(|line| headings.iter().position(|heading| heading.line == line));
            let active_center =
                active_index.map(|index| index as f32 * row_height + row_height / 2.0);
            let top_offset = centered_overflow_offset(
                viewport_height,
                tick_total_height,
                active_center,
                vertical_padding,
            );
            let tick_initial_scroll =
                overflow_scroll_from_offset(top_offset, viewport_height, vertical_padding);
            if tick_max_scroll > 0.0 {
                tick_initial_scroll / tick_max_scroll * max_scroll
            } else {
                tick_initial_scroll
            }
        });
        let delta_y: f32 = event.delta.pixel_delta(px(row_height * 6.0)).y.into();
        self.outline_rail_scroll_offset = Some((current_scroll - delta_y).clamp(0.0, max_scroll));
        cx.stop_propagation();
        cx.notify();
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

    pub(crate) fn render_outline_rail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.outline_rail_visible || !self.active_tab_is_markdown() {
            return None;
        }
        let headings = self.collect_outline_headings();
        if headings.is_empty() {
            return None;
        }

        let rows = self.outline_rail_rows(&headings);
        let active_line = self.outline_rail_active_line(&headings);

        let os = &self.styles.outline;
        let right_margin = self.scaled(os.outline_rail_right_margin_px);
        let scrollbar_hit_left_from_right = self.scaled(
            self.styles.editor.editor_scrollbar_right_margin_px
                + self
                    .styles
                    .editor
                    .editor_scrollbar_hit_width_px
                    .max(self.styles.editor.editor_scrollbar_width_px)
                    .max(self.styles.editor.editor_scrollbar_hover_width_px),
        );
        let track_right = scrollbar_hit_left_from_right.min(right_margin);
        let track_extra_right_padding = (right_margin - track_right).max(0.0);
        let vertical_padding = self.scaled(os.outline_rail_vertical_padding_px);
        let label_font_size = self.scaled(os.outline_rail_label_font_size_px);
        let label_max_width = self.scaled(os.outline_rail_label_max_width_px);
        let label_gap = self.scaled(os.outline_rail_label_gap_px);
        let label_item_height = self.scaled(os.outline_rail_label_item_height_px);
        let label_item_gap = self.scaled(os.outline_rail_label_item_gap_px);
        let label_item_padding_y = self.scaled(os.outline_rail_label_item_padding_y_px);
        let label_line_height = self.scaled(os.outline_rail_label_line_height_px);
        let label_wrap_chars_per_line =
            outline_label_chars_per_line(label_max_width, label_font_size)
                .min(os.outline_rail_label_wrap_chars_per_line.max(1));
        let row_height = self.scaled(os.outline_rail_row_height_px);
        let tick_width = self.scaled(os.outline_rail_tick_width_px);
        let tick_thickness = self
            .scaled(os.outline_rail_tick_thickness_px)
            .round()
            .max(1.0);
        let tick_color = os.outline_rail_tick_color;
        let tick_active_color = os.outline_rail_tick_active_color;
        let label_color = os.outline_rail_label_color;
        let label_hover_color = os.outline_rail_label_hover_color;
        let label_active_color = os.outline_rail_label_active_color;
        let tick_level_opacities = os.outline_rail_tick_level_opacities;
        let label_level_opacities = os.outline_rail_label_level_opacities;
        let hovered_label_line = self.outline_rail_hovered_label_line;
        let hovered_label_opacity = self.outline_rail_hovered_label_opacity.clamp(0.0, 1.0);
        let label_opacity = self.outline_rail_label_opacity.clamp(0.0, 1.0);
        let labels_visible = self.outline_rail_hovered || label_opacity > 0.01;
        let active_extra_width = self.scaled(os.outline_rail_tick_active_extra_width_px);
        let active_anim_seconds = os.outline_rail_tick_active_animation_seconds.max(0.001);

        let previous_active_line = self.outline_rail_last_active_line.get();
        if previous_active_line != active_line {
            self.outline_rail_last_active_line.set(active_line);
            self.outline_rail_active_since
                .set(active_line.map(|_| std::time::Instant::now()));
            let target_active_line = active_line;
            cx.spawn(async move |this, cx| {
                for _ in 0..14 {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                    let keep = this
                        .update(cx, |this, cx| {
                            if this.outline_rail_last_active_line.get() != target_active_line {
                                return false;
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            })
            .detach();
        }

        // Measure the rail to decide between centering and active-centered clipping.
        let viewport_height = self.last_viewport_height.max(0.0);
        let mut total_height = 0.0;
        let mut active_row_center = None;
        for row in &rows {
            if Some(row.line) == active_line {
                active_row_center = Some(total_height + row_height / 2.0);
            }
            total_height += row_height;
        }
        let overflow = viewport_height > 0.0 && total_height > viewport_height;
        let top_offset = if overflow {
            let max_scroll = overflow_max_scroll(viewport_height, total_height, vertical_padding);
            let label_max_scroll = if labels_visible {
                let (label_total_height, _) = outline_label_metrics(
                    &headings,
                    active_line,
                    label_item_height,
                    label_item_gap,
                    label_item_padding_y,
                    label_line_height,
                    label_wrap_chars_per_line,
                    os.outline_rail_label_max_lines,
                );
                overflow_max_scroll(viewport_height, label_total_height, vertical_padding)
            } else {
                0.0
            };
            let shared_max_scroll = max_scroll.max(label_max_scroll);
            if let Some(top_offset) = overflow_offset_from_shared_scroll(
                self.outline_rail_scroll_offset,
                shared_max_scroll,
                max_scroll,
                viewport_height,
                vertical_padding,
            ) {
                top_offset
            } else {
                centered_overflow_offset(
                    viewport_height,
                    total_height,
                    active_row_center,
                    vertical_padding,
                )
            }
        } else {
            0.0
        };

        let mut row_elements: Vec<AnyElement> = Vec::with_capacity(rows.len());
        for row in rows {
            let line = row.line;
            let level_opacity = heading_level_opacity(row.level, tick_level_opacities);
            let is_active = Some(line) == active_line;
            let animated_tick_width = if is_active {
                let progress = self
                    .outline_rail_active_since
                    .get()
                    .map(|started| {
                        (started.elapsed().as_secs_f32() / active_anim_seconds).clamp(0.0, 1.0)
                    })
                    .unwrap_or(1.0);
                let ease = super::ease_in_out_cubic(progress);
                tick_width + active_extra_width * ease
            } else {
                tick_width
            };
            let row_tick_color = if is_active {
                tick_active_color
            } else {
                multiply_alpha(tick_color, level_opacity)
            };
            row_elements.push(
                div()
                    .id(SharedString::from(format!("outline-rail-{}", line)))
                    .h(px(row_height))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .cursor(CursorStyle::Arrow)
                    .child(
                        div()
                            .w(px(tick_width + active_extra_width))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(
                                div()
                                    .w(px(animated_tick_width))
                                    .h(px(tick_thickness))
                                    .rounded_full()
                                    .bg(row_tick_color),
                            ),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.outline_rail_clicked_active_line = Some(line);
                            this.scroll_to_line(line, cx);
                            this.flash_outline_heading_line(line, cx);
                        }),
                    )
                    .into_any_element(),
            );
        }

        let rail_collapsed_width = tick_width + active_extra_width;
        let rail_expanded_width = rail_collapsed_width + label_gap + label_max_width;
        let rail_width = if labels_visible {
            rail_expanded_width
        } else {
            rail_collapsed_width
        };
        let rail_outer_width = rail_width + right_margin;
        let track_width = rail_width + track_extra_right_padding;

        let label_overlay = if labels_visible {
            let mut label_items: Vec<AnyElement> = Vec::with_capacity(headings.len());
            let (label_total_height, active_label_center) = outline_label_metrics(
                &headings,
                active_line,
                label_item_height,
                label_item_gap,
                label_item_padding_y,
                label_line_height,
                label_wrap_chars_per_line,
                os.outline_rail_label_max_lines,
            );
            for heading in &headings {
                let target_line = heading.line;
                let level_opacity = heading_level_opacity(heading.level, label_level_opacities);
                let is_active = Some(target_line) == active_line;
                let is_hovered = hovered_label_line == Some(target_line);
                let base_label_color = if is_active {
                    label_active_color
                } else {
                    multiply_alpha(label_color, level_opacity)
                };
                let row_label_color = if is_hovered {
                    mix_hsla(base_label_color, label_hover_color, hovered_label_opacity)
                } else {
                    base_label_color
                };
                let label_line_count = outline_label_line_count(
                    heading.text.as_ref(),
                    label_wrap_chars_per_line,
                    os.outline_rail_label_max_lines,
                );
                let label_text_max_height = label_line_count as f32 * label_line_height;
                let current_label_item_height =
                    label_item_height.max(label_text_max_height + label_item_padding_y * 2.0);
                label_items.push(
                    div()
                        .id(SharedString::from(format!(
                            "outline-rail-label-{}",
                            target_line
                        )))
                        .h(px(current_label_item_height))
                        .w_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_end()
                        .overflow_hidden()
                        .py(px(label_item_padding_y))
                        .text_size(px(label_font_size))
                        .text_right()
                        .text_color(row_label_color)
                        .cursor(CursorStyle::Arrow)
                        .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                            if *hovering {
                                this.set_outline_rail_hovered_label_line(Some(target_line), cx);
                            } else if this.outline_rail_hovered_label_line == Some(target_line) {
                                this.set_outline_rail_hovered_label_line(None, cx);
                            }
                        }))
                        .child(
                            div()
                                .max_w(px(label_max_width))
                                .max_h(px(label_text_max_height))
                                .overflow_hidden()
                                .line_height(px(label_line_height))
                                .whitespace_normal()
                                .text_right()
                                .child(heading.text.clone()),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.outline_rail_clicked_active_line = Some(target_line);
                                this.scroll_to_line(target_line, cx);
                                this.flash_outline_heading_line(target_line, cx);
                            }),
                        )
                        .into_any_element(),
                );
            }

            let label_overflow = viewport_height > 0.0 && label_total_height > viewport_height;
            let label_top_offset = if label_overflow {
                let max_scroll =
                    overflow_max_scroll(viewport_height, label_total_height, vertical_padding);
                let tick_max_scroll =
                    overflow_max_scroll(viewport_height, total_height, vertical_padding);
                let shared_max_scroll = max_scroll.max(tick_max_scroll);
                if let Some(top_offset) = overflow_offset_from_shared_scroll(
                    self.outline_rail_scroll_offset,
                    shared_max_scroll,
                    max_scroll,
                    viewport_height,
                    vertical_padding,
                ) {
                    top_offset
                } else {
                    centered_overflow_offset(
                        viewport_height,
                        label_total_height,
                        active_label_center,
                        vertical_padding,
                    )
                }
            } else {
                0.0
            };

            let label_column = div()
                .max_w(px(label_max_width))
                .flex()
                .flex_col()
                .items_end()
                .gap(px(label_item_gap))
                .children(label_items);
            let label_right = rail_collapsed_width + label_gap;
            let overlay = div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(label_right))
                .w(px(label_max_width))
                .overflow_hidden()
                .opacity(label_opacity);
            Some(if label_overflow {
                overlay
                    .child(
                        label_column
                            .absolute()
                            .top(px(label_top_offset))
                            .right(px(0.0)),
                    )
                    .into_any_element()
            } else {
                overlay
                    .flex()
                    .flex_col()
                    .justify_center()
                    .items_end()
                    .child(label_column)
                    .into_any_element()
            })
        } else {
            None
        };

        let column = div()
            .w(px(rail_width))
            .flex()
            .flex_col()
            .items_end()
            .children(row_elements);

        let content = div()
            .id("outline-rail-content")
            .relative()
            .h_full()
            .w_full()
            .overflow_hidden()
            .when(labels_visible, |this| {
                let gradient_start_color = gpui::Hsla {
                    a: os.outline_rail_label_gradient_start_color.a * label_opacity,
                    ..os.outline_rail_label_gradient_start_color
                };
                let gradient_end_color = gpui::Hsla {
                    a: os.outline_rail_label_gradient_end_color.a * label_opacity,
                    ..os.outline_rail_label_gradient_end_color
                };
                this.bg(linear_gradient(
                    os.outline_rail_label_gradient_angle_deg,
                    linear_color_stop(
                        gradient_start_color,
                        os.outline_rail_label_gradient_start_stop,
                    ),
                    linear_color_stop(gradient_end_color, os.outline_rail_label_gradient_end_stop),
                ))
            });

        let track_inner = div()
            .id("outline-rail-track-inner")
            .relative()
            .h_full()
            .w_full()
            .overflow_hidden();

        let track_inner = if overflow {
            track_inner.child(column.absolute().top(px(top_offset)).right(px(0.0)))
        } else {
            track_inner
                .flex()
                .flex_col()
                .justify_center()
                .items_end()
                .child(column)
        };
        let track_inner = if let Some(label_overlay) = label_overlay {
            track_inner.child(label_overlay)
        } else {
            track_inner
        };

        let track = div()
            .id("outline-rail-track")
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(track_right))
            .w(px(track_width))
            .occlude()
            .cursor(CursorStyle::Arrow)
            .on_scroll_wheel(cx.listener(Self::on_outline_rail_scroll))
            .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                this.set_outline_rail_hovered(*hovering, cx);
            }))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(track_extra_right_padding))
                    .w(px(rail_width))
                    .child(track_inner),
            );
        let content = content.child(track);

        Some(
            div()
                .id("outline-rail")
                .absolute()
                .top_0()
                .right(px(0.0))
                .h_full()
                .w(px(rail_outer_width))
                .child(content)
                .into_any_element(),
        )
    }
}
