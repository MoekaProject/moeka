use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, ScrollWheelEvent, SharedString,
    Window, div, prelude::*, px, svg,
};
use std::cell::Ref;

use crate::markdown::BlockType;

use super::*;

#[derive(Clone)]
pub(crate) struct OutlineHeading {
    pub line: usize,
    pub level: u8,
    pub text: SharedString,
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

fn outline_text_columns(text: &str) -> usize {
    text.chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum()
}

fn outline_heading_text_width(text: &str, font_size: f32) -> f32 {
    let columns = outline_text_columns(text);
    columns.max(1) as f32 * font_size.max(1.0) * 0.56 + font_size.max(1.0) * 0.35
}

fn outline_chars_per_line(available_width: f32, font_size: f32, fallback: usize) -> usize {
    let char_width = font_size.max(1.0) * 0.56;
    if available_width.is_finite() && available_width > char_width {
        (available_width / char_width).floor().max(8.0) as usize
    } else {
        fallback.max(8)
    }
}

fn outline_heading_line_count(text: &str, chars_per_line: usize) -> usize {
    let columns = outline_text_columns(text).max(1);
    columns.div_ceil(chars_per_line.max(1)).max(1)
}

fn outline_soft_wrappable_text(text: &str, chars_per_line: usize) -> SharedString {
    let break_after_columns = chars_per_line.max(8);
    let mut out = String::with_capacity(text.len());
    let mut run_columns = 0usize;
    let mut changed = false;
    for ch in text.chars() {
        out.push(ch);
        if ch.is_whitespace()
            || matches!(
                ch,
                '-' | '_' | '/' | '\\' | '.' | ':' | ',' | ';' | '，' | '、' | '。'
            )
        {
            run_columns = 0;
            continue;
        }
        run_columns += if ch.is_ascii() { 1 } else { 2 };
        if run_columns >= break_after_columns {
            out.push('\u{200b}');
            run_columns = 0;
            changed = true;
        }
    }
    if changed {
        SharedString::from(out)
    } else {
        SharedString::from(text.to_string())
    }
}

fn normalized_outline_width_bounds(min_width: f32, max_width: f32) -> (f32, f32) {
    let min_width = if min_width.is_finite() {
        min_width.max(0.0)
    } else {
        0.0
    };
    let max_width = if max_width.is_finite() {
        max_width.max(0.0)
    } else {
        min_width
    };
    (min_width.min(max_width), min_width.max(max_width))
}

#[cfg(test)]
mod tests {
    use super::{
        normalized_outline_width_bounds, outline_heading_line_count, outline_soft_wrappable_text,
    };

    #[test]
    fn outline_width_bounds_are_normalized_before_clamp() {
        assert_eq!(
            normalized_outline_width_bounds(324.0, 316.0),
            (316.0, 324.0)
        );
        assert_eq!(
            normalized_outline_width_bounds(f32::NAN, 316.0),
            (0.0, 316.0)
        );
        assert_eq!(
            normalized_outline_width_bounds(324.0, f32::NAN),
            (324.0, 324.0)
        );
    }

    #[test]
    fn outline_heading_line_count_tracks_visual_columns() {
        assert_eq!(outline_heading_line_count("short", 24), 1);
        assert_eq!(
            outline_heading_line_count("abcdefghijklmnopqrstuvwxyz", 12),
            3
        );
        assert_eq!(outline_heading_line_count("中文中文中文", 6), 2);
    }

    #[test]
    fn outline_soft_wrap_inserts_breaks_for_long_runs() {
        let wrapped = outline_soft_wrappable_text("abcdefghijklmnopqrstuvwxyz", 8);
        assert!(wrapped.as_ref().contains('\u{200b}'));
    }
}

fn visible_outline_heading_indices_with_depths(
    headings: &[OutlineHeading],
    collapsed_lines: &std::collections::HashSet<usize>,
) -> Vec<(usize, usize)> {
    let mut visible = Vec::new();
    let mut level_stack: Vec<(u8, bool)> = Vec::new();
    for (index, heading) in headings.iter().enumerate() {
        while level_stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            level_stack.pop();
        }
        let hidden_by_ancestor = level_stack.last().is_some_and(|(_, hidden)| *hidden);
        if !hidden_by_ancestor {
            let depth = level_stack.iter().filter(|(_, hidden)| !*hidden).count();
            visible.push((index, depth));
        }
        level_stack.push((
            heading.level,
            hidden_by_ancestor || collapsed_lines.contains(&heading.line),
        ));
    }
    visible
}

fn outline_heading_has_children(headings: &[OutlineHeading], heading_index: usize) -> bool {
    let Some(current) = headings.get(heading_index) else {
        return false;
    };
    headings
        .get(heading_index + 1)
        .is_some_and(|next| next.level > current.level)
}

fn multiply_alpha(color: gpui::Hsla, factor: f32) -> gpui::Hsla {
    gpui::Hsla {
        a: color.a * factor.clamp(0.0, 1.0),
        ..color
    }
}

fn lerp_hsla(from: gpui::Hsla, to: gpui::Hsla, t: f32) -> gpui::Hsla {
    let t = t.clamp(0.0, 1.0);
    gpui::Hsla {
        h: from.h + (to.h - from.h) * t,
        s: from.s + (to.s - from.s) * t,
        l: from.l + (to.l - from.l) * t,
        a: from.a + (to.a - from.a) * t,
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

    pub(crate) fn collect_outline_headings(&self) -> Ref<'_, Vec<OutlineHeading>> {
        if self.outline_headings_cache_dirty.get() {
            {
                let mut headings = self.outline_headings_cache.borrow_mut();
                headings.clear();
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
            }
            self.outline_headings_cache_dirty.set(false);
        }
        self.outline_headings_cache.borrow()
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

    fn outline_rail_active_line(&self, headings: &[OutlineHeading]) -> Option<usize> {
        self.outline_rail_clicked_active_line
            .filter(|clicked_line| headings.iter().any(|heading| heading.line == *clicked_line))
            .or_else(|| self.first_visible_outline_heading_line(headings))
    }

    fn set_outline_rail_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let rail_changed = self.outline_rail_hovered != hovered;
        self.outline_rail_hovered = hovered;
        if hovered {
            // Cancel any pending toolbar auto-hide that was scheduled while the
            // cursor was still over the content. Entering the rail does not
            // re-run the editor's corner-hover check (the rail occludes it), so
            // without this the queued hide fires ~2s later and hides the toolbar
            // even though the cursor is on the rail.
            self.toolbar_corner_hide_task = None;
        }
        if rail_changed && hovered {
            // Start the hover highlight fade-in animation.
            self.outline_rail_hover_since
                .set(Some(std::time::Instant::now()));
            let anim_seconds = self
                .styles
                .outline
                .outline_rail_hover_animation_seconds
                .max(0.001);
            cx.spawn(async move |this, cx| {
                let frames = ((anim_seconds / 0.016).ceil() as usize).max(1) + 1;
                for _ in 0..frames {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                    let keep = this
                        .update(cx, |this, cx| {
                            if !this.outline_rail_hovered {
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
        let toolbar_changed = self.toolbar_corner_hovered != hovered;
        if self.toolbar_corner_hovered != hovered {
            self.toolbar_corner_hovered = hovered;
            self.sync_toolbar_opacity(cx);
        }
        if rail_changed || toolbar_changed {
            cx.notify();
        }
    }

    fn toggle_outline_rail_heading_collapsed(&mut self, line: usize, cx: &mut Context<Self>) {
        if !self.outline_rail_collapsed_heading_lines.remove(&line) {
            self.outline_rail_collapsed_heading_lines.insert(line);
        }
        cx.notify();
    }

    fn on_outline_rail_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let os = &self.styles.outline;
        let viewport_height = self.last_viewport_height.max(0.0);
        let vertical_padding = self.scaled(os.outline_rail_vertical_padding_px);
        let row_height = self.scaled(os.outline_rail_row_height_px);
        let Some((current_scroll, max_scroll)) = ({
            let headings = self.collect_outline_headings();
            if headings.is_empty() {
                None
            } else {
                let active_line = self.outline_rail_active_line(&headings);
                let tick_total_height = headings.len() as f32 * row_height;
                let tick_max_scroll =
                    overflow_max_scroll(viewport_height, tick_total_height, vertical_padding);
                let max_scroll = tick_max_scroll;
                if max_scroll <= 0.0 {
                    None
                } else {
                    Some((
                        self.outline_rail_scroll_offset.unwrap_or_else(|| {
                            let active_index = active_line.and_then(|line| {
                                headings.iter().position(|heading| heading.line == line)
                            });
                            let active_center = active_index
                                .map(|index| index as f32 * row_height + row_height / 2.0);
                            let top_offset = centered_overflow_offset(
                                viewport_height,
                                tick_total_height,
                                active_center,
                                vertical_padding,
                            );
                            let tick_initial_scroll = overflow_scroll_from_offset(
                                top_offset,
                                viewport_height,
                                vertical_padding,
                            );
                            if tick_max_scroll > 0.0 {
                                tick_initial_scroll / tick_max_scroll * max_scroll
                            } else {
                                tick_initial_scroll
                            }
                        }),
                        max_scroll,
                    ))
                }
            }
        }) else {
            return;
        };
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
            self.outline_rail_zone_from_right.set(0.0);
            return None;
        }
        let headings = self.collect_outline_headings();
        if headings.is_empty() {
            self.outline_rail_zone_from_right.set(0.0);
            return None;
        }

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
        let row_height = self.scaled(os.outline_rail_row_height_px);
        let tick_width = self.scaled(os.outline_rail_tick_width_px);
        let tick_thickness = self
            .scaled(os.outline_rail_tick_thickness_px)
            .round()
            .max(1.0);
        let tick_color = os.outline_rail_tick_color;
        let tick_active_color = os.outline_rail_tick_active_color;
        let tick_level_opacities = os.outline_rail_tick_level_opacities;
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
        let row_count = headings.len();
        let total_height = row_count as f32 * row_height;
        let active_row_center = active_line
            .and_then(|line| headings.iter().position(|heading| heading.line == line))
            .map(|index| index as f32 * row_height + row_height / 2.0);
        let overflow = viewport_height > 0.0 && total_height > viewport_height;
        let top_offset = if overflow {
            let max_scroll = overflow_max_scroll(viewport_height, total_height, vertical_padding);
            let shared_max_scroll = max_scroll;
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

        // Top of the tick column relative to the rail track (which spans the
        // visible viewport): the absolute scroll offset when overflowing, else
        // the vertically-centred position.
        let column_top = if overflow {
            top_offset
        } else {
            ((viewport_height - total_height) / 2.0).max(0.0)
        };
        let overscan_rows = 8usize;
        let (visible_start, visible_end) = if overflow {
            let first = ((-column_top) / row_height).floor().max(0.0) as usize;
            let last = ((viewport_height - column_top) / row_height)
                .ceil()
                .max(0.0) as usize;
            (
                first.saturating_sub(overscan_rows).min(row_count),
                last.saturating_add(overscan_rows).min(row_count),
            )
        } else {
            (0, row_count)
        };
        let mut row_elements: Vec<AnyElement> =
            Vec::with_capacity(visible_end.saturating_sub(visible_start) + 2);
        if visible_start > 0 {
            row_elements.push(
                div()
                    .h(px(visible_start as f32 * row_height))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }
        let edge_fade = self.scaled(os.outline_rail_edge_fade_px).max(1.0);
        let rail_hovered = self.outline_rail_hovered;
        // Hover highlight fade-in progress (0..1), eased.
        let hover_progress = if rail_hovered {
            self.outline_rail_hover_since
                .get()
                .map(|started| {
                    (started.elapsed().as_secs_f32()
                        / os.outline_rail_hover_animation_seconds.max(0.001))
                    .clamp(0.0, 1.0)
                })
                .unwrap_or(1.0)
        } else {
            0.0
        };
        let hover_ease = super::ease_in_out_cubic(hover_progress);
        for row_index in visible_start..visible_end {
            let heading = &headings[row_index];
            let line = heading.line;
            let level_opacity = heading_level_opacity(heading.level, tick_level_opacities);
            let is_active = Some(line) == active_line;
            // Only the active tick animates its width; hovering brightens every
            // tick uniformly rather than singling one out and lengthening it.
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
            // Fade ticks within `edge_fade` of the top and bottom edges of the
            // visible rail viewport, giving a soft gradient at the viewport edges
            // (off-screen ticks clamp to fully transparent).
            let row_center_y = column_top + row_index as f32 * row_height + row_height / 2.0;
            let edge_opacity = if viewport_height > 0.0 {
                (row_center_y.min(viewport_height - row_center_y) / edge_fade).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let resting_color = multiply_alpha(tick_color, level_opacity);
            let base_tick_color = if is_active {
                tick_active_color
            } else {
                // Animate from the resting colour to the active colour on hover.
                lerp_hsla(resting_color, tick_active_color, hover_ease)
            };
            let row_tick_color = multiply_alpha(base_tick_color, edge_opacity);
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
                                    .id(SharedString::from(format!(
                                        "outline-rail-tick-hit-{}",
                                        line
                                    )))
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
                            this.toggle_outline_panel(cx);
                        }),
                    )
                    .into_any_element(),
            );
        }
        if visible_end < row_count {
            row_elements.push(
                div()
                    .h(px((row_count - visible_end) as f32 * row_height))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }

        let rail_collapsed_width = tick_width + active_extra_width;
        let rail_width = rail_collapsed_width;
        let rail_outer_width = rail_width + right_margin;
        let track_width = rail_width + track_extra_right_padding;

        let column = div()
            .id("outline-rail-tick-column")
            .w(px(rail_collapsed_width))
            .flex()
            .flex_col()
            .items_end()
            .children(row_elements);

        let content = div()
            .id("outline-rail-content")
            .relative()
            .h_full()
            .w_full()
            .overflow_hidden();

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
        let track_inner = track_inner;

        let track_hover_width = if self.outline_rail_hovered {
            track_width
        } else {
            (tick_width + active_extra_width).max(tick_thickness) + track_extra_right_padding
        };

        // Record how far in from the right edge the rail's interactive zone
        // reaches this frame so the editor's toolbar-hover logic can keep the
        // toolbar visible while the rail is hovered, regardless of event
        // occlusion or content width.
        self.outline_rail_zone_from_right
            .set(track_right + track_width + right_margin);

        let track = div()
            .id("outline-rail-track")
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(track_right))
            .w(px(track_hover_width))
            .occlude()
            .cursor(CursorStyle::Arrow)
            .on_scroll_wheel(cx.listener(Self::on_outline_rail_scroll))
            // Always listen for hover (not only once already hovered) so hovering
            // anywhere over the full-height rail track — including the empty space
            // above the first tick and below the last — keeps the rail (and the
            // editor toolbar) shown instead of letting the toolbar hide.
            .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                this.set_outline_rail_hovered(*hovering, cx);
            }))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(track_extra_right_padding))
                    .w(px(if self.outline_rail_hovered {
                        rail_width
                    } else {
                        (tick_width + active_extra_width).max(tick_thickness)
                    }))
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

    pub(crate) fn render_outline_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let headings = self.collect_outline_headings();
        let active_line = self.outline_rail_active_line(&headings);
        let visible_heading_indices = visible_outline_heading_indices_with_depths(
            &headings,
            &self.outline_rail_collapsed_heading_lines,
        );
        let ups = &self.styles.utility_panel;
        let os = &self.styles.outline;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let item_min_height = self.scaled(os.outline_rail_label_item_height_px).max(22.0);
        let item_padding_y = self.scaled(os.outline_rail_label_item_padding_y_px);
        let line_height = self.scaled(os.outline_rail_label_line_height_px);
        let font_size = self.scaled(os.outline_rail_label_font_size_px);
        let indent_step = self.scaled(os.outline_rail_label_indent_step_px);
        let fold_icon_size = self.scaled(os.outline_rail_label_fold_icon_size_px);
        let fold_icon_hit_size = (fold_icon_size + self.scaled(8.0)).round();
        let fold_icon_gap = self.scaled(os.outline_rail_label_fold_icon_gap_px);
        let panel_width = self
            .utility_panel_width()
            .max(f32::from(
                self.outline_panel_scroll_handle.bounds().size.width,
            ))
            .max(self.scaled(140.0));
        let scrollbar_reserve = self.scaled(
            self.styles
                .editor
                .editor_scrollbar_hit_width_px
                .max(self.styles.editor.editor_scrollbar_width_px)
                + self.styles.editor.editor_scrollbar_right_margin_px,
        );
        let fallback_chars_per_line = os.outline_rail_label_wrap_chars_per_line.max(8);
        let outline_row_metrics = |heading: &OutlineHeading, depth: usize| {
            let indent = depth as f32 * indent_step.max(0.0);
            let text_width = (panel_width
                - section_padding_x * 2.0
                - indent
                - fold_icon_hit_size
                - fold_icon_gap
                - scrollbar_reserve)
                .max(font_size * fallback_chars_per_line as f32 * 0.35);
            let chars_per_line =
                outline_chars_per_line(text_width, font_size, fallback_chars_per_line);
            let line_count = outline_heading_line_count(heading.text.as_ref(), chars_per_line);
            let height =
                (item_padding_y * 2.0 + line_height * line_count as f32).max(item_min_height);
            (height, chars_per_line)
        };

        let mut children: Vec<AnyElement> = vec![
            div().h(px(top_spacer)).flex_shrink_0().into_any_element(),
            div()
                .h(px(header_height))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(px(header_padding_x))
                .text_size(px(header_font_size))
                .text_color(ups.utility_panel_header_text_color)
                .child(crate::i18n::outline_title(self.ui_language))
                .into_any_element(),
        ];

        if headings.is_empty() {
            children.push(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(empty_font_size))
                    .text_color(ups.utility_panel_empty_text_color)
                    .child(crate::i18n::outline_empty(self.ui_language))
                    .into_any_element(),
            );
        } else {
            children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            let prefix_height = top_spacer + header_height + section_gap;
            let item_heights = visible_heading_indices
                .iter()
                .map(|(heading_index, depth)| {
                    let heading = &headings[*heading_index];
                    outline_row_metrics(heading, *depth).0
                })
                .collect::<Vec<_>>();
            let list_window = super::list_viewport::variable_list_window(
                &self.outline_panel_scroll_handle,
                prefix_height,
                &item_heights,
                12,
                24,
            );
            if list_window.top_spacer_px > 0.0 {
                children.push(
                    div()
                        .h(px(list_window.top_spacer_px))
                        .flex_shrink_0()
                        .into_any_element(),
                );
            }
            let active_bg = ups.utility_panel_button_hover_bg;
            let fold_icon_hover_color = os.outline_rail_label_hover_color;
            let render_start = list_window.render.start;
            for (visible_offset, (heading_index, depth)) in visible_heading_indices
                [list_window.render]
                .iter()
                .copied()
                .enumerate()
            {
                let heading = &headings[heading_index];

                let target_line = heading.line;
                let has_children = outline_heading_has_children(&headings, heading_index);
                let is_collapsed = self
                    .outline_rail_collapsed_heading_lines
                    .contains(&target_line);
                let is_active = Some(target_line) == active_line;
                let color = if is_active {
                    os.outline_rail_label_active_color
                } else {
                    os.outline_rail_label_color
                };
                let indent = depth as f32 * indent_step.max(0.0);
                let inner_padding_left = section_padding_x + indent;
                let (row_height, chars_per_line) = (item_heights[render_start + visible_offset], {
                    outline_row_metrics(heading, depth).1
                });
                let icon_path = if is_collapsed {
                    FOLD_SVG_PATH
                } else {
                    FOLD_EXPANDED_SVG_PATH
                };

                children.push(
                    div()
                        .min_h(px(row_height))
                        .flex_shrink_0()
                        .pl(px(inner_padding_left))
                        .pr(px(section_padding_x))
                        .py(px(item_padding_y))
                        .flex()
                        .items_start()
                        .gap(px(fold_icon_gap))
                        .text_size(px(font_size))
                        .text_color(color)
                        .line_height(px(line_height))
                        .cursor(CursorStyle::Arrow)
                        .when(is_active, |this| this.bg(active_bg))
                        .hover(move |style| style.bg(active_bg))
                        .when(has_children, |this| {
                            this.child(
                                div()
                                    .w(px(fold_icon_hit_size))
                                    .h(px(fold_icon_hit_size))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(color)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.toggle_outline_rail_heading_collapsed(
                                                target_line,
                                                cx,
                                            );
                                        }),
                                    )
                                    .child(
                                        svg()
                                            .path(icon_path)
                                            .size(px(fold_icon_size))
                                            .text_color(color)
                                            .hover(move |style| {
                                                style.text_color(fold_icon_hover_color)
                                            }),
                                    ),
                            )
                        })
                        .when(!has_children, |this| {
                            this.child(div().w(px(fold_icon_hit_size)).flex_none())
                        })
                        .child(div().min_w(px(0.0)).flex_1().whitespace_normal().child(
                            outline_soft_wrappable_text(heading.text.as_ref(), chars_per_line),
                        ))
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
            if list_window.bottom_spacer_px > 0.0 {
                children.push(
                    div()
                        .h(px(list_window.bottom_spacer_px))
                        .flex_shrink_0()
                        .into_any_element(),
                );
            }
            children.push(
                div()
                    .h(px(bottom_spacer))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }

        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::Outline,
            &self.outline_panel_scroll_handle,
            f32::from(self.outline_panel_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        div()
            .id("outline-panel")
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .relative()
            .overflow_hidden()
            .border_l(px(ups.utility_panel_border_width_px))
            .border_color(ups.utility_panel_border_color)
            .child(
                div()
                    .id("outline-panel-scroll")
                    .h_full()
                    .w_full()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        this.reveal_panel_scrollbar(PanelScrollbarKind::Outline, cx);
                    }))
                    .track_scroll(&self.outline_panel_scroll_handle)
                    .flex()
                    .flex_col()
                    .children(children),
            )
            .when_some(scrollbar, |this, scrollbar| this.child(scrollbar))
            .into_any_element()
    }

    pub(crate) fn outline_panel_preferred_width(&self) -> f32 {
        let headings = self.collect_outline_headings();
        let ups = &self.styles.utility_panel;
        let os = &self.styles.outline;
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let font_size = self.scaled(os.outline_rail_label_font_size_px);
        let indent_step = self.scaled(os.outline_rail_label_indent_step_px);
        let fold_icon_size = self.scaled(os.outline_rail_label_fold_icon_size_px);
        let fold_icon_gap = self.scaled(os.outline_rail_label_fold_icon_gap_px);
        let text_cap = self.scaled(os.outline_rail_label_max_width_px);
        let visible_heading_indices = visible_outline_heading_indices_with_depths(
            &headings,
            &self.outline_rail_collapsed_heading_lines,
        );
        let header_width = header_padding_x * 2.0
            + outline_heading_text_width(
                crate::i18n::outline_title(self.ui_language),
                header_font_size,
            )
            + self.scaled(20.0);
        let content_width = visible_heading_indices
            .iter()
            .take(512)
            .map(|(heading_index, depth)| {
                let heading = &headings[*heading_index];
                let text_width = outline_heading_text_width(heading.text.as_ref(), font_size)
                    .min(text_cap.max(font_size));
                section_padding_x * 2.0
                    + *depth as f32 * indent_step.max(0.0)
                    + fold_icon_size
                    + fold_icon_gap
                    + text_width
                    + self.scaled(20.0)
            })
            .fold(header_width, f32::max);
        let min_width = self.scaled(150.0);
        let max_width = self.scaled(self.styles.utility_panel.utility_panel_resize_max_width_px);
        let (min_width, max_width) = normalized_outline_width_bounds(min_width, max_width);
        let content_width = if content_width.is_finite() {
            content_width.max(0.0)
        } else {
            min_width
        };
        content_width.clamp(min_width, max_width)
    }

    pub(crate) fn outline_panel_resize_bounds(&self) -> (f32, f32) {
        let preferred = self.outline_panel_preferred_width();
        let min_width = (preferred * 0.85).max(self.scaled(140.0));
        let global_max = self.scaled(self.styles.utility_panel.utility_panel_resize_max_width_px);
        let max_width = (preferred * 1.25).max(min_width).min(global_max);
        normalized_outline_width_bounds(min_width, max_width)
    }
}
