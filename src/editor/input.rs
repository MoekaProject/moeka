use std::collections::HashMap;
use std::ops::Range;
use std::time::Duration;

use gpui::{
    Bounds, Context, FontFallbacks, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, ScrollWheelEvent, SharedString, TextRun, Window, WrappedLine, point, px, size,
};

use crate::markdown::{BlockType, InlineKind};

use super::*;

pub(super) struct TableCellDisplayInfo {
    pub(super) wrapped_lines: Vec<WrappedLine>,
    pub(super) hard_line_ranges: Vec<Range<usize>>,
    pub(super) hanging_wraps: Vec<super::table_cell_render::HangingWrap>,
    pub(super) alignment: crate::markdown::TableAlign,
    pub(super) display_text: String,
    pub(super) display_to_buffer: Vec<usize>,
    pub(super) buffer_to_display: Vec<usize>,
    pub(super) cell_range: Range<usize>,
    pub(super) display_source_end: usize,
    pub(super) line_height: f32,
}

impl TableCellDisplayInfo {
    pub(super) fn hard_line_top(&self, hard_line_index: usize) -> f32 {
        self.wrapped_lines
            .iter()
            .take(hard_line_index)
            .map(|line| f32::from(line.size(px(self.line_height)).height))
            .sum()
    }
}

fn nearest_table_cell_in_row(
    areas: &[(Bounds<Pixels>, usize, usize, usize)],
    table: usize,
    row: usize,
    position: Point<Pixels>,
) -> Option<(Bounds<Pixels>, usize, usize, usize)> {
    areas
        .iter()
        .filter(|(bounds, start, visual_row, _)| {
            *start == table
                && *visual_row == row
                && position.y >= bounds.origin.y
                && position.y < bounds.origin.y + bounds.size.height
        })
        .min_by(|(left, ..), (right, ..)| {
            let distance = |bounds: &Bounds<Pixels>| {
                let x = f32::from(position.x);
                let start = f32::from(bounds.origin.x);
                (x - x.clamp(start, start + f32::from(bounds.size.width))).abs()
            };
            distance(left).total_cmp(&distance(right))
        })
        .copied()
}

fn table_cell_footnote_label_for_click(
    decoration: &crate::markdown::LineDecoration,
    column: usize,
) -> Option<&str> {
    decoration.footnote_references.iter().find_map(|reference| {
        (reference.full_range.start <= column && column <= reference.full_range.end)
            .then_some(reference.label.as_str())
    })
}

fn table_cell_buffer_column_for_hit(
    info: &TableCellDisplayInfo,
    source: &str,
    hard_line_start: usize,
    soft_row: usize,
    local_x: f32,
    display_index: usize,
) -> usize {
    if soft_row == 0 && local_x <= 0.0 {
        // Left padding belongs before a collapsed opening marker, not to the
        // first visible glyph. Invert the map so accepted hard breaks work too.
        if let Some(source_start) = info
            .buffer_to_display
            .iter()
            .position(|&index| index == hard_line_start)
            && let Some(tail) = source.get(source_start..)
        {
            let trimmed = tail.trim_start_matches([' ', '\t']);
            return if trimmed.is_empty() {
                source_start
            } else {
                source_start + tail.len() - trimmed.len()
            };
        }
    }
    info.display_to_buffer
        .get(display_index)
        .copied()
        .unwrap_or(info.display_source_end)
}

fn table_cell_visual_line_count(info: &TableCellDisplayInfo) -> usize {
    info.wrapped_lines
        .iter()
        .zip(&info.hard_line_ranges)
        .map(|(line, range)| {
            helpers::wrap_boundary_byte_indices(
                line,
                info.display_text.get(range.clone()).unwrap_or(""),
            )
            .len()
                + 1
        })
        .sum::<usize>()
        .max(1)
}

fn table_cell_wrapped_line_for_y(
    info: &TableCellDisplayInfo,
    local_y: f32,
) -> Option<(usize, f32)> {
    let mut line_top = 0.0;
    for (index, wrapped) in info.wrapped_lines.iter().enumerate() {
        let height = f32::from(wrapped.size(px(info.line_height)).height);
        // A taller sibling cell can leave space below this cell's last hard line.
        if local_y < line_top + height || index + 1 == info.wrapped_lines.len() {
            let last_row = wrapped.wrap_boundaries().len();
            let last_row_center = wrapped.row_top(last_row, px(info.line_height))
                + wrapped.row_height(last_row, px(info.line_height)) * 0.5;
            return Some((
                index,
                (local_y - line_top)
                    .max(0.0)
                    .min(f32::from(last_row_center)),
            ));
        }
        line_top += height;
    }
    None
}

fn table_cell_position(
    info: &TableCellDisplayInfo,
    cell_offset: usize,
    affinity: Option<WrapBoundaryAffinityKind>,
) -> Option<(usize, Point<Pixels>)> {
    let line_height = info.line_height;
    let display_index = if cell_offset >= info.buffer_to_display.len() {
        info.display_text.len()
    } else {
        helpers::buffer_to_display_offset(&info.buffer_to_display, cell_offset)
    };
    let hard_line_index = info
        .hard_line_ranges
        .iter()
        .position(|range| display_index >= range.start && display_index <= range.end)
        .unwrap_or_else(|| info.hard_line_ranges.len().saturating_sub(1));
    let range = info.hard_line_ranges.get(hard_line_index)?;
    let wrapped = info.wrapped_lines.get(hard_line_index)?;
    let local_index = display_index.saturating_sub(range.start).min(range.len());
    let position = info
        .hanging_wraps
        .get(hard_line_index)
        .copied()
        .unwrap_or_default()
        .position(
            wrapped,
            info.display_text.get(range.clone())?,
            local_index,
            line_height,
            affinity,
        );
    Some((hard_line_index, position))
}

fn table_cell_visual_position(
    info: &TableCellDisplayInfo,
    cell_offset: usize,
    inner_width: f32,
    affinity: Option<WrapBoundaryAffinityKind>,
) -> Option<(usize, f32)> {
    let (hard_line_index, position) = table_cell_position(info, cell_offset, affinity)?;
    let wrapped = info.wrapped_lines.get(hard_line_index)?;
    let line_height = info.line_height;
    let prior_lines = info
        .wrapped_lines
        .iter()
        .zip(&info.hard_line_ranges)
        .take(hard_line_index)
        .map(|(line, range)| {
            helpers::wrap_boundary_byte_indices(
                line,
                info.display_text.get(range.clone()).unwrap_or(""),
            )
            .len()
                + 1
        })
        .sum::<usize>();
    let soft_line = wrapped.row_for_y(position.y, px(line_height));
    let content_offset = helpers::table_cell_content_offset(
        info.alignment,
        inner_width,
        f32::from(wrapped.unwrapped_layout.width),
    );
    Some((
        prior_lines + soft_line,
        content_offset + f32::from(position.x),
    ))
}

fn table_cell_offset_for_visual_line(
    info: &TableCellDisplayInfo,
    target_visual_line: usize,
    goal_x: f32,
    inner_width: f32,
) -> Option<(usize, Option<WrapBoundaryAffinityKind>)> {
    let line_height = info.line_height;
    let mut first_visual_line = 0usize;
    for (hard_line, (wrapped, range)) in info
        .wrapped_lines
        .iter()
        .zip(&info.hard_line_ranges)
        .enumerate()
    {
        let hard_text = info.display_text.get(range.clone()).unwrap_or("");
        let line_count = helpers::wrap_boundary_byte_indices(wrapped, hard_text).len() + 1;
        if target_visual_line < first_visual_line + line_count {
            let soft_line = target_visual_line - first_visual_line;
            let content_offset = helpers::table_cell_content_offset(
                info.alignment,
                inner_width,
                f32::from(wrapped.unwrapped_layout.width),
            );
            let target_y = wrapped.row_top(soft_line, px(line_height))
                + wrapped.row_height(soft_line, px(line_height)) * 0.5;
            let local_x = info
                .hanging_wraps
                .get(hard_line)
                .copied()
                .unwrap_or_default()
                .hit_x(wrapped, goal_x - content_offset, target_y, line_height);
            let local_display_index = match wrapped
                .closest_index_for_position(point(local_x, target_y), px(line_height))
            {
                Ok(index) | Err(index) => index.min(range.len()),
            };
            let display_index = range.start + local_display_index;
            let offset = if display_index < info.display_to_buffer.len() {
                info.display_to_buffer[display_index]
            } else {
                info.display_source_end
            };
            return Some((
                offset,
                EditorView::wrap_boundary_affinity_for_row(
                    wrapped,
                    hard_text,
                    local_display_index,
                    soft_line,
                ),
            ));
        }
        first_visual_line += line_count;
    }
    None
}

#[derive(Clone)]
pub(crate) struct FrontMatterTextHit {
    pub bounds: Bounds<Pixels>,
    pub origin: Point<Pixels>,
    pub shaped: WrappedLine,
    pub line_height: f32,
    pub source_map: Vec<usize>,
}

impl FrontMatterTextHit {
    pub fn offset_for_position(&self, position: Point<Pixels>) -> usize {
        let local = point((position.x - self.origin.x).max(px(0.0)), px(0.0));
        let index = match self
            .shaped
            .closest_index_for_position(local, px(self.line_height))
        {
            Ok(index) | Err(index) => index,
        };
        self.source_map
            .get(index)
            .copied()
            .or_else(|| self.source_map.last().copied())
            .unwrap_or(0)
    }
}

pub(super) struct InlineLineDisplayInfo {
    pub(super) display_text: String,
    pub(super) display_to_buffer: Vec<usize>,
    pub(super) base_runs: Vec<TextRun>,
    pub(super) buffer_to_display: Vec<usize>,
    pub(super) font_size: f32,
    pub(super) line_height: f32,
    pub(super) wrapped_line: WrappedLine,
    pub(super) wrap_boundary_upstream_xs: HashMap<usize, f32>,
    pub(super) indent_px: f32,
    pub(super) content_x: f32,
}

fn visible_line_hit(geometry: &[(usize, f32, f32)], position_y: f32) -> Option<(usize, f32)> {
    geometry.iter().find_map(|(line, top, height)| {
        (*top <= position_y && position_y < *top + *height).then_some((*line, *top))
    })
}

fn local_text_y_for_hit(
    position_y: f32,
    document_y: f32,
    visible_line_top: Option<f32>,
    document_text_top: f32,
    line_text_inset: f32,
) -> f32 {
    visible_line_top
        .map(|line_top| position_y - line_top - line_text_inset)
        .unwrap_or(document_y - document_text_top)
        .max(0.0)
}

fn display_index_to_buffer_col(
    display_text: &str,
    display_to_buffer: &[usize],
    display_index: usize,
) -> usize {
    let mut display_col = display_index.min(display_text.len());
    while display_col > 0 && !display_text.is_char_boundary(display_col) {
        display_col -= 1;
    }
    if display_col < display_to_buffer.len() {
        display_to_buffer[display_col]
    } else {
        display_to_buffer.last().copied().map_or(0, |last| last + 1)
    }
}

fn compute_inline_wrap_boundary_upstream_xs(
    window: &mut Window,
    wrapped: &WrappedLine,
    display_text: &str,
    runs: &[TextRun],
    font_size: f32,
    line_height: f32,
    letter_spacing: f32,
) -> HashMap<usize, f32> {
    let mut upstream_xs = HashMap::new();
    let mut segment_start = 0;

    for boundary in helpers::wrap_boundary_byte_indices(wrapped, display_text) {
        let segment_text = &display_text[segment_start..boundary];
        if segment_text.contains('\u{a0}') {
            upstream_xs.insert(
                boundary,
                f32::from(
                    wrapped.unwrapped_layout.x_for_index(boundary)
                        - wrapped.unwrapped_layout.x_for_index(segment_start),
                ),
            );
            segment_start = boundary;
            continue;
        }
        let segment_runs = helpers::extract_runs_for_range(runs, segment_start, boundary);
        let segment_shared: SharedString = if segment_text.is_empty() {
            " ".into()
        } else {
            segment_text.to_string().into()
        };

        if let Ok(segment_lines) = window.text_system().shape_text(
            segment_shared,
            px(font_size),
            &segment_runs,
            None,
            None,
        ) && let Some(mut segment_wrapped) = segment_lines.into_iter().next()
        {
            if letter_spacing != 0.0 {
                helpers::apply_letter_spacing(&mut segment_wrapped, letter_spacing);
            }
            let segment_end_x = segment_wrapped
                .position_for_index(segment_text.len(), px(line_height))
                .map(|pos| f32::from(pos.x))
                .unwrap_or(0.0);
            upstream_xs.insert(boundary, segment_end_x);
        }

        segment_start = boundary;
    }

    upstream_xs
}

#[inline(always)]
pub(super) const fn ime_debug_enabled() -> bool {
    false
}

pub(super) fn ime_debug_log(message: impl AsRef<str>) {
    if ime_debug_enabled() {
        log_error!("[ime-debug] {}", message.as_ref());
    }
}

impl EditorView {
    pub(super) fn sync_link_navigation_modifier(
        &mut self,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let held = modifiers.secondary();
        if self.command_held != held {
            self.command_held = held;
            cx.notify();
        }
    }

    fn line_diff_selection_offset_at(
        &self,
        position: Point<Pixels>,
        hunk_index: usize,
    ) -> Option<(usize, Option<WrapBoundaryAffinityKind>)> {
        self.line_diff_selection_layouts
            .iter()
            .filter(|layout| layout.hunk_index == hunk_index)
            .min_by(|a, b| {
                let distance = |layout: &super::element::LineDiffSelectionLayout| {
                    let top = f32::from(layout.origin.y);
                    let bottom =
                        top + f32::from(layout.wrapped.size(px(layout.line_height)).height);
                    let y = f32::from(position.y);
                    (top - y).max(y - bottom).max(0.0)
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|layout| layout.selection_at(position))
    }

    fn finish_line_diff_selection_move(
        &mut self,
        selection: line_diff::LineDiffSelection,
        cx: &mut Context<Self>,
    ) {
        self.line_diff_selection = Some(selection);
        if let Some(comparison) = &mut self.line_diff_comparison {
            comparison.cursor_entered = true;
        }
        self.next_blink_epoch();
        cx.notify();
    }

    pub(super) fn move_line_diff_selection_word(
        &mut self,
        forward: bool,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mut selection) = self.line_diff_selection else {
            return false;
        };
        let Some(layout) = self
            .line_diff_selection_layouts
            .iter()
            .find(|layout| layout.hunk_index == selection.hunk_index)
        else {
            return true;
        };
        let buffer = Buffer::from_text(&layout.source_text);
        let mut cursor = Cursor {
            offset: selection.head,
            anchor: selection.anchor,
        };
        match (forward, extend) {
            (false, false) => cursor.move_word_left(&buffer),
            (true, false) => cursor.move_word_right(&buffer),
            (false, true) => cursor.select_word_left(&buffer),
            (true, true) => cursor.select_word_right(&buffer),
        }
        selection.head = cursor.offset;
        selection.anchor = cursor.anchor;
        selection.affinity = None;
        selection.goal_x = None;
        self.finish_line_diff_selection_move(selection, cx);
        true
    }

    pub(super) fn move_line_diff_selection_document_edge(
        &mut self,
        to_end: bool,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mut selection) = self.line_diff_selection else {
            return false;
        };
        let Some(layout) = self
            .line_diff_selection_layouts
            .iter()
            .find(|layout| layout.hunk_index == selection.hunk_index)
        else {
            return true;
        };
        selection.head = if to_end { layout.source_text.len() } else { 0 };
        if !extend {
            selection.anchor = selection.head;
        }
        selection.affinity = None;
        selection.goal_x = None;
        self.finish_line_diff_selection_move(selection, cx);
        true
    }

    pub(super) fn move_line_diff_selection_horizontal(
        &mut self,
        forward: bool,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mut selection) = self.line_diff_selection else {
            return false;
        };
        let Some(layout) = self
            .line_diff_selection_layouts
            .iter()
            .find(|layout| layout.hunk_index == selection.hunk_index)
        else {
            return true;
        };
        selection.move_head(&layout.source_text, forward, extend);
        self.finish_line_diff_selection_move(selection, cx);
        true
    }

    pub(super) fn move_line_diff_selection_vertical(
        &mut self,
        down: bool,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mut selection) = self.line_diff_selection else {
            return false;
        };
        let Some(layout) = self.line_diff_selection_layouts.iter().find(|layout| {
            layout.hunk_index == selection.hunk_index
                && layout.source_range.start <= selection.head
                && selection.head <= layout.source_range.end
        }) else {
            return true;
        };
        let Some(position) = layout.position_for_selection(selection) else {
            return true;
        };
        let row = layout.wrapped.row_for_y(position.y, px(layout.line_height));
        let last_row = layout.wrapped.wrap_boundaries().len();
        let mut target_layout = layout;
        let target_row = if down && row == last_row {
            if let Some(next) = self.line_diff_selection_layouts.iter().find(|next| {
                next.hunk_index == selection.hunk_index
                    && next.source_range.start == layout.source_range.end + 1
            }) {
                target_layout = next;
                0
            } else {
                row
            }
        } else if !down && row == 0 {
            if let Some(previous) = self.line_diff_selection_layouts.iter().find(|previous| {
                previous.hunk_index == selection.hunk_index
                    && previous.source_range.end + 1 == layout.source_range.start
            }) {
                target_layout = previous;
                previous.wrapped.wrap_boundaries().len()
            } else {
                row
            }
        } else if down {
            (row + 1).min(last_row)
        } else {
            row.saturating_sub(1)
        };
        let goal_x = selection.goal_x.unwrap_or(f32::from(position.x));
        let same_row = target_layout.source_range == layout.source_range && target_row == row;
        let layout = target_layout;
        let target_y = layout.wrapped.row_top(target_row, px(layout.line_height))
            + layout
                .wrapped
                .row_height(target_row, px(layout.line_height))
                * 0.5;
        let display_index = match layout
            .wrapped
            .closest_index_for_position(point(px(goal_x), target_y), px(layout.line_height))
        {
            Ok(index) | Err(index) => index.min(layout.display_text.len()),
        };
        selection.head = if same_row {
            if down { layout.source_text.len() } else { 0 }
        } else {
            layout.source_offset_for_index(display_index)
        };
        selection.affinity = if same_row {
            None
        } else {
            Self::wrap_boundary_affinity_for_row(
                &layout.wrapped,
                &layout.display_text,
                display_index,
                target_row,
            )
        };
        selection.goal_x = Some(goal_x);
        if !extend {
            selection.anchor = selection.head;
        }
        self.finish_line_diff_selection_move(selection, cx);
        true
    }

    pub(super) fn move_line_diff_selection_line_edge(
        &mut self,
        to_end: bool,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mut selection) = self.line_diff_selection else {
            return false;
        };
        let Some(layout) = self.line_diff_selection_layouts.iter().find(|layout| {
            layout.hunk_index == selection.hunk_index
                && layout.source_range.start <= selection.head
                && selection.head <= layout.source_range.end
        }) else {
            return true;
        };
        let Some(position) = layout.position_for_selection(selection) else {
            return true;
        };
        let row = layout.wrapped.row_for_y(position.y, px(layout.line_height));
        let row_range = helpers::wrap_segment_byte_ranges(
            layout.display_text.len(),
            &helpers::wrap_boundary_byte_indices(&layout.wrapped, &layout.display_text),
        )
        .get(row)
        .cloned();
        if let Some(range) = row_range {
            let display_index = if to_end { range.end } else { range.start };
            selection.head = layout.source_offset_for_index(display_index);
            selection.affinity = Self::wrap_boundary_affinity_for_row(
                &layout.wrapped,
                &layout.display_text,
                display_index,
                row,
            );
            selection.goal_x = None;
            if !extend {
                selection.anchor = selection.head;
            }
            self.finish_line_diff_selection_move(selection, cx);
        }
        true
    }

    pub(super) fn selection_drag_scroll_delta_for_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<f32> {
        let pos_y: f32 = position.y.into();
        let origin_y: f32 = self.content_origin.y.into();
        let rel_pos_y = pos_y - origin_y;
        let scroll_margin = 48.0;

        if rel_pos_y < scroll_margin {
            let intensity = ((scroll_margin - rel_pos_y) / scroll_margin).max(0.0);
            Some(-(6.0 + intensity * intensity * 42.0))
        } else if rel_pos_y > self.last_viewport_height - scroll_margin {
            let intensity = ((rel_pos_y - (self.last_viewport_height - scroll_margin))
                / scroll_margin)
                .max(0.0);
            Some(6.0 + intensity * intensity * 42.0)
        } else {
            None
        }
    }

    fn sync_selection_drag_scroll_task(&mut self, cx: &mut Context<Self>) {
        if !self.is_selecting || self.selection_drag_position.is_none() {
            self.selection_drag_scroll_task = None;
            return;
        }

        let Some(position) = self.selection_drag_position else {
            self.selection_drag_scroll_task = None;
            return;
        };
        if self
            .selection_drag_scroll_delta_for_position(position)
            .is_none()
        {
            self.selection_drag_scroll_task = None;
            return;
        }
        if self.selection_drag_scroll_task.is_some() {
            return;
        }

        self.selection_drag_scroll_task = Some(cx.spawn(async move |this, cx| {
            // EditorView is the window root. Resolve its own window after the
            // mouse event releases it, never the app's potentially different active window.
            let Ok(Some(window_handle)) = cx.update(|cx| {
                cx.windows()
                    .into_iter()
                    .filter_map(|window| window.downcast::<Self>())
                    .find(|window| window.entity(cx).is_ok_and(|entity| entity == this))
            }) else {
                return;
            };
            loop {
                let did_scroll = window_handle
                    .update(cx, |editor, window, cx| {
                        if cx.weak_entity() != this {
                            return None;
                        }
                        editor.selection_drag_scroll_task.as_ref()?;
                        let position = editor.selection_drag_position?;
                        let delta = editor.selection_drag_scroll_delta_for_position(position)?;
                        let previous_scroll = editor.scroll_offset;
                        editor.scroll_offset += delta;
                        editor.clamp_scroll();
                        if (editor.scroll_offset - previous_scroll).abs() < f32::EPSILON {
                            return None;
                        }
                        if editor.table_cell_selection.is_none()
                            && !editor.update_table_boundary_drag_selection(position)
                        {
                            let offset =
                                editor.selection_drag_offset_for_position(window, position);
                            editor.update_wrap_boundary_affinity_for_position(offset, position);
                            editor.cursor.select_to(offset, &editor.buffer);
                            if editor.cursor.has_selection() {
                                editor.suppress_fold_for_empty_selection_drag = false;
                            }
                        }
                        cx.notify();
                        Some(())
                    })
                    .ok()
                    .flatten()
                    .is_some();

                if !did_scroll {
                    return;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        }));
    }

    fn selection_drag_offset_for_position(
        &mut self,
        window: &mut Window,
        position: Point<Pixels>,
    ) -> usize {
        if self.layout_dirty {
            return self.offset_for_position(position);
        }
        let cell = self
            .table_click_areas
            .iter()
            .find(|(bounds, _, _, _)| bounds.contains(&position))
            .copied()
            .or_else(|| {
                let (table, anchor_line, _) = self.table_cell_for_offset(self.cursor.anchor)?;
                let row = self.table_visual_row_for_source_line(table, anchor_line)?;
                nearest_table_cell_in_row(&self.table_click_areas, table, row, position)
            });
        let offset = cell
            .and_then(|(bounds, table, row, column)| {
                let hit_position =
                    point(position.x.clamp(bounds.left(), bounds.right()), position.y);
                self.table_cell_offset_for_position(
                    window,
                    table,
                    row,
                    column,
                    &bounds,
                    hit_position,
                )
            })
            .unwrap_or_else(|| self.offset_for_position(position));
        self.table_visible_selection_target(self.cursor.anchor, offset)
    }

    fn update_table_boundary_drag_selection(&mut self, position: Point<Pixels>) -> bool {
        if !self.selection_drag_has_moved {
            return false;
        }
        let Some(origin) = self.table_boundary_drag_origin else {
            return false;
        };
        if !self
            .table_click_areas
            .iter()
            .any(|(bounds, table_start, _, _)| {
                *table_start == origin.table_start_line && bounds.contains(&position)
            })
        {
            return false;
        }
        let Some(table_info) = self
            .decorations
            .get(origin.table_start_line)
            .and_then(|decoration| decoration.table_info.as_ref())
        else {
            return false;
        };
        let table_start = self.buffer.line_start(table_info.table_start_line);
        let table_end = self
            .buffer
            .line_end(table_info.table_end_line.saturating_sub(1));
        match origin.side {
            TableBoundarySide::Above => {
                self.cursor.anchor = table_start;
                self.cursor.offset = table_end;
            }
            TableBoundarySide::Below => {
                self.cursor.anchor = table_end;
                self.cursor.offset = table_start;
            }
        }
        self.table_boundary_caret = None;
        self.clear_wrap_boundary_affinity();
        self.suppress_fold_for_empty_selection_drag = false;
        true
    }

    pub(crate) fn primary_cursor_has_inline_edit_headroom(&self) -> bool {
        let cursor_line = self.cursor_line();
        let cursor_y = self.line_y_offset(cursor_line);
        let cursor_line_height = self.visual_height_for_line(cursor_line);
        let viewport_top = self.scroll_offset + self.padding_top();
        let viewport_bottom =
            self.scroll_offset + self.editor_visible_height() - self.padding_bottom();

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

    // Mouse handlers

    pub(super) fn remove_front_matter_list_item_for_position(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.layout_dirty
            || self.active_tab_is_read_only()
            || self.context_menu.is_some()
            || self.preview_overlay.is_some()
        {
            return false;
        }
        let Some((_, header, item)) = self
            .front_matter_delete_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&position))
            .copied()
        else {
            return false;
        };
        if !self.front_matter_group_plan().iter().any(|plan| {
            plan.group.header_line == header
                && !plan.active
                && !plan.source_visible
                && item < plan.group.items.len()
        }) {
            return false;
        }
        self.is_selecting = false;
        self.selection_drag_position = None;
        self.selection_drag_scroll_task = None;
        self.suppress_next_editor_mouse_up = true;
        self.remove_front_matter_list_item(header, item, cx);
        true
    }

    pub(super) fn open_front_matter_type_menu_for_position(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.context_menu.is_some() || self.preview_overlay.is_some() {
            return false;
        }
        let Some((bounds, line)) = self
            .front_matter_type_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&position))
            .copied()
        else {
            return false;
        };
        if super::front_matter::property_type(&self.buffer, &self.decorations, line).is_none() {
            return false;
        }
        let menu = ContextMenu {
            position: point(bounds.left(), bounds.bottom()),
            kind: ContextMenuKind::FrontMatterType {
                line,
                pane_id: self.current_pane_id(),
                tab_id: self.active_tab_id.clone(),
                source: Arc::from(self.buffer.text()),
            },
        };
        self.is_selecting = false;
        self.selection_drag_position = None;
        self.selection_drag_scroll_task = None;
        self.last_mouse_down_target = None;
        self.suppress_next_editor_mouse_up = true;
        self.open_context_menu(menu, false, cx);
        true
    }

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_front_matter_input_geometry(window, self.front_matter_input_viewport_bounds());
        self.on_mouse_down_inner(event, window, cx);
        self.sync_line_diff_comparison_cursor(cx);
        self.refresh_front_matter_input_geometry(window, self.front_matter_input_viewport_bounds());
    }

    pub(super) fn handle_line_diff_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.button != MouseButton::Left
            || self.context_menu.is_some()
            || self.preview_overlay.is_some()
        {
            return false;
        }
        if let Some(&(_, hunk_index)) = self
            .line_diff_restore_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.restore_line_diff_hunk(hunk_index, cx);
        } else if let Some(&(_, hunk_index)) = self
            .line_diff_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.toggle_line_diff_hunk(hunk_index, cx);
        } else if let Some(layout) = self
            .line_diff_selection_layouts
            .iter()
            .find(|layout| layout.contains_position(event.position))
        {
            let hunk_index = layout.hunk_index;
            let (offset, affinity) = layout.selection_at(event.position);
            let mut selection = if event.modifiers.shift
                && let Some(mut selection) = self.line_diff_selection
                && selection.hunk_index == hunk_index
            {
                selection.head = offset;
                selection
            } else {
                line_diff::LineDiffSelection {
                    hunk_index,
                    anchor: offset,
                    head: offset,
                    ..Default::default()
                }
            };
            selection.affinity = affinity;
            selection.goal_x = None;
            if event.click_count >= 3 {
                selection.anchor = 0;
                selection.head = layout.source_text.len();
            } else if event.click_count == 2 {
                let buffer = Buffer::from_text(&layout.source_text);
                let range = Self::word_selection_range(&layout.source_text, offset);
                selection.anchor = buffer.clamp_grapheme_offset(range.start);
                selection.head = buffer.clamp_grapheme_offset(range.end);
                selection.affinity = None;
            }
            self.finish_line_diff_selection_move(selection, cx);
            self.line_diff_selection_dragging = true;
            self.cursor.anchor = self.cursor.offset;
            self.extra_cursors.clear();
            self.extra_marked_ranges.clear();
            self.table_cell_selection = None;
            self.table_boundary_caret = None;
            self.is_selecting = false;
            self.selection_drag_position = None;
            self.selection_drag_scroll_task = None;
        } else {
            return false;
        }
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    fn on_mouse_down_inner(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu.is_some() {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if self.preview_overlay.is_some() {
            self.on_preview_overlay_mouse_down(event, window, cx);
            return;
        }
        if self.handle_line_diff_mouse_down(event, window, cx) {
            window.focus(&self.focus_handle);
            return;
        }
        self.last_mouse_down_target = None;

        if event.button == MouseButton::Left
            && let Some(range) = self.collapsed_front_matter_range()
            && let Some(summary) = self.front_matter_summary_line()
            && let Some((_, top, bottom)) = self
                .visible_line_geometry
                .iter()
                .find(|(line, _, _)| *line == summary)
            && event.position.y >= px(*top)
            && event.position.y < px(*bottom)
            && event.position.x >= px(self.last_content_left)
        {
            self.toggle_fold(range.start - 1, cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        if event.button == MouseButton::Left
            && self.remove_front_matter_list_item_for_position(event.position, cx)
        {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        if event.button == MouseButton::Left
            && self.open_front_matter_type_menu_for_position(event.position, cx)
        {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        if event.button == MouseButton::Left {
            if let Some(&(_, start_line, action)) = self
                .code_block_display_action_areas
                .iter()
                .find(|(bounds, _, _)| bounds.contains(&event.position))
            {
                self.toggle_code_block_display_action(start_line, action, cx);
                cx.stop_propagation();
                return;
            }
            if self.start_code_block_horizontal_scrollbar_drag(event.position, cx) {
                self.is_selecting = false;
                self.selection_drag_position = None;
                cx.stop_propagation();
                return;
            }
        }

        if self.active_tab_supports_text_buffer() {
            window.focus(&self.focus_handle(cx));
        }

        if event.button == MouseButton::Left && self.line_format_toolbar_line.is_some() {
            self.dismiss_line_format_toolbar(cx);
        }

        if event.button == MouseButton::Left
            && event.click_count == 1
            && !event.modifiers.shift
            && !event.modifiers.secondary()
            && self.append_line_after_final_rendered_block(event.position, cx)
        {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        if event.button == MouseButton::Left {
            self.suppress_fold_for_empty_selection_drag = false;
            self.selection_drag_has_moved = false;

            if let Some((_, line_idx, action)) = self
                .generated_preview_action_areas
                .iter()
                .find(|(bounds, _, _)| bounds.contains(&event.position))
                .copied()
            {
                match action {
                    GeneratedPreviewAction::Fullscreen => {
                        self.toggle_generated_preview_overlay(line_idx);
                    }
                    GeneratedPreviewAction::Restore => {
                        if let Some(cache_key) = self.generated_preview_cache_key_for_line(line_idx)
                        {
                            self.reset_generated_preview_view(&cache_key);
                        }
                    }
                    GeneratedPreviewAction::ZoomIn => {
                        if let Some(cache_key) = self.generated_preview_cache_key_for_line(line_idx)
                        {
                            self.zoom_generated_preview_view(&cache_key, 1.25);
                        }
                    }
                    GeneratedPreviewAction::ZoomOut => {
                        if let Some(cache_key) = self.generated_preview_cache_key_for_line(line_idx)
                        {
                            self.zoom_generated_preview_view(&cache_key, 0.8);
                        }
                    }
                    GeneratedPreviewAction::Copy => {
                        self.copy_code_block_group_to_clipboard(line_idx, cx);
                    }
                    GeneratedPreviewAction::Code => {
                        if !self.active_tab_is_read_only() {
                            self.toggle_generated_preview_code_view(line_idx);
                        }
                    }
                }
                self.clear_wrap_boundary_affinity();
                self.is_selecting = false;
                self.selection_drag_position = None;
                self.generated_preview_drag_state = None;
                cx.notify();
                return;
            }

            if let Some(line_idx) = self
                .image_preview_zoom_areas
                .iter()
                .find(|(bounds, _)| bounds.contains(&event.position))
                .map(|(_, line)| *line)
            {
                if self.toggle_image_preview_overlay(line_idx) {
                    self.clear_wrap_boundary_affinity();
                    self.is_selecting = false;
                    self.selection_drag_position = None;
                    self.image_drag_state = None;
                    cx.notify();
                    return;
                }
            }

            if let Some(line_idx) = self
                .generated_preview_zoom_areas
                .iter()
                .find(|(bounds, _)| bounds.contains(&event.position))
                .map(|(_, line)| *line)
            {
                if self.toggle_generated_preview_overlay(line_idx) {
                    self.clear_wrap_boundary_affinity();
                    self.is_selecting = false;
                    self.selection_drag_position = None;
                    self.image_drag_state = None;
                    cx.notify();
                    return;
                }
            }

            if let Some((_, line_idx)) = self
                .generated_preview_image_areas
                .iter()
                .find(|(bounds, _)| bounds.contains(&event.position))
                .copied()
                && let Some(cache_key) = self.generated_preview_cache_key_for_line(line_idx)
            {
                if event.click_count >= 2 && self.toggle_generated_preview_overlay(line_idx) {
                    self.clear_wrap_boundary_affinity();
                    self.is_selecting = false;
                    self.selection_drag_position = None;
                    self.generated_preview_drag_state = None;
                    cx.notify();
                    return;
                }
                let view = self.generated_preview_view(&cache_key);
                self.generated_preview_drag_state = Some(GeneratedPreviewDragState {
                    cache_key,
                    start_position: event.position,
                    start_pan_x: view.pan_x,
                    start_pan_y: view.pan_y,
                });
                self.clear_wrap_boundary_affinity();
                self.is_selecting = false;
                self.selection_drag_position = None;
                cx.notify();
                return;
            }
        }

        if self.active_tab_is_image() && event.button == MouseButton::Left {
            if event.click_count >= 2
                && self
                    .active_image_bounds
                    .is_some_and(|bounds| bounds.contains(&event.position))
                && self.open_active_image_preview_overlay()
            {
                self.image_drag_state = None;
                cx.notify();
                return;
            }
            if event.click_count >= 2
                && self
                    .active_image_bounds
                    .is_some_and(|bounds| !bounds.contains(&event.position))
            {
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

        if let Some(&(_, start_line)) = self
            .code_block_copy_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.copy_code_block_group_to_clipboard(start_line, cx);
            return;
        }

        self.selected_handle = None;
        if event.button == MouseButton::Left && self.line_diff_selection.take().is_some() {
            self.line_diff_selection_dragging = false;
            cx.notify();
        }
        if let Some(&(_, line_idx, is_image_fold)) = self
            .gutter_fold_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
        {
            if is_image_fold {
                self.toggle_image_fold(line_idx, cx);
            } else if self
                .decorations
                .get(line_idx)
                .and_then(|decoration| decoration.block_preview.as_ref())
                .is_some_and(|preview| preview.is_anchor)
            {
                self.toggle_generated_preview_fold(line_idx, cx);
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

        if !self.active_tab_is_read_only()
            && let Some(&(_, table_start, col)) = self
                .table_column_border_segments
                .iter()
                .find(|(bounds, _, _)| bounds.contains(&event.position))
        {
            self.begin_table_handle_press(
                (table_start, true, col),
                event.position,
                event.modifiers.shift,
                event.modifiers.secondary(),
                cx,
            );
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        if !self.active_tab_is_read_only()
            && let Some(&(_, table_start, visual_row)) = self
                .table_row_border_segments
                .iter()
                .find(|(bounds, _, _)| bounds.contains(&event.position))
        {
            self.begin_table_handle_press(
                (table_start, false, visual_row),
                event.position,
                event.modifiers.shift,
                event.modifiers.secondary(),
                cx,
            );
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        if let Some(line_idx) = self
            .image_click_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
        {
            if event.button == MouseButton::Left
                && event.click_count >= 2
                && self.toggle_image_preview_overlay(line_idx)
            {
                self.clear_wrap_boundary_affinity();
                self.is_selecting = false;
                self.selection_drag_position = None;
                self.image_drag_state = None;
                cx.notify();
                return;
            }
            if self.select_image_syntax_on_line(line_idx) {
                cx.notify();
                return;
            }
        }

        if let Some((checkbox_bounds, line)) = self
            .checkbox_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .copied()
        {
            self.checkpoint();
            if let Some((bounds, table, row, col)) = self
                .table_click_areas
                .iter()
                .find(|(bounds, _, _, _)| bounds.contains(&event.position))
                .copied()
                .or_else(|| {
                    let table = self
                        .decorations
                        .get(line)?
                        .table_info
                        .as_ref()?
                        .table_start_line;
                    let row = self.table_visual_row_for_source_line(table, line)?;
                    nearest_table_cell_in_row(&self.table_click_areas, table, row, event.position)
                })
            {
                let cell_offset = self
                    .table_cell_offset_for_position(
                        window,
                        table,
                        row,
                        col,
                        &bounds,
                        event.position,
                    )
                    .and_then(|offset| {
                        self.table_source_line_for_visual_row(table, row)
                            .and_then(|line| {
                                self.decorations
                                    .get(line)?
                                    .table_info
                                    .as_ref()
                                    .and_then(|info| {
                                        info.cell_ranges.get(col).map(|range| {
                                            offset.saturating_sub(
                                                self.buffer.line_start(line) + range.start,
                                            )
                                        })
                                    })
                            })
                    })
                    .unwrap_or(0);
                self.toggle_table_cell_checkbox(table, row, col, cell_offset);
            } else {
                self.toggle_checkbox(line);
            }
            self.reparse();
            self.hovering_checkbox = true;
            self.hovering_checkbox_line = Some(line);
            self.hovering_checkbox_bounds = Some(checkbox_bounds);
            self.pause_blinking(cx);
            return;
        }

        self.clear_vertical_move_goal_x();

        if event.button == MouseButton::Left {
            if let Some(&(_, boundary)) = self
                .table_boundary_caret_areas
                .iter()
                .find(|(bounds, _)| bounds.contains(&event.position))
            {
                self.clear_extra_cursors();
                self.clear_wrap_boundary_affinity();
                self.marked_range = None;
                self.extra_marked_ranges.clear();
                self.selected_handle = None;
                if self.focus_table_boundary(boundary) {
                    self.table_boundary_drag_origin = Some(boundary);
                    self.is_selecting = true;
                    self.selection_drag_has_moved = false;
                    self.selection_drag_position = Some(event.position);
                    self.last_mouse_down_target = Some((event.position, self.cursor.offset));
                    self.sync_selection_drag_scroll_task(cx);
                    self.pause_blinking(cx);
                    cx.notify();
                    return;
                }
            }
            self.table_boundary_caret = None;
            if let Some(&(bounds, table_start, visual_row, col)) = self
                .table_click_areas
                .iter()
                .find(|(bounds, _, _, _)| bounds.contains(&event.position))
            {
                self.table_cell_selection = None;
                self.table_handles.selection = None;
                if let Some(offset) = self.table_cell_offset_for_position(
                    window,
                    table_start,
                    visual_row,
                    col,
                    &bounds,
                    event.position,
                ) {
                    self.last_mouse_down_target = Some((event.position, offset));
                    if event.modifiers.secondary() {
                        let (line, column) = self.buffer.offset_to_line_col(offset);
                        let footnote_label = self
                            .decorations
                            .get(line)
                            .and_then(|decoration| {
                                table_cell_footnote_label_for_click(decoration, column)
                            })
                            .map(str::to_string);
                        if let Some(label) = footnote_label
                            && self.navigate_to_footnote_label(&label, cx)
                        {
                            self.is_selecting = false;
                            return;
                        }
                        let link_target = self
                            .decorations
                            .get(line)
                            .and_then(|decoration| {
                                crate::markdown::link_target_at_column(decoration, column)
                            })
                            .map(str::to_string);
                        if let Some(target) = link_target
                            && self.navigate_markdown_target(&target, true, cx)
                        {
                            self.is_selecting = false;
                            return;
                        }
                    }
                    ime_debug_log(format!(
                        "table cell mouse_down table_start={} visual_row={} col={} offset={} shift={} click_count={}",
                        table_start,
                        visual_row,
                        col,
                        offset,
                        event.modifiers.shift,
                        event.click_count
                    ));
                    if event.click_count >= 3 {
                        self.clear_extra_cursors();
                        self.clear_wrap_boundary_affinity();
                        self.select_logical_line_at_offset(offset, cx);
                        self.selection_drag_position = None;
                        self.is_selecting = false;
                    } else if event.click_count == 2 {
                        self.clear_extra_cursors();
                        self.clear_wrap_boundary_affinity();
                        self.select_word_at_offset(offset);
                        self.is_selecting = false;
                    } else if event.modifiers.shift {
                        self.maybe_schedule_preview_reveal_for_click(
                            self.cursor.offset,
                            offset,
                            event.position,
                        );
                        let offset =
                            self.table_visible_selection_target(self.cursor.anchor, offset);
                        self.cursor.select_to(offset, &self.buffer);
                        self.is_selecting = false;
                    } else {
                        self.maybe_schedule_preview_reveal_for_click(
                            self.cursor.offset,
                            offset,
                            event.position,
                        );
                        self.cursor.move_to(offset, &self.buffer);
                        self.is_selecting = true;
                        self.selection_drag_position = Some(event.position);
                        self.sync_selection_drag_scroll_task(cx);
                    }
                    cx.notify();
                    return;
                }
            }
        }

        if event.button == MouseButton::Left && event.click_count >= 2 {
            self.clear_extra_cursors();
            self.table_handles.selection = None;
            self.clear_wrap_boundary_affinity();
            let offset = self.offset_for_position(event.position);
            self.last_mouse_down_target = Some((event.position, offset));
            if event.click_count >= 3 {
                self.select_logical_line_at_offset(offset, cx);
            } else {
                self.select_word_at_offset(offset);
            }
            self.selection_drag_position = None;
            self.is_selecting = false;
            cx.notify();
            return;
        }

        self.is_selecting = true;
        let prev_offset = self.cursor.offset;
        self.table_cell_selection = None;
        self.table_handles.selection = None;
        let offset = self.offset_for_position(event.position);
        self.selection_drag_position = Some(event.position);
        self.sync_selection_drag_scroll_task(cx);
        self.last_mouse_down_target = Some((event.position, offset));
        self.update_wrap_boundary_affinity_for_position(offset, event.position);
        if event.modifiers.secondary() {
            let (line, col) = self.buffer.offset_to_line_col(offset);
            let footnote_label = self.decorations.get(line).and_then(|dec| {
                dec.footnote_references
                    .iter()
                    .find_map(|footnote_reference| {
                        if col < footnote_reference.full_range.start
                            || col > footnote_reference.full_range.end
                        {
                            return None;
                        }
                        Some(footnote_reference.label.clone())
                    })
            });
            if let Some(label) = footnote_label
                && self.navigate_to_footnote_label(&label, cx)
            {
                self.is_selecting = false;
                return;
            }

            let wiki_link_target = self.decorations.get(line).and_then(|dec| {
                dec.wiki_links.iter().find_map(|wiki_link| {
                    if col < wiki_link.full_range.start || col > wiki_link.full_range.end {
                        return None;
                    }
                    Some(wiki_link.target_path.clone())
                })
            });
            if let Some(target) = wiki_link_target
                && self.navigate_markdown_target(&target, true, cx)
            {
                self.is_selecting = false;
                return;
            }

            let link_url = self
                .decorations
                .get(line)
                .and_then(|decoration| crate::markdown::link_target_at_column(decoration, col))
                .map(str::to_string);
            if let Some(url) = link_url
                && self.navigate_markdown_target(&url, true, cx)
            {
                self.is_selecting = false;
                return;
            }
            self.add_cursor_at(offset, offset);
            self.is_selecting = false;
            cx.notify();
            return;
        }

        self.clear_extra_cursors();
        if event.modifiers.shift {
            self.maybe_schedule_preview_reveal_for_click(prev_offset, offset, event.position);
            self.cursor.select_to(offset, &self.buffer);
        } else {
            self.maybe_schedule_preview_reveal_for_click(prev_offset, offset, event.position);
            self.cursor.move_to(offset, &self.buffer);
            self.maybe_auto_select_link_url(prev_offset, true, cx);
        }
        cx.notify();
    }

    fn select_image_syntax_on_line(&mut self, line_idx: usize) -> bool {
        let Some(target) = self.image_menu_target_on_line(line_idx) else {
            return false;
        };

        self.cursor.anchor = target.full_range.start.min(self.buffer.len());
        self.cursor.offset = target.full_range.end.min(self.buffer.len());
        true
    }

    fn generated_preview_cache_key_for_line(&self, line_idx: usize) -> Option<String> {
        self.decorations
            .get(line_idx)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .map(|preview| preview.cache_key.clone())
    }

    /// Toggles a generated preview between its rendered view and its source
    /// (edit) view. When the cursor is already inside the preview's source it
    /// moves out of the block so the chart/formula renders again; otherwise it
    /// moves into the source to start editing.
    fn toggle_generated_preview_code_view(&mut self, line_idx: usize) -> bool {
        if self.active_tab_is_read_only() {
            return false;
        }
        let Some(preview) = self
            .decorations
            .get(line_idx)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .cloned()
        else {
            return false;
        };
        let cursor_line = self.buffer.offset_to_line_col(self.cursor.offset).0;
        let in_source = (preview.start_line..=preview.end_line).contains(&cursor_line)
            && preview.source_range.start <= self.cursor.offset
            && self.cursor.offset <= preview.source_range.end;
        let entering = !in_source;
        let target_offset = if in_source {
            // Exit edit view: move the cursor out of the block so it renders.
            let line_count = self.buffer.line_count();
            if preview.end_line + 1 < line_count {
                self.buffer.line_start(preview.end_line + 1)
            } else if preview.start_line > 0 {
                self.buffer.line_end(preview.start_line - 1)
            } else {
                preview.source_range.start.min(self.buffer.len())
            }
        } else {
            // Enter edit view.
            preview.source_range.start.min(self.buffer.len())
        };
        self.folded_sections.remove(&preview.start_line);
        self.folded_section_ends.remove(&preview.start_line);
        self.clear_extra_cursors();
        // Pin the block's TOP line (not the moving cursor line) so switching
        // edit↔render keeps the chart/formula visually anchored in place. The
        // cursor lands inside or just after the block, but anchoring its line
        // would scroll the view by the block's height change; anchoring the
        // block top leaves it put while the content below reflows. Capture the
        // preserved scroll AFTER the anchor (mirrors the click path).
        let block_top_offset = self.buffer.line_start(preview.start_line);
        self.capture_focus_scroll_anchor(block_top_offset);
        let preserve_scroll = self.scroll_offset;
        self.cursor.anchor = target_offset;
        self.cursor.offset = target_offset;
        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        self.pending_center_cursor_in_view_immediately = false;
        self.pending_preview_reveal = Some(PendingPreviewReveal {
            scroll_offset: preserve_scroll,
        });
        // Entering edit view: pre-track this preview as the code-mode key (and
        // reset its zoom/pan) now so the first prepaint already sees focused ==
        // tracked and does not fire an extra reset+notify frame, which flashed
        // the render on entry.
        if entering {
            self.reset_generated_preview_view(&preview.cache_key);
            self.code_mode_preview_key = Some(preview.cache_key.clone());
        }
        self.clear_line_display_info();
        true
    }

    fn image_preview_bounds_for_line(&self, line_idx: usize) -> Option<Bounds<Pixels>> {
        self.image_click_areas
            .iter()
            .find(|(_, line)| *line == line_idx)
            .map(|(bounds, _)| bounds.clone())
    }

    fn toggle_image_preview_overlay(&mut self, line_idx: usize) -> bool {
        let Some(cache_key) = self
            .decorations
            .get(line_idx)
            .and_then(|decoration| decoration.image_path.as_ref())
            .cloned()
        else {
            return false;
        };

        if self.preview_overlay_state().is_some_and(|overlay| {
            overlay.line_idx == Some(line_idx) && overlay.cache_key == cache_key
        }) {
            self.close_preview_overlay();
            return true;
        }

        let has_preview_bounds = self.image_preview_bounds_for_line(line_idx).is_some();
        if !has_preview_bounds
            || !self
                .image_cache
                .get(&cache_key)
                .is_some_and(|entry| entry.is_some())
        {
            return false;
        }

        self.open_preview_overlay(cache_key, Some(line_idx));
        true
    }

    fn open_active_image_preview_overlay(&mut self) -> bool {
        let Some(cache_key) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| tab.path.as_ref())
            .map(|path| path.to_string_lossy().into_owned())
        else {
            return false;
        };
        if !self
            .image_cache
            .get(&cache_key)
            .is_some_and(|entry| entry.is_some())
        {
            return false;
        }
        self.open_preview_overlay(cache_key, None);
        true
    }

    fn toggle_generated_preview_overlay(&mut self, line_idx: usize) -> bool {
        if self
            .preview_overlay_state()
            .is_some_and(|overlay| overlay.line_idx == Some(line_idx))
        {
            self.close_preview_overlay();
            return true;
        }
        let Some(preview) = self
            .decorations
            .get(line_idx)
            .and_then(|decoration| decoration.block_preview.as_ref())
        else {
            return false;
        };
        if !self
            .image_cache
            .get(&preview.cache_key)
            .is_some_and(|entry| entry.is_some())
        {
            return false;
        }
        self.open_preview_overlay(preview.cache_key.clone(), Some(line_idx));
        true
    }

    fn open_preview_overlay(&mut self, cache_key: String, line_idx: Option<usize>) {
        let math = line_idx
            .and_then(|line| self.decorations.get(line))
            .and_then(|decoration| decoration.block_preview.as_ref())
            .is_some_and(|preview| preview.kind == markdown::BlockPreviewKind::Math);
        self.line_format_toolbar_line = None;
        self.line_format_toolbar_bounds = None;
        self.preview_overlay = Some(PreviewOverlayState {
            cache_key,
            line_idx,
            view: ImageViewState {
                zoom_factor: if math { 2.0 } else { 1.0 },
                ..ImageViewState::default()
            },
        });
        self.preview_overlay_press = None;
        self.preview_overlay_bounds = None;
        self.preview_overlay_image_bounds = None;
    }

    fn maybe_schedule_preview_reveal_for_click(
        &mut self,
        previous_offset: usize,
        next_offset: usize,
        position: Point<Pixels>,
    ) {
        let previous_preview = self.preview_cache_key_for_offset(previous_offset);
        let next_preview = self.preview_cache_key_for_offset(next_offset);
        if previous_preview == next_preview {
            return;
        }
        if previous_preview.is_none() && next_preview.is_none() {
            return;
        }

        self.pending_scroll_to_cursor = false;
        self.pending_input_bounds = Some(self.provisional_input_bounds_for_click(position));
        self.pending_preview_reveal = Some(PendingPreviewReveal {
            scroll_offset: self.scroll_offset,
        });
    }

    /// Records the focused line's current content-space top so an upcoming
    /// focus-driven fold/expand of content above it can be compensated on the
    /// next layout, keeping that line visually still. Used by both mouse clicks
    /// and keyboard navigation. Skipped while the layout is dirty (geometry
    /// cache unreliable) or when the line is currently off-screen, so that a
    /// real scroll-into-view is never suppressed.
    fn capture_focus_scroll_anchor(&mut self, offset: usize) {
        self.fold_scroll_guard = None;
        if self.layout_dirty {
            return;
        }
        let line = self
            .buffer
            .offset_to_line_col(offset.min(self.buffer.len()))
            .0;
        let rel_y_before = self.text_y_offset_for_line(line);
        let screen_y = f32::from(self.content_origin.y) + rel_y_before - self.scroll_offset;
        let viewport_top = f32::from(self.content_origin.y);
        let viewport_bottom = viewport_top + self.last_viewport_height;
        if screen_y < viewport_top || screen_y > viewport_bottom {
            return;
        }
        let scroll_before = self.scroll_offset;
        // Pre-apply the predictable part of the compensation (a generated
        // preview above the new cursor collapsing from edit mode to its rendered
        // image) so the very first painted frame is already correct and the view
        // does not flash. Other focus-driven height changes above the caret are
        // compensated within the same frame by the markdown scroll-anchor delta
        // in `element.rs`; the exact post-layout anchor below fixes any residual.
        let predicted = self.predict_focus_fold_delta(line, offset);
        if predicted.abs() > 0.5 {
            self.scroll_offset = (scroll_before + predicted).max(0.0);
        }
        self.pending_click_anchor = Some(ClickScrollAnchor {
            line,
            rel_y_before,
            scroll_before,
        });
    }

    /// Predicts the content-height change above `anchor_line` caused by the
    /// focus moving to `target_offset` — specifically a generated preview that
    /// was in edit (code) mode collapsing to its rendered image. Returns the
    /// signed delta (negative when content shrinks). Other focus changes are
    /// left to the exact post-layout correction.
    fn predict_focus_fold_delta(&self, anchor_line: usize, target_offset: usize) -> f32 {
        let Some(old_key) = self.code_mode_preview_key.clone() else {
            return 0.0;
        };
        // Still editing the same preview → no collapse.
        if self
            .focused_code_mode_preview_key_at(target_offset)
            .as_deref()
            == Some(old_key.as_str())
        {
            return 0.0;
        }
        let Some((start_line, end_line, kind, cache_key)) =
            self.decorations.iter().find_map(|dec| {
                dec.block_preview
                    .as_ref()
                    .filter(|preview| preview.is_anchor && preview.cache_key == old_key)
                    .map(|preview| {
                        (
                            preview.start_line,
                            preview.end_line,
                            preview.kind,
                            preview.cache_key.clone(),
                        )
                    })
            })
        else {
            return 0.0;
        };
        // A collapse at or below the anchor does not shift it.
        if start_line >= anchor_line {
            return 0.0;
        }
        let line_count = self.buffer.line_count();
        let old_height = self.line_y_offset((end_line + 1).min(line_count))
            - self.line_y_offset(start_line.min(line_count));
        let Some(Some(image)) = self.image_cache.get(&cache_key) else {
            return 0.0;
        };
        let size = image.size(0);
        let (img_w, img_h) = (size.width.0 as f32, size.height.0 as f32);
        if img_w <= 0.0 || img_h <= 0.0 {
            return 0.0;
        }
        let (_, _, _, bg_height) = helpers::fit_generated_preview_size(
            img_w,
            img_h,
            kind,
            self.last_text_width,
            self.scaled(self.styles.code_block.generated_preview_min_height_px),
            self.scaled(self.styles.code_block.generated_preview_max_height_px),
            self.scaled(self.styles.code_block.generated_preview_padding_x_px),
            self.scaled(self.styles.code_block.generated_preview_padding_y_px),
        );
        let (pad_top, pad_bottom) = self.block_padding_for_line(start_line);
        let new_height = bg_height + pad_top + pad_bottom;
        new_height - old_height
    }

    /// Like `focused_code_mode_preview_key`, but evaluated at an arbitrary
    /// offset (used before the cursor has actually moved on click).
    fn focused_code_mode_preview_key_at(&self, offset: usize) -> Option<String> {
        if !self.markdown_syntax_expansion_allowed() {
            return None;
        }
        let line = self
            .buffer
            .offset_to_line_col(offset.min(self.buffer.len()))
            .0;
        let preview = self.decorations.get(line)?.block_preview.as_ref()?;
        if (preview.start_line..=preview.end_line).contains(&line)
            && preview.source_range.start <= offset
            && offset <= preview.source_range.end
        {
            Some(preview.cache_key.clone())
        } else {
            None
        }
    }

    /// Scrolls to the cursor for keyboard navigation while preserving visual
    /// stability: ordinary left/right/up/down navigation must not change the
    /// editor scroll position unless the cursor enters the top/bottom third of
    /// the visible editor. Avoid the post-layout click anchor here because that
    /// anchor intentionally mutates `scroll_offset` after syntax fold/expand,
    /// which reads as view jitter during keyboard navigation.
    pub(crate) fn navigate_scroll_to_cursor(&mut self) {
        self.minimap_state.cancel_drag();
        self.cancel_pending_scroll_navigation();
        self.sync_front_matter_group_focus();
        self.scroll_to_cursor();
    }

    fn preview_cache_key_for_offset(&self, offset: usize) -> Option<&str> {
        let line = self
            .buffer
            .offset_to_line_col(offset.min(self.buffer.len()))
            .0;
        self.decorations
            .get(line)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .map(|preview| preview.cache_key.as_str())
    }

    fn provisional_input_bounds_for_click(&self, _position: Point<Pixels>) -> Bounds<Pixels> {
        let anchor_offset = self.buffer.clamp_offset(self.cursor.offset);
        let (line, col) = self.buffer.offset_to_line_col(anchor_offset);
        let text_top_y = f32::from(self.content_origin.y) + self.text_y_offset_for_line(line)
            - self.scroll_offset;

        if let Some(info) = self
            .line_display_info
            .get(line)
            .and_then(|info| info.as_ref())
        {
            let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
            let affinity = self
                .wrap_boundary_affinity
                .and_then(|current| (current.offset == anchor_offset).then_some(current.kind));
            let pos = Self::visual_position_for_wrapped_display(
                &info.wrapped_line,
                &info.display_text,
                disp_col,
                info.line_height,
                info.indent_px,
                &info.wrap_boundary_upstream_xs,
                affinity,
            )
            .unwrap_or_default();
            return Bounds::new(
                point(
                    px(info.content_x + f32::from(pos.x)),
                    px(text_top_y + f32::from(pos.y)),
                ),
                size(
                    px(info.font_size * 0.6),
                    info.wrapped_line.row_height(
                        info.wrapped_line.row_for_y(pos.y, px(info.line_height)),
                        px(info.line_height),
                    ),
                ),
            );
        }

        let block_type = if line < self.decorations.len() {
            &self.decorations[line].block_type
        } else {
            &BlockType::Normal
        };
        let line_height = self.line_height_for_block(block_type);
        let x = self.last_content_left + col as f32 * self.base_font_size() * 0.6;
        Bounds::new(
            point(px(x), px(text_top_y)),
            size(px(self.base_font_size() * 0.6), px(line_height)),
        )
    }

    pub(super) fn skip_folded_caret_navigation(&mut self, previous: &[Cursor]) {
        self.skip_folded_navigation(previous, false);
    }

    pub(super) fn skip_folded_selection_navigation(&mut self, previous: &[Cursor]) {
        self.skip_folded_navigation(previous, true);
    }

    fn skip_folded_navigation(&mut self, previous: &[Cursor], selecting: bool) {
        let folded_ranges: Vec<_> = self
            .folded_sections
            .iter()
            .filter_map(|&line| self.fold_range_for_line(line))
            .collect();
        let hidden: Vec<_> = if self.collapsed_front_matter_range().is_none()
            && !self.markdown_syntax_expansion_allowed()
        {
            self.decorations
                .first()
                .and_then(|d| d.table_info.as_ref())
                .filter(|table| table.is_front_matter)
                .map(|table| {
                    (table.table_start_line..table.table_end_line)
                        .filter(|&line| self.front_matter_navigation_skips_line(line))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut changed = false;
        for (cursor, previous) in std::iter::once(&mut self.cursor)
            .chain(&mut self.extra_cursors)
            .zip(previous)
        {
            let skipped = super::fold::skip_collapsed_fold_navigation(
                &self.buffer,
                &folded_ranges,
                previous.offset,
                cursor,
                selecting,
            );
            changed |= skipped
                || super::fold::skip_front_matter_navigation_head(
                    &self.buffer,
                    None,
                    |line| hidden.contains(&line),
                    previous.offset,
                    cursor,
                    selecting,
                );
        }
        if changed {
            self.reset_cursor_visual_targeting();
        }
    }

    pub(crate) fn maybe_auto_select_link_url(
        &mut self,
        prev_offset: usize,
        also_select_desc: bool,
        cx: &mut Context<Self>,
    ) {
        // Arrow navigation reaches this hook before syntax focus is updated.
        // Skip folded metadata in the actual movement direction, without unfolding it.
        if let Some(range) = self.collapsed_front_matter_range()
            && let Some(offset) = super::fold::front_matter_fold_navigation_offset(
                &self.buffer,
                &range,
                prev_offset,
                self.cursor.offset,
            )
        {
            self.cursor.move_to(offset, &self.buffer);
            self.reset_cursor_visual_targeting();
            return;
        }
        if !self.markdown_syntax_expansion_allowed()
            && let Some(offset) = super::fold::front_matter_hidden_navigation_offset(
                &self.buffer,
                prev_offset,
                self.cursor.offset,
                |line| self.front_matter_navigation_skips_line(line),
            )
        {
            self.cursor.move_to(offset, &self.buffer);
            self.reset_cursor_visual_targeting();
            return;
        }
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
                    let came_from_outside_desc =
                        prev_offset < desc_abs_start || prev_offset > desc_abs_end;
                    if came_from_outside_desc
                        && new_offset >= desc_abs_start
                        && new_offset <= desc_abs_end
                    {
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

                    if new_offset < full_abs_start || new_offset > full_abs_end {
                        continue;
                    }

                    let entered_from_outside =
                        prev_offset < full_abs_start || prev_offset > full_abs_end;
                    if entered_from_outside
                        && new_offset >= inner_abs_start
                        && new_offset <= inner_abs_end
                        && inner_abs_start < inner_abs_end
                    {
                        self.cursor.anchor = inner_abs_start;
                        self.cursor.offset = inner_abs_end;
                        self.is_selecting = false;
                    }
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
        window.prevent_default();
        cx.stop_propagation();
        self.is_selecting = false;
        self.cancel_line_drag(cx);
        self.dismiss_line_format_toolbar(cx);
        if let Some(layout) = self
            .line_diff_selection_layouts
            .iter()
            .find(|layout| layout.contains_position(event.position))
        {
            let (offset, affinity) = layout.selection_at(event.position);
            let selection = self
                .line_diff_selection
                .filter(|selection| {
                    selection.hunk_index == layout.hunk_index && selection.range().contains(&offset)
                })
                .unwrap_or(line_diff::LineDiffSelection {
                    hunk_index: layout.hunk_index,
                    anchor: offset,
                    head: offset,
                    affinity,
                    ..Default::default()
                });
            self.finish_line_diff_selection_move(selection, cx);
            self.open_format_context_menu(event.position, cx);
            return;
        }
        self.line_diff_selection = None;
        self.line_diff_selection_dragging = false;
        if let Some((_, table, is_column, index)) = self
            .table_border_segment_areas
            .iter()
            .find(|(bounds, _, _, _)| bounds.contains(&event.position))
            .copied()
        {
            self.open_table_border_context_menu((table, is_column, index), event.position, cx);
            return;
        }
        if !self
            .table_click_areas
            .iter()
            .any(|(bounds, _, _, _)| bounds.contains(&event.position))
        {
            self.table_cell_selection = None;
        }

        if self.active_tab_is_image() {
            self.open_context_menu(
                ContextMenu {
                    position: event.position,
                    kind: ContextMenuKind::Image,
                },
                true,
                cx,
            );
            return;
        }

        if let Some(&(bounds, table_start, visual_row, col)) = self
            .table_click_areas
            .iter()
            .find(|(bounds, _, _, _)| bounds.contains(&event.position))
        {
            if let Some(selection) = self.table_cell_selection.as_ref()
                && selection.contains(table_start, visual_row, col)
            {
                self.table_handles.selection = None;
                self.open_context_menu(
                    ContextMenu {
                        position: event.position,
                        kind: ContextMenuKind::Table {
                            table_start_line: table_start,
                            row: visual_row,
                            col,
                        },
                    },
                    true,
                    cx,
                );
                return;
            }
            self.table_cell_selection = None;
            self.table_handles.selection = None;
            if let Some(offset) = self.table_cell_offset_for_position(
                window,
                table_start,
                visual_row,
                col,
                &bounds,
                event.position,
            ) {
                preserve_context_menu_selection(&mut self.cursor, offset, &self.buffer);
            }
            let is_front_matter = self
                .decorations
                .get(table_start)
                .and_then(|dec| dec.table_info.as_ref())
                .is_some_and(|ti| ti.is_front_matter);
            self.open_context_menu(
                ContextMenu {
                    position: event.position,
                    kind: ContextMenuKind::Table {
                        table_start_line: table_start,
                        row: visual_row,
                        col,
                    },
                },
                true,
                cx,
            );
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
        preserve_context_menu_selection(&mut self.cursor, offset, &self.buffer);

        self.open_format_context_menu(event.position, cx);
    }

    pub(crate) fn select_word_at_offset(&mut self, offset: usize) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let line_start = self.buffer.line_start(line);
        let line_content = self.buffer.line_content(line);
        let range = Self::word_selection_range(line_content, col);
        self.cursor.anchor = line_start + range.start;
        self.cursor.offset = line_start + range.end;
    }

    fn word_selection_range(line_content: &str, col: usize) -> Range<usize> {
        crate::cursor::word_range_at(line_content, col)
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_mouse_up_inner(event, window, cx);
        self.refresh_front_matter_input_geometry(window, self.front_matter_input_viewport_bounds());
    }

    fn on_mouse_up_inner(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview_overlay.is_some() {
            self.on_preview_overlay_mouse_up(event, true, cx);
            cx.stop_propagation();
            return;
        }
        if self.line_diff_selection_dragging && event.button == MouseButton::Left {
            self.line_diff_selection_dragging = false;
            cx.stop_propagation();
            return;
        }
        if self.finish_table_handle_press(event, window, cx) {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if self.suppress_next_editor_mouse_up || self.context_menu.is_some() {
            self.suppress_next_editor_mouse_up = false;
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if self.stop_code_block_horizontal_scrollbar_drag(event.position, cx) {
            return;
        }
        if self.stop_editor_scrollbar_drag(event.position, cx) {
            return;
        }

        if let Some(drag) = self.image_drag_state.take() {
            let _ = drag;
            cx.notify();
            return;
        }

        if self.generated_preview_drag_state.take().is_some() {
            cx.notify();
            return;
        }

        if let Some(drag) = self.table_drag.take() {
            match drag {
                TableDragState::DraggingColumn {
                    table_start_line,
                    source_indices,
                    current_x,
                } => {
                    if let Some(insert_at) = self.table_col_insert_at_x(table_start_line, current_x)
                    {
                        if let Some((start, end, lines)) =
                            self.table_lines_for_block(table_start_line)
                        {
                            let new_lines = crate::table_ops::move_selected_columns(
                                &lines,
                                &source_indices,
                                insert_at,
                            );
                            if new_lines != lines {
                                self.replace_table_block(start, end, &new_lines, cx);
                                let count = source_indices.len();
                                let selected_col = insert_at.saturating_sub(
                                    source_indices
                                        .iter()
                                        .filter(|index| **index < insert_at)
                                        .count(),
                                );
                                self.table_handles.selection =
                                    Some(super::table_handles::TableBorderSelection {
                                        table_start_line,
                                        is_column: true,
                                        anchor: selected_col,
                                        indices: (selected_col..selected_col + count).collect(),
                                    });
                            }
                        }
                    }
                }
                TableDragState::DraggingRow {
                    table_start_line,
                    source_indices,
                    current_y,
                } => {
                    if let Some(insert_visual) =
                        self.table_row_insert_at_y(table_start_line, current_y)
                    {
                        if let Some((start, end, lines)) =
                            self.table_lines_for_block(table_start_line)
                        {
                            let new_lines = if self.block_is_front_matter(table_start_line) {
                                if source_indices.len() != 1 {
                                    lines.clone()
                                } else {
                                    crate::table_ops::move_front_matter_row(
                                        &lines,
                                        source_indices[0],
                                        insert_visual,
                                    )
                                }
                            } else {
                                crate::table_ops::move_selected_rows(
                                    &lines,
                                    &source_indices,
                                    insert_visual,
                                )
                            };
                            if new_lines != lines {
                                self.replace_table_block(start, end, &new_lines, cx);
                                let count = source_indices.len();
                                let selected_row = insert_visual.saturating_sub(
                                    source_indices
                                        .iter()
                                        .filter(|index| **index < insert_visual)
                                        .count(),
                                );
                                self.table_handles.selection =
                                    Some(super::table_handles::TableBorderSelection {
                                        table_start_line,
                                        is_column: false,
                                        anchor: selected_row,
                                        indices: (selected_row..selected_row + count).collect(),
                                    });
                            }
                        }
                    }
                }
            }
            cx.notify();
            return;
        }
        let was_selecting = self.is_selecting;
        let had_selection = self.cursor.has_selection();
        self.is_selecting = false;
        self.selection_drag_position = None;
        self.selection_drag_scroll_task = None;
        self.suppress_fold_for_empty_selection_drag =
            was_selecting && self.selection_drag_has_moved && !had_selection;
        self.selection_drag_has_moved = false;
        self.table_boundary_drag_origin = None;
        if was_selecting {
            cx.notify();
        }
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before: Vec<_> = self
            .all_cursor_ranges()
            .into_iter()
            .map(|(head, anchor)| head.min(anchor)..head.max(anchor))
            .chain(self.marked_ranges().cloned())
            .collect();
        self.refresh_front_matter_input_geometry(window, self.front_matter_input_viewport_bounds());
        self.on_mouse_move_inner(event, window, cx);
        let after: Vec<_> = self
            .all_cursor_ranges()
            .into_iter()
            .map(|(head, anchor)| head.min(anchor)..head.max(anchor))
            .chain(self.marked_ranges().cloned())
            .collect();
        if before != after {
            self.refresh_front_matter_input_geometry(
                window,
                self.front_matter_input_viewport_bounds(),
            );
        }
    }

    fn on_mouse_move_inner(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line_diff_hovered_hunk = self
            .line_diff_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, hunk)| *hunk);
        if self.line_diff_hovered_hunk != line_diff_hovered_hunk {
            self.line_diff_hovered_hunk = line_diff_hovered_hunk;
            cx.notify();
        }
        if self.preview_overlay.is_some() {
            self.on_preview_overlay_mouse_move(event, cx);
            cx.stop_propagation();
            return;
        }
        if self.line_diff_selection_dragging
            && let Some(mut selection) = self.line_diff_selection
            && let Some((offset, affinity)) =
                self.line_diff_selection_offset_at(event.position, selection.hunk_index)
        {
            selection.head = offset;
            selection.affinity = affinity;
            selection.goal_x = None;
            self.finish_line_diff_selection_move(selection, cx);
            cx.stop_propagation();
            return;
        }
        if self.update_table_handle_press(event, cx) {
            cx.stop_propagation();
            return;
        }
        if self.runtime_profiler.is_some() {
            self.runtime_profile_mouse_move_events =
                self.runtime_profile_mouse_move_events.wrapping_add(1);
        }
        let toolbar_hover_changed = self.update_toolbar_corner_hover(event.position, cx);
        let track_precise_hover_position = event.modifiers.secondary()
            || self.editor_uses_horizontal_text_scroll()
            || !self.code_block_hover_areas.is_empty()
            || self.preview_overlay.is_some();
        if track_precise_hover_position {
            self.hovered_editor_position = Some(event.position);
        } else {
            self.hovered_editor_position = None;
        }
        let dragging_scrollbar = self.update_editor_scrollbar_drag(event.position, cx);
        let dragging_code_block_scrollbar =
            self.update_code_block_horizontal_scrollbar_drag(event.position, cx);
        let scrollbar_hovered =
            dragging_scrollbar || self.editor_scrollbar_region_contains(event.position);
        self.set_editor_scrollbar_hovered(scrollbar_hovered, cx);
        if self.editor_uses_horizontal_text_scroll() {
            self.sync_code_block_horizontal_scrollbar(cx);
        }
        if dragging_scrollbar || dragging_code_block_scrollbar {
            if toolbar_hover_changed {
                cx.notify();
            }
            return;
        }
        if self
            .line_format_toolbar_bounds
            .is_some_and(|bounds| bounds.contains(&event.position))
        {
            self.set_line_format_toolbar_hovered(true, cx);
            if self.hovering_fold_line.take().is_some() {
                self.sync_gutter_fold_reveal(cx);
                cx.notify();
            }
            return;
        }
        if self.line_format_toolbar_hovered {
            self.set_line_format_toolbar_hovered(false, cx);
        }
        if let Some(drag) = self.generated_preview_drag_state.clone() {
            if event.dragging() {
                let view = self.generated_preview_view_mut(&drag.cache_key);
                view.pan_x = drag.start_pan_x
                    + (f32::from(event.position.x) - f32::from(drag.start_position.x));
                view.pan_y = drag.start_pan_y
                    + (f32::from(event.position.y) - f32::from(drag.start_position.y));
                self.generated_preview_drag_state = Some(drag);
                cx.notify();
                return;
            }
        }

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

        let cmd = event.modifiers.secondary();
        if cmd != self.command_held {
            self.command_held = cmd;
            cx.notify();
        }

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

        let has_interactive_hover_targets = !self.code_block_hover_areas.is_empty()
            || !self.code_block_copy_areas.is_empty()
            || !self.code_block_display_action_areas.is_empty()
            || !self.checkbox_areas.is_empty()
            || !self.image_click_areas.is_empty()
            || !self.generated_preview_click_areas.is_empty()
            || !self.generated_preview_action_areas.is_empty()
            || !self.generated_preview_zoom_areas.is_empty()
            || !self.image_edit_areas.is_empty()
            || !self.gutter_fold_areas.is_empty()
            || !self.table_click_areas.is_empty()
            || !self.table_column_border_segments.is_empty()
            || !self.table_row_border_segments.is_empty();
        if !has_interactive_hover_targets && !self.is_selecting && !self.command_held {
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
            self.sync_generated_preview_action_opacity(cx);
            self.sync_code_block_horizontal_scrollbar(cx);
            cx.notify();
        }
        let hovering_code_block_scrollbar = self
            .code_block_horizontal_scrollbar_drag
            .map(|drag| drag.start_line)
            .or_else(|| {
                self.code_block_horizontal_scrollbar_at_position(event.position)
                    .map(|geometry| geometry.start_line)
                    .filter(|start_line| {
                        *start_line == super::NON_MARKDOWN_HORIZONTAL_SCROLL_TARGET
                            || self.hovering_code_block_start == Some(*start_line)
                    })
            });
        self.set_code_block_horizontal_scrollbar_hovered(hovering_code_block_scrollbar, cx);

        let hovering_code_block_copy = self
            .code_block_copy_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, start_line)| *start_line);
        if hovering_code_block_copy != self.hovering_code_block_copy_start {
            self.hovering_code_block_copy_start = hovering_code_block_copy;
            cx.notify();
        }

        let hovering_code_block_display_action = self
            .code_block_display_action_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
            .map(|(_, start_line, action)| (*start_line, *action));
        if hovering_code_block_display_action != self.hovering_code_block_display_action {
            self.hovering_code_block_display_action = hovering_code_block_display_action;
            cx.notify();
        }

        let hovering_checkbox = self
            .checkbox_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .copied();
        if hovering_checkbox
            != self
                .hovering_checkbox_bounds
                .zip(self.hovering_checkbox_line)
        {
            self.hovering_checkbox_bounds = hovering_checkbox.map(|(bounds, _)| bounds);
            self.hovering_checkbox_line = hovering_checkbox.map(|(_, line)| line);
            self.hovering_checkbox = hovering_checkbox.is_some();
            cx.notify();
        }

        let hovering_image_preview = self
            .image_click_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line);
        if hovering_image_preview != self.hovering_image_preview_line {
            self.hovering_image_preview_line = hovering_image_preview;
            cx.notify();
        }

        let hovering_generated_preview = self
            .generated_preview_click_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
            .or_else(|| {
                let bounds = self.preview_overlay_bounds?;
                if bounds.contains(&event.position) {
                    self.preview_overlay_state()
                        .and_then(|overlay| overlay.line_idx)
                } else {
                    None
                }
            });
        if hovering_generated_preview != self.hovering_generated_preview_line {
            self.hovering_generated_preview_line = hovering_generated_preview;
            self.sync_generated_preview_action_opacity(cx);
            cx.notify();
        }

        let hovering_generated_preview_action = self
            .generated_preview_action_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
            .map(|(_, line, action)| (*line, *action));
        if hovering_generated_preview_action != self.hovering_generated_preview_action {
            self.hovering_generated_preview_action = hovering_generated_preview_action;
            self.sync_generated_preview_action_opacity(cx);
            cx.notify();
        }

        let hovering_preview_overlay_action = self
            .preview_overlay_action_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, action)| *action);
        if hovering_preview_overlay_action != self.hovering_preview_overlay_action {
            self.hovering_preview_overlay_action = hovering_preview_overlay_action;
            cx.notify();
        }

        let hovering_generated_preview_zoom = hovering_generated_preview_action
            .map(|(line, _)| line)
            .or_else(|| {
                self.generated_preview_zoom_areas
                    .iter()
                    .find(|(bounds, _)| bounds.contains(&event.position))
                    .map(|(_, line)| *line)
            });
        if hovering_generated_preview_zoom != self.hovering_generated_preview_zoom_line {
            self.hovering_generated_preview_zoom_line = hovering_generated_preview_zoom;
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

        let line_format_enabled = self.active_tab_is_markdown()
            && !self.active_tab_is_read_only()
            && !self.has_extra_cursors();
        let hovering_format_line = line_format_enabled
            .then(|| {
                self.format_line_areas
                    .iter()
                    .find(|(bounds, _)| bounds.contains(&event.position))
                    .map(|(_, line)| *line)
            })
            .flatten();
        let hovering_format_trigger_line = line_format_enabled
            .then(|| {
                self.format_trigger_areas
                    .iter()
                    .find(|(bounds, _)| bounds.contains(&event.position))
                    .map(|(_, line)| *line)
            })
            .flatten();
        if hovering_format_line != self.hovering_format_line
            || hovering_format_trigger_line != self.hovering_format_trigger_line
        {
            self.update_line_format_hover(hovering_format_line, hovering_format_trigger_line, cx);
        }

        let hovering_fold = self
            .gutter_fold_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
            .map(|(_, line, _)| *line);
        if hovering_fold != self.hovering_fold_line {
            self.hovering_fold_line = hovering_fold;
            self.sync_gutter_fold_reveal(cx);
            cx.notify();
        }

        let fold_hit_padding = self.scaled(self.styles.self_drawn.gutter_fold_hit_padding_px);
        let pane_left = self.last_editor_left;
        let hovering_gutter_fold_area = super::element::gutter_fold_hover_x_contains(
            event.position.x,
            self.gutter_icon_column_left(pane_left),
            self.gutter_icon_column_width(),
            fold_hit_padding,
        );
        if hovering_gutter_fold_area != self.hovering_gutter_fold_area {
            self.hovering_gutter_fold_area = hovering_gutter_fold_area;
            self.sync_gutter_fold_reveal(cx);
            cx.notify();
        }

        let hovering_table = self
            .table_click_areas
            .iter()
            .find(|(bounds, _, _, _)| {
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
            .map(|(_, table_start, _, _)| *table_start)
            .filter(|_| !self.active_tab_is_read_only());
        if hovering_table != self.hovering_table_start {
            self.hovering_table_start = hovering_table;
            cx.notify();
        }

        let hovering_handle = self
            .table_column_border_segments
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
            .map(|(_, tsl, col)| (*tsl, true, *col))
            .or_else(|| {
                self.table_row_border_segments
                    .iter()
                    .find(|(bounds, _, _)| bounds.contains(&event.position))
                    .map(|(_, tsl, row)| (*tsl, false, *row))
            })
            .filter(|_| !self.active_tab_is_read_only());
        if hovering_handle != self.hovering_handle {
            self.hovering_handle = hovering_handle;
            cx.notify();
        }
        if self.is_selecting {
            let old_cursor_offset = self.cursor.offset;
            let old_wrap_affinity = self.wrap_boundary_affinity_for_offset(old_cursor_offset);
            let old_scroll_offset = self.scroll_offset;
            if self
                .last_mouse_down_target
                .is_some_and(|(start_position, _)| {
                    let moved_x = (f32::from(event.position.x) - f32::from(start_position.x)).abs();
                    let moved_y = (f32::from(event.position.y) - f32::from(start_position.y)).abs();
                    moved_x >= 3.0 || moved_y >= 3.0
                })
            {
                self.selection_drag_has_moved = true;
            }
            self.selection_drag_position = Some(event.position);
            self.sync_selection_drag_scroll_task(cx);
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

            let hovered_table_cell = self
                .table_click_areas
                .iter()
                .find(|(bounds, _, _, _)| bounds.contains(&event.position))
                .copied()
                .or_else(|| {
                    let (table, anchor_line, _) = self.table_cell_for_offset(self.cursor.anchor)?;
                    let anchor_row = self.table_visual_row_for_source_line(table, anchor_line)?;
                    nearest_table_cell_in_row(
                        &self.table_click_areas,
                        table,
                        anchor_row,
                        event.position,
                    )
                });
            if let Some((start_position, _)) = self.last_mouse_down_target
                && self.selection_drag_has_moved
                && let Some((_, table, start_row, start_col)) = self
                    .table_click_areas
                    .iter()
                    .find(|(bounds, _, _, _)| bounds.contains(&start_position))
                    .copied()
                && let Some((_, target_table, end_row, end_col)) = hovered_table_cell
                && table == target_table
                && (start_row != end_row || start_col != end_col)
            {
                self.table_cell_selection =
                    Some(super::table_handles::TableCellSelection::between(
                        table,
                        (start_row, start_col),
                        (end_row, end_col),
                    ));
                self.table_handles.selection = None;
                self.cursor.anchor = self.cursor.offset;
                cx.notify();
                return;
            }
            if self.table_cell_selection.is_some() {
                return;
            }
            if self.update_table_boundary_drag_selection(event.position) {
                cx.notify();
                return;
            }

            if self.selection_drag_has_moved && self.table_boundary_drag_origin.is_some() {
                self.table_boundary_caret = None;
            }

            let offset = if let Some((bounds, table_start, visual_row, col)) = hovered_table_cell {
                let position = point(
                    px(f32::from(event.position.x).clamp(
                        f32::from(bounds.origin.x),
                        f32::from(bounds.origin.x + bounds.size.width),
                    )),
                    event.position.y,
                );
                self.table_cell_offset_for_position(
                    window,
                    table_start,
                    visual_row,
                    col,
                    &bounds,
                    position,
                )
                .unwrap_or_else(|| self.offset_for_position(event.position))
            } else {
                self.offset_for_position(event.position)
            };
            let offset = self.table_visible_selection_target(self.cursor.anchor, offset);
            self.update_wrap_boundary_affinity_for_position(offset, event.position);
            if offset != self.cursor.offset {
                self.cursor.select_to(offset, &self.buffer);
            }
            if self.cursor.has_selection() {
                self.suppress_fold_for_empty_selection_drag = false;
            }
            if self.cursor.offset != old_cursor_offset
                || self.wrap_boundary_affinity_for_offset(self.cursor.offset) != old_wrap_affinity
                || (self.scroll_offset - old_scroll_offset).abs() > f32::EPSILON
            {
                cx.notify();
            }
        } else if toolbar_hover_changed {
            cx.notify();
        }
    }

    pub(super) fn table_cell_display_info(
        &self,
        window: &mut Window,
        source_line: usize,
        col: usize,
        cell_inner_width: f32,
    ) -> Option<TableCellDisplayInfo> {
        let dec = self.decorations.get(source_line)?;
        let ti = dec.table_info.as_ref()?;
        let (cell_content, cell_range) = super::table_cell_render::content_with_range(
            ti,
            self.buffer.line_content(source_line),
            col,
        );
        let line_start = self.buffer.line_start(source_line);
        let mut cell_decoration = helpers::table_cell_decoration(dec, &cell_content, &cell_range);
        let mut cell_active_ranges = helpers::table_cell_active_ranges(
            line_start,
            &cell_range,
            &self.table_cell_syntax_focus(),
            &cell_decoration,
        );

        let text_style = window.text_style();
        let editor_styles = &self.styles.editor;
        let configured_font_family = |configured: &'static str| {
            if configured == crate::platform::fonts::SYSTEM_UI {
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

        let mut italic_font = text_style.font();
        italic_font.family = configured_font_family(editor_styles.editor_italic_latin_font_family);
        italic_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_italic_latin_font_family);

        let mut italic_cjk_font = text_style.font();
        italic_cjk_font.family = configured_font_family(editor_styles.editor_body_cjk_font_family);
        italic_cjk_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_body_cjk_font_family);

        let font_size = self.base_font_size();
        let base_char_width = {
            let test_run = TextRun {
                len: 1,
                font: monospace_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_text("x".into(), px(font_size), &[test_run], None, None)
                .ok()
                .and_then(|lines| lines.first().map(|line| f32::from(line.width())))
                .unwrap_or(font_size * 0.6)
        };
        let body_space_width = {
            let test_run = TextRun {
                len: 1,
                font: body_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(test_lines) =
                window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[test_run], None, None)
            {
                test_lines
                    .first()
                    .map(|line| f32::from(line.width()).max(1.0))
                    .unwrap_or_else(|| base_char_width.max(1.0))
            } else {
                base_char_width.max(1.0)
            }
        };
        helpers::add_inline_math_display_replacements(
            &mut cell_decoration,
            &cell_content,
            &self.image_cache,
            body_space_width,
        );
        let line_height = font_size * self.styles.editor.editor_paragraph_line_height_factor;
        let letter_spacing = self.styles.editor.editor_letter_spacing_px;
        let task_prefix_columns = super::table_cell_render::task_prefix_columns(
            base_char_width + letter_spacing,
            font_size * self.styles.checkbox.checkbox_size_factor + letter_spacing.max(0.0),
            self.scaled(self.styles.checkbox.checkbox_text_gap_px),
        );

        let build_cell = |active: &[Range<usize>]| {
            let (folded_raw, d2b_raw, b2d_raw, prefix_collapsed) = helpers::build_display_text(
                &cell_content,
                &cell_decoration,
                active,
                false,
                None,
                0,
                task_prefix_columns,
                task_prefix_columns,
                self.styles.checkbox.checkbox_parent_prefix_chars as usize,
                0,
                self.styles.editor.editor_superscript_scale_factor,
                self.styles.editor.editor_subscript_scale_factor,
                self.styles.editor.editor_superscript_scale_factor,
            );
            let trimmed_range = helpers::table_cell_display_range(
                &folded_raw,
                &cell_content,
                &cell_decoration,
                &b2d_raw,
                active,
            );
            let trim_start = trimmed_range.start;
            let trimmed_len = trimmed_range.len();
            let display_text = folded_raw[trimmed_range.clone()].to_string();
            let display_to_buffer = d2b_raw[trimmed_range].to_vec();
            let buffer_to_display = b2d_raw
                .iter()
                .map(|&display_index| display_index.saturating_sub(trim_start).min(trimmed_len))
                .collect::<Vec<_>>();

            let mut cell_runs = helpers::build_text_runs_for_display(
                &display_text,
                &cell_content,
                &display_to_buffer,
                &cell_decoration,
                active,
                false,
                prefix_collapsed,
                &body_font,
                &body_cjk_font,
                &monospace_font,
                &heading_font,
                &heading_cjk_font,
                &italic_font,
                &italic_cjk_font,
                0,
                font_size,
                &self.theme,
                self.styles.wiki_link.wiki_link_text_color,
                self.styles.wiki_link.wiki_link_text_weight,
                self.styles.editor.editor_list_marker_text_color,
                self.styles.editor.editor_list_marker_font_weight,
                self.styles.editor.editor_heading_marker_text_color,
                None,
                false,
            );
            if ti.is_header {
                for run in &mut cell_runs {
                    run.font.weight = gpui::FontWeight::BOLD;
                }
            }

            (
                display_text,
                display_to_buffer,
                buffer_to_display,
                prefix_collapsed,
                cell_runs,
            )
        };
        let prepared = super::table_cell_render::prepare_cell(
            window,
            &cell_content,
            &cell_decoration,
            &mut cell_active_ranges,
            cell_inner_width,
            font_size,
            letter_spacing,
            line_height,
            &self.image_cache,
            build_cell,
        )
        .ok()?;
        let (display_text, display_to_buffer, buffer_to_display, _, _) = prepared.display;
        if prepared.lines.len() != prepared.hard_ranges.len() {
            return None;
        }

        Some(TableCellDisplayInfo {
            wrapped_lines: prepared.lines,
            hard_line_ranges: prepared.hard_ranges,
            hanging_wraps: prepared.wraps,
            alignment: ti
                .column_alignments
                .get(col)
                .copied()
                .unwrap_or(crate::markdown::TableAlign::Left),
            display_text,
            display_to_buffer,
            buffer_to_display,
            cell_range,
            display_source_end: helpers::table_cell_visible_source_range(
                &cell_content,
                &cell_decoration,
                &cell_active_ranges,
            )
            .end,
            line_height,
        })
    }

    fn table_cell_display_geometry(
        &self,
        window: &mut Window,
        table_start: usize,
        source_line: usize,
        column: usize,
    ) -> Option<(TableCellDisplayInfo, f32)> {
        let visual_row = self.table_visual_row_for_source_line(table_start, source_line)?;
        let bounds = self
            .table_click_areas
            .iter()
            .find(|(_, candidate_table, candidate_row, candidate_column)| {
                *candidate_table == table_start
                    && *candidate_row == visual_row
                    && *candidate_column == column
            })
            .map(|(bounds, _, _, _)| *bounds)?;
        let padding_x = self.scaled(self.styles.table.table_cell_padding_px);
        let inner_width = (f32::from(bounds.size.width) - padding_x * 2.0).max(1.0);
        let info = self.table_cell_display_info(window, source_line, column, inner_width)?;
        Some((info, inner_width))
    }

    pub(crate) fn apply_table_cursor_target(
        &mut self,
        (offset, affinity): (usize, Option<WrapBoundaryAffinityKind>),
    ) -> usize {
        self.wrap_boundary_affinity = affinity.map(|kind| WrapBoundaryAffinity { offset, kind });
        self.table_cursor_paint_bounds = None;
        offset
    }

    pub(crate) fn table_row_bounds_for_offset(
        &self,
        window: &mut Window,
        offset: usize,
        affinity: Option<WrapBoundaryAffinityKind>,
    ) -> Option<Bounds<Pixels>> {
        let (table, line, column) = self.table_cell_for_offset(offset)?;
        let row = self.table_visual_row_for_source_line(table, line)?;
        let bounds = self
            .table_click_areas
            .iter()
            .find_map(|(bounds, t, r, c)| {
                (*t == table && *r == row && *c == column).then_some(*bounds)
            })?;
        let (info, inner_width) = self.table_cell_display_geometry(window, table, line, column)?;
        let cell_offset =
            offset.saturating_sub(self.buffer.line_start(line) + info.cell_range.start);
        let (hard_line, position) = table_cell_position(&info, cell_offset, affinity)?;
        let wrapped = info.wrapped_lines.get(hard_line)?;
        let padding_x = self.scaled(self.styles.table.table_cell_padding_px);
        let padding_y = padding_x * 0.6;
        let content_offset = helpers::table_cell_content_offset(
            info.alignment,
            inner_width,
            f32::from(wrapped.unwrapped_layout.width),
        );
        Some(Bounds::new(
            point(
                bounds.origin.x + px(padding_x + content_offset) + position.x,
                bounds.origin.y + px(padding_y + info.hard_line_top(hard_line)) + position.y,
            ),
            size(
                px(self.base_font_size() * 0.6),
                wrapped.row_height(
                    wrapped.row_for_y(position.y, px(info.line_height)),
                    px(info.line_height),
                ),
            ),
        ))
    }

    pub(crate) fn document_row_bounds_for_offset(
        &self,
        window: &mut Window,
        offset: usize,
        affinity: Option<WrapBoundaryAffinityKind>,
    ) -> Option<(f32, f32)> {
        if self.table_cell_for_offset(offset).is_some() {
            let bounds = self.table_row_bounds_for_offset(window, offset, affinity)?;
            let top = f32::from(bounds.origin.y - self.content_origin.y) + self.scroll_offset;
            return Some((top, top + f32::from(bounds.size.height)));
        }
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        if info.visual_line_count == 0 {
            return None;
        }
        let position = Self::visual_position_for_wrapped_display(
            &info.wrapped_line,
            &info.display_text,
            helpers::buffer_to_display_offset(&info.buffer_to_display, col),
            info.line_height,
            info.indent_px,
            &info.wrap_boundary_upstream_xs,
            affinity,
        )?;
        let row = info
            .wrapped_line
            .row_for_y(position.y, px(info.line_height));
        let top = self.text_y_offset_for_line(line) + f32::from(position.y);
        Some((
            top,
            top + f32::from(info.wrapped_line.row_height(row, px(info.line_height))),
        ))
    }

    pub(crate) fn table_visual_x_for_offset(
        &self,
        window: &mut Window,
        offset: usize,
    ) -> Option<f32> {
        let (table_start, source_line, column) = self.table_cell_for_offset(offset)?;
        let (info, inner_width) =
            self.table_cell_display_geometry(window, table_start, source_line, column)?;
        let line_start = self.buffer.line_start(source_line);
        let cell_offset = offset.saturating_sub(line_start + info.cell_range.start);
        table_cell_visual_position(
            &info,
            cell_offset,
            inner_width,
            self.wrap_boundary_affinity_for_offset(offset),
        )
        .map(|(_, x)| x)
    }

    pub(crate) fn table_vertical_target(
        &self,
        window: &mut Window,
        offset: usize,
        down: bool,
        goal_x: f32,
    ) -> Option<(usize, Option<WrapBoundaryAffinityKind>)> {
        let (table_start, source_line, column) = self.table_cell_for_offset(offset)?;
        let (info, inner_width) =
            self.table_cell_display_geometry(window, table_start, source_line, column)?;
        let line_start = self.buffer.line_start(source_line);
        let cell_offset = offset.saturating_sub(line_start + info.cell_range.start);
        let (visual_line, _) = table_cell_visual_position(
            &info,
            cell_offset,
            inner_width,
            self.wrap_boundary_affinity_for_offset(offset),
        )?;
        let visual_line_count = table_cell_visual_line_count(&info);
        let target_visual_line = if down {
            visual_line
                .checked_add(1)
                .filter(|line| *line < visual_line_count)
        } else {
            visual_line.checked_sub(1)
        };
        if let Some(target_visual_line) = target_visual_line {
            let (target_cell_offset, affinity) =
                table_cell_offset_for_visual_line(&info, target_visual_line, goal_x, inner_width)?;
            return Some((
                line_start + info.cell_range.start + target_cell_offset,
                affinity,
            ));
        }

        let rows = super::input_table::visible_table_line_indices(&self.decorations, table_start)?;
        let row = rows.iter().position(|line| *line == source_line)?;
        let target_row = if down {
            row.checked_add(1).filter(|target| *target < rows.len())
        } else {
            row.checked_sub(1)
        }?;
        let target_line = rows[target_row];
        let target_column = self
            .decorations
            .get(target_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .map(|table_info| column.min(table_info.cell_ranges.len().saturating_sub(1)))?;
        let (target_info, target_inner_width) =
            self.table_cell_display_geometry(window, table_start, target_line, target_column)?;
        let target_visual_line = if down {
            0
        } else {
            table_cell_visual_line_count(&target_info).saturating_sub(1)
        };
        let (target_cell_offset, affinity) = table_cell_offset_for_visual_line(
            &target_info,
            target_visual_line,
            goal_x,
            target_inner_width,
        )?;
        Some((
            self.buffer.line_start(target_line) + target_info.cell_range.start + target_cell_offset,
            affinity,
        ))
    }

    pub(super) fn front_matter_input_viewport_bounds(&self) -> Bounds<Pixels> {
        Bounds::new(
            self.content_origin,
            size(px(self.last_viewport_width), px(self.last_viewport_height)),
        )
    }

    /// Input callbacks can precede prepaint. Shape the small metadata region
    /// synchronously so hits, caret scrolling, and native IME share current rows.
    pub(super) fn refresh_front_matter_input_geometry(
        &mut self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
    ) {
        let columns = self.measure_front_matter_columns(window);
        if self.front_matter_columns != columns {
            self.front_matter_columns = columns;
            self.front_matter_group_geometry_dirty = true;
            self.last_stable_input_bounds = None;
            self.pending_input_bounds = None;
            if let Some(table) = self
                .decorations
                .first()
                .and_then(|d| d.table_info.as_ref())
                .filter(|table| table.is_front_matter)
            {
                let rows = table.table_start_line..table.table_end_line;
                for line in rows.clone() {
                    if let Some(info) = self.line_display_info.get_mut(line) {
                        *info = None;
                    }
                }
                self.clear_sparse_line_geometry_range(rows);
                self.invalidate_line_geometry_cache();
            }
        }
        let width = f32::from(bounds.size.width);
        if width > 0.0
            && (self.last_viewport_width - width).abs() > 0.5
            && let Some(table) = self
                .decorations
                .first()
                .and_then(|d| d.table_info.as_ref())
                .filter(|table| table.is_front_matter)
        {
            let rows = table.table_start_line..table.table_end_line;
            self.last_viewport_width = width;
            self.front_matter_group_geometry_dirty = true;
            self.last_stable_input_bounds = None;
            self.pending_input_bounds = None;
            for line in rows.clone() {
                if let Some(info) = self.line_display_info.get_mut(line) {
                    *info = None;
                }
            }
            self.clear_sparse_line_geometry_range(rows);
            self.invalidate_line_geometry_cache();
        }
        self.sync_front_matter_group_focus_for_width(width);
        if !self.front_matter_group_geometry_dirty || bounds.size.width <= px(0.0) {
            return;
        }
        let Some(table) = self
            .decorations
            .first()
            .and_then(|d| d.table_info.as_ref())
            .filter(|table| table.is_front_matter)
        else {
            return;
        };
        let rows = table.table_start_line..table.table_end_line;
        let plans = self.front_matter_group_plan_for_width(f32::from(bounds.size.width));
        let hidden = |line| {
            self.front_matter_line_is_hidden(line)
                || (self.front_matter_summary_line() != Some(line)
                    && plans.iter().any(|plan| plan.hides_line(line)))
        };
        if rows.clone().all(|line| {
            hidden(line)
                || self
                    .line_display_info
                    .get(line)
                    .and_then(|info| info.as_ref())
                    .is_some_and(|info| info.visual_line_count > 0)
        }) {
            return;
        }
        let mut updates = Vec::new();
        for line in rows.clone() {
            let info = self.inline_line_display_info_with_group_plan(window, line, bounds, &plans);
            let padding = self.block_padding_for_line(line);
            updates.push((
                line,
                info.map(|info| LineDisplayInfo {
                    visual_line_count: info.wrapped_line.wrap_boundaries().len() + 1,
                    wrap_width_px: info.wrapped_line.wrap_width.map(f32::from),
                    unwrapped_width_px: f32::from(info.wrapped_line.unwrapped_layout.width),
                    display_text: info.display_text,
                    display_to_buffer: info.display_to_buffer,
                    buffer_to_display: info.buffer_to_display,
                    base_runs: info.base_runs,
                    font_size: info.font_size,
                    line_height: info.line_height,
                    wrapped_line: info.wrapped_line,
                    wrap_boundary_upstream_xs: info.wrap_boundary_upstream_xs,
                    extra_height: padding.0 + padding.1,
                    indent_px: info.indent_px,
                    content_x: info.content_x,
                    horizontal_clip_right_x: None,
                    selection_padding_left_px: 0.0,
                }),
            ));
        }
        self.apply_line_display_info_updates(self.buffer.line_count(), updates);
        self.invalidate_line_geometry_cache();
        let mut type_areas = Vec::new();
        let mut fold_areas = Vec::new();
        let fold_line = super::fold::front_matter_fold_control_line(
            &self.decorations,
            &plans,
            self.collapsed_front_matter_range().is_some(),
        );
        let viewport = bounds.intersect(&window.content_mask().bounds);
        let slot_size = self.scaled(self.styles.self_drawn.format_trigger_size_px);
        let (column_inset, _) = super::element::front_matter_content_geometry(
            true,
            0.0,
            1.0,
            crate::ui::self_drawn::FrontMatterControl::column_width(
                &self.styles.self_drawn,
                self.scaled(1.0),
            ),
        );
        for line in rows {
            let Some(info) = self
                .line_display_info
                .get(line)
                .and_then(|info| info.as_ref())
            else {
                continue;
            };
            let separator = self.decorations[line]
                .table_info
                .as_ref()
                .is_some_and(|table| table.is_separator);
            let summary = self.front_matter_summary_line() == Some(line);
            let left = info.content_x
                - if separator && !summary {
                    0.0
                } else {
                    column_inset
                };
            let top =
                f32::from(bounds.origin.y) + self.text_y_offset_for_line(line) - self.scroll_offset;
            let property_line = plans
                .iter()
                .find(|plan| plan.label_line() == line)
                .map_or(line, |plan| plan.group.header_line);
            if !summary
                && super::front_matter::property_type(
                    &self.buffer,
                    &self.decorations,
                    property_line,
                )
                .is_some()
                && let Some(hit) = super::element::front_matter_type_hit_bounds(
                    Bounds::new(
                        point(px(left), px(top + (info.line_height - slot_size) * 0.5)),
                        size(px(slot_size), px(slot_size)),
                    ),
                    viewport,
                )
            {
                type_areas.push((hit, property_line));
            }
            if fold_line == Some(line) {
                let icon_size = self.scaled(self.styles.self_drawn.gutter_fold_icon_size_px);
                let hit = super::element::front_matter_fold_hit_bounds(
                    left - icon_size
                        - self.scaled(self.styles.self_drawn.format_trigger_content_gap_px),
                    left,
                    top,
                    info.total_height(),
                    viewport,
                );
                if hit.size.width > px(0.0) && hit.size.height > px(0.0) {
                    fold_areas.push((hit, 0, false));
                }
            }
        }
        self.front_matter_type_areas = type_areas;
        self.gutter_fold_areas = fold_areas;
        let mut hits = Vec::new();
        let mut deletes = Vec::new();
        let mut scroll_areas = Vec::new();
        for plan in plans
            .iter()
            .filter(|plan| !plan.active && !plan.source_visible)
        {
            let line = plan.label_line();
            let Some(info) = self
                .line_display_info
                .get(line)
                .and_then(|info| info.as_ref())
            else {
                continue;
            };
            let Some(mut font) = info.base_runs.first().map(|run| run.font.clone()) else {
                continue;
            };
            font.weight = gpui::FontWeight::NORMAL;
            let mut decoration = self.decorations[line].clone();
            helpers::apply_front_matter_group_display(
                &mut decoration,
                line,
                Some(plan),
                self.buffer.line_content(line),
            );
            let start = decoration
                .front_matter
                .as_ref()
                .and_then(|fm| info.buffer_to_display.get(fm.value_range.start))
                .copied()
                .unwrap_or(0);
            let width = if self.editor_uses_expanded_layout() {
                f32::from(bounds.size.width)
            } else {
                f32::from(bounds.size.width).min(self.max_editor_width())
            };
            let (_, width) = super::element::front_matter_content_geometry(
                true,
                0.0,
                (width - self.padding_left() - self.padding_right()).max(self.scaled(120.0)),
                crate::ui::self_drawn::FrontMatterControl::column_width(
                    &self.styles.self_drawn,
                    self.scaled(1.0),
                ),
            );
            super::element::push_front_matter_group_paint(
                self,
                plan,
                window,
                bounds,
                &info.wrapped_line,
                start,
                info.content_x,
                f32::from(bounds.origin.y) + self.text_y_offset_for_line(line) - self.scroll_offset,
                info.line_height,
                info.font_size,
                width,
                &font,
                &mut Vec::new(),
                &mut hits,
                &mut deletes,
                &mut scroll_areas,
                &mut Vec::new(),
            );
        }
        self.front_matter_text_areas = hits;
        self.front_matter_delete_areas = deletes;
        self.front_matter_list_scroll_areas = scroll_areas;
    }

    pub(super) fn inline_line_display_info(
        &self,
        window: &mut Window,
        line_idx: usize,
        bounds: Bounds<Pixels>,
    ) -> Option<InlineLineDisplayInfo> {
        let group_plans = self.front_matter_group_plan_for_width(f32::from(bounds.size.width));
        self.inline_line_display_info_with_group_plan(window, line_idx, bounds, &group_plans)
    }

    pub(super) fn inline_line_display_info_with_group_plan(
        &self,
        window: &mut Window,
        line_idx: usize,
        bounds: Bounds<Pixels>,
        group_plans: &[helpers::FrontMatterGroupPlan],
    ) -> Option<InlineLineDisplayInfo> {
        let mut decoration = self.decorations.get(line_idx)?.clone();
        let group_plan = group_plans
            .iter()
            .find(|plan| plan.group.source_lines.contains(&line_idx));
        let front_matter_summary = self.front_matter_fold_summary(line_idx);
        if self.front_matter_line_is_hidden(line_idx)
            || (front_matter_summary.is_none()
                && group_plan.is_some_and(|plan| plan.hides_line(line_idx)))
        {
            return None;
        }
        let is_front_matter_line = decoration
            .table_info
            .as_ref()
            .is_some_and(|table_info| table_info.is_front_matter);
        if matches!(decoration.block_type, BlockType::Table) && !is_front_matter_line {
            return None;
        }

        let viewport_width: f32 = bounds.size.width.into();
        let origin_x: f32 = bounds.origin.x.into();
        let expanded_layout = self.editor_uses_expanded_layout();
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
        let (front_matter_content_left, wrap_width) = super::element::front_matter_content_geometry(
            front_matter_summary.is_some()
                || (is_front_matter_line
                    && !decoration
                        .table_info
                        .as_ref()
                        .is_some_and(|table| table.is_separator)),
            editor_left + self.padding_left(),
            text_width,
            crate::ui::self_drawn::FrontMatterControl::column_width(
                &self.styles.self_drawn,
                self.scaled(1.0),
            ),
        );

        let text_style = window.text_style();
        let editor_styles = &self.styles.editor;
        let configured_font_family = |configured: &'static str| {
            if configured == crate::platform::fonts::SYSTEM_UI {
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

        let mut italic_font = text_style.font();
        italic_font.family = configured_font_family(editor_styles.editor_italic_latin_font_family);
        italic_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_italic_latin_font_family);

        let mut italic_cjk_font = text_style.font();
        italic_cjk_font.family = configured_font_family(editor_styles.editor_body_cjk_font_family);
        italic_cjk_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_body_cjk_font_family);

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
                test_lines
                    .first()
                    .map(|line| f32::from(line.width()))
                    .unwrap_or_else(|| self.base_font_size() * 0.6)
            } else {
                self.base_font_size() * 0.6
            }
        };
        let body_space_width = {
            let test_run = TextRun {
                len: 1,
                font: body_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(test_lines) = window.text_system().shape_text(
                " ".into(),
                px(self.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                test_lines
                    .first()
                    .map(|line| f32::from(line.width()).max(1.0))
                    .unwrap_or_else(|| base_char_width.max(1.0))
            } else {
                base_char_width.max(1.0)
            }
        };

        let table_cell_padding_px = self.scaled(self.styles.table.table_cell_padding_px);
        let table_cell_padding_chars = if table_cell_padding_px <= 0.0 {
            0
        } else {
            (table_cell_padding_px / base_char_width).ceil().max(1.0) as usize
        };
        let checkbox_text_gap_px = self.scaled(self.styles.checkbox.checkbox_text_gap_px);
        let task_source_indent_step_chars =
            self.styles.checkbox.checkbox_parent_prefix_chars as usize;
        let collapsed_task_prefix_display_columns = helpers::task_prefix_display_columns(
            body_space_width,
            self.base_font_size() * self.styles.checkbox.checkbox_size_factor,
            checkbox_text_gap_px,
        );
        let checkbox_indent_step_display_columns = collapsed_task_prefix_display_columns;

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
        let heading_content_start =
            matches!(decoration.block_type, BlockType::Heading(_)).then(|| {
                self.line_content_start_col(line_idx)
                    .min(line_content.len())
            });
        let font_size = self.font_size_for_block(&decoration.block_type);
        let base_line_height = self.line_height_for_block(&decoration.block_type);
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
                        crate::markdown::InlineKind::Underline => 2,
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
            if let Some(content_start) = heading_content_start {
                if is_cursor_line {
                    Some(
                        self.cursor
                            .offset
                            .saturating_sub(line_start_offset)
                            .max(content_start),
                    )
                } else {
                    None
                }
            } else {
                Some(line_content.len())
            }
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
        let mut active_ranges: Vec<Range<usize>> =
            if self.is_selecting && self.selection_drag_has_moved {
                Vec::new()
            } else {
                all_sel_ranges
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
                    .collect()
            };
        if self.is_selecting && self.selection_drag_has_moved && self.cursor.has_selection() {
            let (anchor_line, anchor_col) = self.buffer.offset_to_line_col(self.cursor.anchor);
            let (head_line, head_col) = self.buffer.offset_to_line_col(self.cursor.offset);
            if anchor_line == line_idx && head_line == line_idx {
                for syntax_range in &decoration.syntax_ranges {
                    let anchor_inside = anchor_col >= syntax_range.trigger_range.start
                        && anchor_col <= syntax_range.trigger_range.end;
                    let head_inside = head_col >= syntax_range.trigger_range.start
                        && head_col <= syntax_range.trigger_range.end;
                    if anchor_inside && head_inside {
                        active_ranges.push(syntax_range.trigger_range.clone());
                        break;
                    }
                }
            }
        }
        for marked in self.marked_ranges() {
            if heading_content_start.is_some() && ime_active_on_line {
                continue;
            }
            if marked.end <= line_start_offset || marked.start >= line_end_offset {
                continue;
            }
            let mut start = marked
                .start
                .saturating_sub(line_start_offset)
                .min(line_content.len());
            let end = marked
                .end
                .saturating_sub(line_start_offset)
                .min(line_content.len());
            if let Some(content_start) = heading_content_start {
                if end <= content_start {
                    continue;
                }
                start = start.max(content_start);
            }
            active_ranges.push(start..end);
        }
        let syntax_expansion_allowed = self.markdown_syntax_expansion_allowed();
        if !syntax_expansion_allowed {
            active_ranges.clear();
        }
        if syntax_expansion_allowed
            && (!self.is_selecting || !self.selection_drag_has_moved)
            && active_ranges.is_empty()
            && !self.suppress_fold_for_empty_selection_drag
            && let Some(col) = cursor_col
        {
            active_ranges.push(col.min(line_content.len())..col.min(line_content.len()));
        }

        self.append_source_reveal_range(line_idx, &mut active_ranges);
        if group_plan.is_some_and(|plan| plan.active) {
            active_ranges.push(0..line_content.len());
        }
        if self.markdown_syntax_expansion_allowed() {
            helpers::preserve_heading_syntax_focus(
                &decoration,
                line_start_offset..line_end_offset,
                self.all_cursors_snapshot(),
                &mut active_ranges,
            );
        }
        helpers::expand_source_syntax(self.source_mode, line_content.len(), &mut active_ranges);
        helpers::apply_front_matter_group_display(
            &mut decoration,
            line_idx,
            group_plan,
            line_content,
        );

        let front_matter_row_active = is_front_matter_line
            && helpers::front_matter_row_is_active(&active_ranges, line_content.len());
        let code_block_syntax_active = if decoration
            .table_info
            .as_ref()
            .is_some_and(|table| table.is_front_matter && table.is_separator)
        {
            self.front_matter_delimiters_are_visible()
        } else {
            (syntax_expansion_allowed
                && active_code_fence_group
                    .is_some_and(|(start, end)| line_idx >= start && line_idx <= end))
                || self.source_reveals_code_fence_group(line_idx)
                || front_matter_row_active
        };
        let placeholder_unit_width =
            body_space_width * (font_size / self.base_font_size()).max(0.1);
        helpers::add_inline_math_display_replacements(
            &mut decoration,
            line_content,
            &self.image_cache,
            placeholder_unit_width,
        );
        let line_height = base_line_height;
        super::front_matter::apply_date_time_format(
            &mut decoration,
            self.app_settings.date_time_format(),
        );
        let (mut display_text, mut display_to_buffer, buffer_to_display, prefix_collapsed) =
            if let Some(summary) = front_matter_summary.as_deref() {
                helpers::build_front_matter_fold_display(line_content, summary)
            } else {
                helpers::build_display_text(
                    line_content,
                    &decoration,
                    &active_ranges,
                    code_block_syntax_active,
                    None,
                    table_cell_padding_chars,
                    collapsed_task_prefix_display_columns,
                    checkbox_indent_step_display_columns,
                    self.styles.checkbox.checkbox_parent_prefix_chars as usize,
                    0,
                    self.styles.editor.editor_superscript_scale_factor,
                    self.styles.editor.editor_subscript_scale_factor,
                    self.styles.editor.editor_superscript_scale_factor,
                )
            };
        self.apply_list_display_styles(
            line_content,
            &decoration,
            &mut display_text,
            prefix_collapsed,
        );

        let mut runs = helpers::build_text_runs_for_display_with_language(
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
            &italic_font,
            &italic_cjk_font,
            collapsed_task_prefix_display_columns,
            font_size,
            &self.theme,
            self.styles.wiki_link.wiki_link_text_color,
            self.styles.wiki_link.wiki_link_text_weight,
            self.styles.editor.editor_list_marker_text_color,
            self.styles.editor.editor_list_marker_font_weight,
            self.styles.editor.editor_heading_marker_text_color,
            Some(self.styles.table.front_matter_key_text_color),
            self.active_tab_is_non_markdown_text(),
            self.source_highlight_language(),
        );

        if front_matter_summary.is_some() {
            for run in &mut runs {
                run.color = self.theme.syntax_marker;
            }
        }
        let display_shared: SharedString = if display_text.is_empty() {
            " ".into()
        } else {
            display_text.clone().into()
        };

        let is_list_type = matches!(
            decoration.block_type,
            BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
        );
        let prefix_bytes = if front_matter_summary.is_some() {
            0
        } else if is_front_matter_line {
            decoration
                .front_matter
                .as_ref()
                .and_then(|info| buffer_to_display.get(info.value_range.start))
                .copied()
                .unwrap_or(0)
        } else if is_list_type {
            helpers::compute_display_prefix_bytes(
                &display_text,
                &decoration.block_type,
                prefix_collapsed,
                collapsed_task_prefix_display_columns,
            )
        } else {
            0
        };
        let wrap_indent_px = if prefix_bytes > 0 && prefix_bytes <= display_text.len() {
            let prefix_text = &display_text[..prefix_bytes];
            let prefix_runs = helpers::extract_runs_for_range(&runs, 0, prefix_bytes);
            window
                .text_system()
                .shape_text(
                    prefix_text.to_string().into(),
                    px(font_size),
                    &prefix_runs,
                    None,
                    None,
                )
                .ok()
                .and_then(|lines| lines.into_iter().next())
                .map(|line| f32::from(line.width()))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let letter_spacing = editor_styles.editor_letter_spacing_px;
        let base_wrap_width = if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            helpers::code_block_soft_wrap_width(
                (text_width
                    - self.scaled(self.styles.code_block.code_block_text_padding_left_px)
                    - self.scaled(self.styles.code_block.code_block_wrap_right_padding_px))
                .max(self.scaled(80.0)),
                base_char_width,
            )
        } else {
            wrap_width
        };
        let effective_wrap_width = if is_front_matter_line {
            base_wrap_width
        } else if wrap_indent_px > 0.0 {
            (base_wrap_width - wrap_indent_px).max(base_wrap_width * 0.3)
        } else {
            base_wrap_width
        };
        let indicator_width = helpers::folded_heading_indicator_width(
            &decoration.block_type,
            self.active_tab_is_markdown(),
            self.folded_sections.contains(&line_idx),
            self.scaled(self.styles.self_drawn.folded_heading_indicator_size_px),
            self.scaled(self.styles.self_drawn.folded_heading_indicator_gap_px),
        );
        let wrap_width_param = if self.editor_wraps_to_available_width() {
            Some(px((effective_wrap_width - indicator_width).max(1.0)))
        } else {
            None
        };

        let Ok(mut wrapped_lines) = window.text_system().shape_text(
            display_shared,
            px(font_size),
            &runs,
            wrap_width_param,
            None,
        ) else {
            return None;
        };

        if letter_spacing != 0.0 {
            for wrapped in &mut wrapped_lines {
                helpers::apply_letter_spacing(wrapped, letter_spacing);
            }
        }

        let mut wrapped_line = wrapped_lines.into_iter().next()?;
        helpers::keep_inline_math_placeholders_unwrapped(
            &mut wrapped_line,
            &decoration,
            line_content,
            &buffer_to_display,
            &active_ranges,
            &self.image_cache,
            0,
        );
        helpers::apply_inline_math_row_heights(
            &mut wrapped_line,
            &decoration,
            line_content,
            &buffer_to_display,
            &active_ranges,
            &self.image_cache,
            line_height,
            0,
        );
        let metadata_indent = if is_front_matter_line {
            self.front_matter_columns
                .or_else(|| self.measure_front_matter_columns(window))
                .and_then(|columns| {
                    super::front_matter_layout::align_and_wrap(
                        &mut wrapped_line,
                        &decoration,
                        &buffer_to_display,
                        helpers::front_matter_row_is_active(&active_ranges, line_content.len()),
                        columns,
                        wrap_width_param.map(|_| base_wrap_width),
                        font_size * self.styles.checkbox.checkbox_size_factor,
                    )
                })
        } else {
            None
        };
        if metadata_indent.is_some() {
            super::front_matter_layout::align_stacked_padding_hits(
                &wrapped_line,
                &decoration,
                &buffer_to_display,
                &mut display_to_buffer,
            );
        }
        let indent_px = if let Some(indent) = metadata_indent {
            indent
        } else if prefix_bytes > 0 && prefix_bytes <= display_text.len() {
            if is_front_matter_line {
                super::front_matter::wrap_indent(
                    f32::from(wrapped_line.unwrapped_layout.x_for_index(prefix_bytes)),
                    wrap_width_param.map(|_| base_wrap_width),
                )
            } else {
                wrapped_line
                    .position_for_index(prefix_bytes, px(line_height))
                    .map(|position| f32::from(position.x))
                    .unwrap_or(0.0)
            }
        } else {
            0.0
        };
        let base_content_x = if is_front_matter_line {
            front_matter_content_left
        } else if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            editor_left
                + self.padding_left()
                + self.scaled(self.styles.code_block.code_block_text_padding_left_px)
        } else {
            editor_left + self.padding_left()
        };
        let content_x =
            if matches!(decoration.block_type, BlockType::TaskList(_)) && prefix_collapsed {
                let target_prefix_width = helpers::task_prefix_target_width_px(
                    line_content,
                    font_size * self.styles.checkbox.checkbox_size_factor,
                    checkbox_text_gap_px,
                    task_source_indent_step_chars,
                )
                .unwrap_or(indent_px);
                base_content_x + (target_prefix_width - indent_px)
            } else {
                base_content_x
            };
        let wrap_boundary_upstream_xs = if metadata_indent.is_some() {
            super::front_matter_layout::wrap_boundary_upstream_xs(&wrapped_line)
        } else {
            compute_inline_wrap_boundary_upstream_xs(
                window,
                &wrapped_line,
                &display_text,
                &runs,
                font_size,
                line_height,
                letter_spacing,
            )
        };

        Some(InlineLineDisplayInfo {
            display_text,
            display_to_buffer,
            base_runs: runs,
            buffer_to_display,
            font_size,
            line_height,
            wrapped_line,
            wrap_boundary_upstream_xs,
            indent_px,
            content_x,
        })
    }

    fn table_cell_offset_for_position(
        &mut self,
        window: &mut Window,
        table_start: usize,
        visual_row: usize,
        col: usize,
        bounds: &Bounds<Pixels>,
        position: Point<Pixels>,
    ) -> Option<usize> {
        let target = self.table_cell_hit_for_position(
            window,
            table_start,
            visual_row,
            col,
            bounds,
            position,
        )?;
        Some(self.apply_table_cursor_target(target))
    }

    pub(super) fn table_cell_hit_for_position(
        &self,
        window: &mut Window,
        table_start: usize,
        visual_row: usize,
        col: usize,
        bounds: &Bounds<Pixels>,
        position: Point<Pixels>,
    ) -> Option<(usize, Option<WrapBoundaryAffinityKind>)> {
        let source_line = self.table_source_line_for_visual_row(table_start, visual_row)?;
        let line_start = self.buffer.line_start(source_line);
        let padding_x = self.scaled(self.styles.table.table_cell_padding_px);
        let padding_y = self.scaled(self.styles.table.table_cell_padding_px) * 0.6;
        let inner_width = (f32::from(bounds.size.width) - padding_x * 2.0).max(1.0);
        let info = self.table_cell_display_info(window, source_line, col, inner_width)?;
        let line_height = info.line_height;
        let local_y = (f32::from(position.y) - f32::from(bounds.origin.y) - padding_y).max(0.0);
        let (selected_line, hit_y) = table_cell_wrapped_line_for_y(&info, local_y)?;
        let wrapped = info.wrapped_lines.get(selected_line)?;
        let hard_range = info.hard_line_ranges.get(selected_line)?;
        let content_offset = helpers::table_cell_content_offset(
            info.alignment,
            inner_width,
            f32::from(wrapped.unwrapped_layout.width),
        );
        let local_x =
            (f32::from(position.x) - f32::from(bounds.origin.x) - padding_x - content_offset)
                .max(0.0);
        let hit_x = info
            .hanging_wraps
            .get(selected_line)
            .copied()
            .unwrap_or_default()
            .hit_x(wrapped, local_x, px(hit_y), line_height);
        let local_display_index =
            match wrapped.closest_index_for_position(point(hit_x, px(hit_y)), px(line_height)) {
                Ok(index) | Err(index) => index,
            };
        let mut display_index = (hard_range.start + local_display_index.min(hard_range.len()))
            .min(info.display_text.len());
        while display_index > 0 && !info.display_text.is_char_boundary(display_index) {
            display_index -= 1;
        }
        let soft_row = wrapped.row_for_y(px(hit_y), px(line_height));
        let source = self
            .decorations
            .get(source_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .filter(|table| !table.is_front_matter)
            .and_then(|_| {
                self.buffer
                    .line_content(source_line)
                    .get(info.cell_range.clone())
            });
        let buffer_col = if let Some(source) = source {
            table_cell_buffer_column_for_hit(
                &info,
                source,
                hard_range.start,
                soft_row,
                local_x,
                display_index,
            )
        } else {
            info.display_to_buffer
                .get(display_index)
                .copied()
                .unwrap_or(info.display_source_end)
        };
        Some((
            line_start + info.cell_range.start + buffer_col,
            Self::wrap_boundary_affinity_for_row(
                wrapped,
                info.display_text.get(hard_range.clone())?,
                display_index - hard_range.start,
                soft_row,
            ),
        ))
    }

    pub(crate) fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(40.0));
        if !event.modifiers.platform
            && !event.modifiers.control
            && (delta.x != px(0.0) || event.modifiers.shift)
            && let Some((_, header, max_offset)) = self
                .front_matter_list_scroll_areas
                .iter()
                .find(|(bounds, _, _)| bounds.contains(&event.position))
                .copied()
        {
            let dx = if delta.x != px(0.0) {
                f32::from(delta.x)
            } else {
                f32::from(delta.y)
            };
            let offset = self
                .front_matter_list_scroll_offsets
                .entry(header)
                .or_default();
            *offset = (*offset - dx).clamp(0.0, max_offset);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if delta.x != px(0.0) || delta.y != px(0.0) {
            self.hide_line_format_toolbar_for_scroll(cx);
        }
        if self.outline_rail_clicked_active_line.is_some() {
            self.outline_rail_clicked_active_line = None;
        }

        if self.active_tab_is_image() {
            let delta = event.delta.pixel_delta(px(40.0));
            let dx = f32::from(delta.x);
            let dy = f32::from(delta.y);
            let zoom_gesture = event.modifiers.platform || event.modifiers.control;

            if zoom_gesture {
                let Some(factor) = super::preview_overlay::wheel_zoom_factor(dy) else {
                    return;
                };
                self.zoom_active_image_view(factor, Some(event.position));
                cx.notify();
                return;
            }

            if dx == 0.0 && dy == 0.0 {
                return;
            }
            if let Some(image_view) = self.active_image_view_state_mut() {
                image_view.pan_x -= dx;
                image_view.pan_y -= dy;
            }
            self.clamp_active_image_view();
            cx.notify();
            return;
        }

        if self.context_menu.is_some() {
            return;
        }
        if self.editor_uses_horizontal_text_scroll() {
            let delta = event.delta.pixel_delta(px(40.0));
            let delta_x = f32::from(delta.x);
            let delta_y = f32::from(delta.y);
            let horizontal_delta = if delta_x.abs() > 0.01 {
                -delta_x
            } else if event.modifiers.shift {
                -delta_y
            } else {
                0.0
            };
            if horizontal_delta.abs() > 0.01
                && self.scroll_code_block_horizontal(
                    super::NON_MARKDOWN_HORIZONTAL_SCROLL_TARGET,
                    horizontal_delta,
                )
            {
                cx.notify();
                return;
            }
        }
        if !self.line_drag_active()
            && let Some(start_line) = self
                .code_block_hover_areas
                .iter()
                .find(|(bounds, _)| bounds.contains(&event.position))
                .map(|(_, start_line)| *start_line)
                .filter(|start_line| !self.code_block_word_wrap_enabled(*start_line))
        {
            let delta = event.delta.pixel_delta(px(40.0));
            let delta_x = f32::from(delta.x);
            let delta_y = f32::from(delta.y);
            let horizontal_delta = if delta_x.abs() > 0.01 {
                -delta_x
            } else if event.modifiers.shift {
                -delta_y
            } else {
                0.0
            };
            if horizontal_delta.abs() > 0.01
                && self.scroll_code_block_horizontal(start_line, horizontal_delta)
            {
                cx.notify();
                return;
            }
        }
        let delta_y: f32 = event
            .delta
            .pixel_delta(px(
                self.base_font_size() * self.styles.editor.editor_paragraph_line_height_factor
            ))
            .y
            .into();
        let scroll_offset_before_wheel = self.scroll_offset;
        // A queued preview layout must not restore an offset captured before
        // this wheel event (for example while a diagram was still loading).
        self.pending_preview_reveal = None;
        self.fold_scroll_guard = None;
        self.pending_click_anchor = None;
        self.pending_typing_scroll_guard = None;
        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        self.pending_center_cursor_in_view_immediately = false;
        self.scroll_anim_epoch = self.scroll_anim_epoch.wrapping_add(1);
        self.scroll_offset -= delta_y;
        self.clamp_scroll();
        // User scrolling overrides any pending centered-cursor confirmation.
        self.center_cursor_confirm_frames = 0;
        if self.embedded_editor && self.embedded_read_only {
            // A pending streaming-update anchor stores the scroll offset it
            // was captured at; fold the user's wheel delta into it so the
            // post-layout anchor correction doesn't cancel this scroll.
            let applied_delta = self.scroll_offset - scroll_offset_before_wheel;
            if let Some(anchor) = self.pending_click_anchor.as_mut() {
                anchor.scroll_before += applied_delta;
            }
            if delta_y > 0.0 {
                // Scrolling up detaches bottom-following immediately; the
                // 4px re-attach tolerance below would otherwise re-stick on
                // small trackpad deltas and snap the view back to the bottom.
                self.embedded_stick_to_bottom = false;
            } else {
                // Re-attach bottom-following only when the user scrolls back
                // down to the end of the transcript.
                let max_scroll = (self
                    .total_content_height()
                    .max(self.embedded_content_height.get())
                    - self.last_viewport_height)
                    .max(0.0);
                self.embedded_stick_to_bottom = self.scroll_offset >= max_scroll - 4.0;
            }
        }
        self.update_toolbar_visibility_for_scroll(cx);
        self.reveal_editor_scrollbar(cx);
        cx.notify();
    }

    fn append_line_after_final_rendered_block(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.layout_dirty
            || self.source_mode
            || !self.active_tab_is_markdown()
            || self.active_tab_is_read_only()
            || !self.active_tab_can_start_content_edit()
            || self.buffer.text().ends_with('\n')
        {
            return false;
        }
        let last = self.buffer.line_count().saturating_sub(1);
        let Some(decoration) = self.decorations.get(last) else {
            return false;
        };
        if decoration.table_info.is_none() && decoration.block_preview.is_none() {
            return false;
        }
        let y = f32::from(position.y);
        let x = f32::from(position.x);
        let bottom = f32::from(self.content_origin.y) + self.total_content_height()
            - self.padding_bottom()
            - self.scroll_offset;
        if y < bottom
            || y >= f32::from(self.content_origin.y) + self.editor_visible_height()
            || x < self.last_editor_left
            || x >= self.last_editor_left + self.last_editor_width
        {
            return false;
        }
        self.checkpoint();
        let end = self.buffer.len();
        self.queue_non_markdown_line_display_transfer(&(end..end), "\n");
        self.buffer.insert(end, "\n");
        self.cursor.move_to(self.buffer.len(), &self.buffer);
        self.clear_extra_cursors();
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        self.selected_handle = None;
        self.is_selecting = false;
        self.selection_drag_position = None;
        self.reparse();
        self.scroll_to_cursor();
        self.commit_history_transaction();
        self.schedule_auto_save_for_active_tab(cx);
        self.pause_blinking(cx);
        cx.notify();
        true
    }

    pub(crate) fn offset_for_position(&self, position: Point<Pixels>) -> usize {
        if !self.layout_dirty
            && let Some(hit) = self
                .front_matter_text_areas
                .iter()
                .find(|hit| hit.bounds.contains(&position))
        {
            return hit.offset_for_position(position).min(self.buffer.len());
        }
        let pos_y: f32 = position.y.into();
        let pos_x: f32 = position.x.into();
        let origin_y: f32 = self.content_origin.y.into();
        if self.layout_dirty {
            return self.cursor.offset;
        }

        let y = (pos_y - origin_y) + self.scroll_offset;
        let content_bottom = self.total_content_height() - self.padding_bottom();
        if y >= content_bottom {
            let last_line = (0..self.buffer.line_count())
                .rev()
                .find(|&line| self.visual_height_for_line(line) > 0.0)
                .unwrap_or(0);
            return self.buffer.line_end(last_line).min(self.buffer.len());
        }
        let visible_line_geometry = if self.front_matter_group_geometry_dirty {
            None
        } else {
            visible_line_hit(&self.visible_line_geometry, pos_y)
                .filter(|(line, _)| !self.front_matter_presentation_line_is_hidden(*line))
        };
        let line = visible_line_geometry
            .map(|(line, _)| line)
            .unwrap_or_else(|| self.line_at_y(y));
        let line_content = self.buffer.line_content(line);
        if line_content.is_empty() {
            return self.buffer.line_start(line);
        }

        if let Some(Some(info)) = self.line_display_info.get(line) {
            let line_text_inset = self.block_padding_for_line(line).0
                + self.image_preview_extra_height_for_line(line)
                + self.line_diff_preview_height_for_line(line, false);
            let local_y = Self::clamp_wrapped_hit_y(
                &info.wrapped_line,
                local_text_y_for_hit(
                    pos_y,
                    y,
                    visible_line_geometry.map(|(_, top)| top),
                    self.text_y_offset_for_line(line),
                    line_text_inset,
                ),
                info.line_height,
            );
            let visual_line_idx = info
                .wrapped_line
                .row_for_y(px(local_y), px(info.line_height));
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

            // Clicking left of a folded line lands before its hidden syntax, so
            // the caret stays put once the click expands the line.
            if visual_line_idx == 0 && local_x <= 0.0 && info.display_text != line_content {
                let leading_ws =
                    line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
                return self.buffer.line_start(line) + leading_ws;
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
            let buffer_col = display_index_to_buffer_col(
                &info.display_text,
                &info.display_to_buffer,
                display_index,
            );
            return self.buffer.line_start(line) + buffer_col;
        }

        let x = (pos_x - self.last_content_left).max(0.0);
        let approx_char_width = self.base_font_size() * 0.6;
        let col = (x / approx_char_width) as usize;
        self.buffer.line_col_to_offset(line, col)
    }
}

#[cfg(test)]
mod table_cell_pointer_tests {
    use super::{
        TableCellDisplayInfo, helpers, table_cell_buffer_column_for_hit,
        table_cell_footnote_label_for_click,
    };
    use crate::markdown::{self, LineDecoration, TableAlign};
    use std::ops::Range;

    #[test]
    fn context_menu_preserves_forward_and_reversed_cell_text_selections() {
        let buffer =
            crate::buffer::Buffer::from_text("| A | B |\n| --- | --- |\n| selected text | other |");
        let start = buffer.text().find("selected").unwrap();
        let end = start + "selected text".len();
        for reversed in [false, true] {
            let original = crate::cursor::Cursor {
                anchor: if reversed { end } else { start },
                offset: if reversed { start } else { end },
            };
            for hit in [start, start + 3, end] {
                let mut cursor = original;
                super::preserve_context_menu_selection(&mut cursor, hit, &buffer);
                assert_eq!(cursor, original);
            }
            let mut cursor = original;
            let other = buffer.text().find("other").unwrap();
            super::preserve_context_menu_selection(&mut cursor, other, &buffer);
            assert_eq!(cursor.offset, other);
            assert_eq!(cursor.anchor, other);
        }
    }

    fn display_info(source: &str, active_ranges: &[Range<usize>]) -> TableCellDisplayInfo {
        let decoration =
            helpers::table_cell_decoration(&LineDecoration::default(), source, &(0..source.len()));
        let (raw, d2b, b2d, _) = helpers::build_display_text(
            source,
            &decoration,
            active_ranges,
            false,
            None,
            0,
            3,
            3,
            2,
            0,
            1.0,
            1.0,
            1.0,
        );
        let range =
            helpers::table_cell_display_range(&raw, source, &decoration, &b2d, active_ranges);
        let display_text = raw[range.clone()].to_string();
        TableCellDisplayInfo {
            wrapped_lines: Vec::new(),
            hanging_wraps: Vec::new(),
            hard_line_ranges: helpers::hard_line_byte_ranges(&display_text),
            alignment: TableAlign::Left,
            display_to_buffer: d2b[range.clone()].to_vec(),
            buffer_to_display: b2d
                .iter()
                .map(|index| index.saturating_sub(range.start).min(range.len()))
                .collect(),
            cell_range: 0..source.len(),
            display_source_end: helpers::table_cell_visible_source_range(
                source,
                &decoration,
                active_ranges,
            )
            .end,
            display_text,
            line_height: 24.0,
        }
    }

    #[test]
    fn typed_edge_spaces_are_displayed_before_another_character_is_entered() {
        for initial in ["", "中🙂", "**é🙂**", "- [ ] "] {
            for at_start in [false, true] {
                let mut row = format!("| {initial}          |");
                let mut caret = 2 + if at_start { 0 } else { initial.len() };
                for inserted in [" ", " ", "\t"] {
                    row.insert_str(caret, inserted);
                    caret += inserted.len();
                    let text = format!("| Header |\n| --- |\n{row}");
                    let decorations = markdown::parse_decorations(&text);
                    let table = decorations[2].table_info.as_ref().unwrap();
                    let (content, range) =
                        crate::editor::table_cell_render::content_with_range(table, &row, 0);
                    assert!(range.contains(&(caret - 1)));
                    let local = caret - range.start;
                    let focus = helpers::TableCellSyntaxFocus {
                        selections: vec![caret..caret],
                        ..Default::default()
                    };
                    let decoration =
                        helpers::table_cell_decoration(&decorations[2], &content, &range);
                    let active = helpers::table_cell_active_ranges(0, &range, &focus, &decoration);
                    let info = display_info(&content, &active);
                    let display_caret = info.buffer_to_display[local];
                    assert!(
                        display_caret >= inserted.len(),
                        "{row:?}, content={content:?}, active={active:?}, display={:?}, local={local}",
                        info.display_text
                    );
                    assert_eq!(
                        &info.display_text[display_caret - inserted.len()..display_caret],
                        inserted,
                        "{row:?} at {caret}",
                    );
                    assert_eq!(
                        info.display_to_buffer[display_caret - inserted.len()],
                        local - inserted.len(),
                    );
                }
            }
        }
    }

    #[test]
    fn active_cell_reveals_only_padding_up_to_its_caret_or_selection() {
        let source = "value     ";
        for (active, expected) in [
            (vec![], "value"),
            (vec![7..7], "value  "),
            (vec![5..8], "value   "),
        ] {
            let info = display_info(source, &active);
            assert_eq!(info.display_text, expected);
            assert_eq!(info.display_source_end, expected.len());
        }
        let source = "     ";
        assert_eq!(display_info(source, &[2..2]).display_text, "  ");
        assert_eq!(display_info(source, &[]).display_text, "");
    }

    #[test]
    fn repeated_cell_end_hits_insert_before_source_alignment_padding() {
        for (source, initial) in [
            ("|                 |", ""),
            ("| \t |", ""),
            ("| 中🙂          |", "中🙂"),
            ("| **é🙂**          |", "**é🙂**"),
            ("| - [ ]            |", "- [ ] "),
            ("| value          ", "value"),
        ] {
            let mut row = source.to_string();
            let mut expected = initial.to_string();
            for inserted in ["x", " y", "中🙂"] {
                let text = format!("| Header |\n| --- |\n{row}");
                let decorations = markdown::parse_decorations(&text);
                let table = decorations[2].table_info.as_ref().unwrap();
                let (content, range) =
                    crate::editor::table_cell_render::content_with_range(table, &row, 0);
                let info = display_info(&content, &[]);
                let column = table_cell_buffer_column_for_hit(
                    &info,
                    &content,
                    0,
                    0,
                    100.0,
                    info.display_text.len(),
                );
                row.insert_str(range.start + column, inserted);
                expected.push_str(inserted);
                assert!(
                    row.starts_with(&format!("| {expected}")),
                    "{source:?}: {row:?}, expected {expected:?}",
                );
            }
        }
    }

    #[test]
    fn blank_cell_hits_on_either_edge_keep_padding_after_the_caret() {
        let source = "\t    ";
        let info = display_info(source, &[]);
        assert!(info.display_text.is_empty());
        for x in [-1.0, 0.0, 100.0] {
            assert_eq!(
                table_cell_buffer_column_for_hit(&info, source, 0, 0, x, 0),
                0,
            );
        }
    }

    #[test]
    fn left_padding_maps_before_collapsed_opening_markers_not_first_visible_glyph() {
        for source in ["**é🙂**", "`é🙂`", "~~é🙂~~", "[é🙂](https://example.com)"] {
            let info = display_info(source, &[]);
            let first_visible = info.display_to_buffer[0];
            assert!(first_visible > 0, "{source:?}");
            assert_eq!(
                table_cell_buffer_column_for_hit(&info, source, 0, 0, 0.0, 0),
                0,
                "{source:?}",
            );
            assert_eq!(
                table_cell_buffer_column_for_hit(&info, source, 0, 0, 0.1, 0),
                first_visible,
                "{source:?}",
            );
        }
    }

    #[test]
    fn left_padding_skips_only_real_leading_source_whitespace() {
        let source = "  **é🙂**  ";
        let info = display_info(source, &[]);
        assert_eq!(
            table_cell_buffer_column_for_hit(&info, source, 0, 0, 0.0, 0),
            2,
        );
        assert_eq!(
            table_cell_buffer_column_for_hit(&info, source, 0, 0, 1.0, 0),
            4,
        );
    }

    #[test]
    fn soft_wrap_left_edge_keeps_its_own_source_position() {
        let source = "**alpha beta**";
        let info = display_info(source, &[]);
        assert_eq!(info.display_text, "alpha beta");
        assert_eq!(
            table_cell_buffer_column_for_hit(&info, source, 0, 1, 0.0, 6),
            source.find("beta").unwrap(),
        );
    }

    #[test]
    fn later_hard_line_left_padding_maps_after_accepted_break_before_opening_marker() {
        for (source, marker) in [
            ("first<br>**é🙂**", "**"),
            ("first<BR/>[é🙂](https://example.com)", "["),
            ("first<Br   />`é🙂`", "`"),
        ] {
            let info = display_info(source, &[]);
            let start = info.hard_line_ranges[1].start;
            assert_eq!(
                table_cell_buffer_column_for_hit(&info, source, start, 0, 0.0, start),
                source.find(marker).unwrap(),
                "{source:?}",
            );
            assert_eq!(
                table_cell_buffer_column_for_hit(&info, source, start, 0, 0.1, start),
                info.display_to_buffer[start],
                "{source:?}",
            );
        }
    }

    #[test]
    fn expanded_source_keeps_literal_marker_hit_positions() {
        let source = "**é🙂**";
        let info = display_info(source, &[0..source.len()]);
        assert_eq!(info.display_text, source);
        for (index, _) in source.char_indices() {
            assert_eq!(
                table_cell_buffer_column_for_hit(&info, source, 0, 0, 1.0, index),
                index,
            );
        }
        assert_eq!(
            table_cell_buffer_column_for_hit(&info, source, 0, 0, 0.0, 0),
            0,
        );
    }

    #[test]
    fn footnote_click_target_matches_ordinary_inclusive_source_range_in_cells() {
        let source = "See [^note].\n\n| Other | Reference |\n| --- | --- |\n| 隣🙂 | See [^note]. |\n\n[^note]: Footnote";
        let decorations = markdown::parse_decorations(source);
        for line in [0, 4] {
            let decoration = &decorations[line];
            let reference = &decoration.footnote_references[0];
            for column in reference.full_range.start..=reference.full_range.end {
                assert_eq!(
                    table_cell_footnote_label_for_click(decoration, column),
                    Some("note"),
                );
            }
            for column in [reference.full_range.start - 1, reference.full_range.end + 1] {
                assert_eq!(
                    table_cell_footnote_label_for_click(decoration, column),
                    None
                );
            }
        }
    }

    #[test]
    fn mapped_collapsed_footnote_click_resolves_label_without_stealing_link_targets() {
        let source = "| Reference |\n| --- |\n| [^note] [web](https://example.com) [[page.md]] |\n\n[^note]: Footnote";
        let decorations = markdown::parse_decorations(source);
        let row = &decorations[2];
        let table = row.table_info.as_ref().unwrap();
        let content = &table.cells[0];
        let info = display_info(content, &[]);
        for display_index in 0..info.display_text.len() {
            let column = table.cell_ranges[0].start + info.display_to_buffer[display_index];
            if row.footnote_references[0].full_range.contains(&column) {
                assert_eq!(
                    table_cell_footnote_label_for_click(row, column),
                    Some("note")
                );
            }
        }
        for target in ["https://example.com", "page.md"] {
            let column = source.lines().nth(2).unwrap().find(target).unwrap();
            assert_eq!(table_cell_footnote_label_for_click(row, column), None);
            assert_eq!(markdown::link_target_at_column(row, column), Some(target));
        }
    }

    #[test]
    fn inline_code_footnote_text_is_not_a_navigation_target() {
        let source = "| Reference |\n| --- |\n| `[^note]` |\n\n[^note]: Footnote";
        let decorations = markdown::parse_decorations(source);
        let row = &decorations[2];
        for column in 0..source.lines().nth(2).unwrap().len() {
            assert_eq!(table_cell_footnote_label_for_click(row, column), None);
        }
    }
}

fn preserve_context_menu_selection(
    cursor: &mut crate::cursor::Cursor,
    offset: usize,
    buffer: &crate::buffer::Buffer,
) {
    let selection = cursor.selection_range();
    if !cursor.has_selection() || offset < selection.start || offset > selection.end {
        cursor.move_to(offset, buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::super::input_wrap::test_wrapped_line;
    use super::{
        EditorView, TableCellDisplayInfo, WrapBoundaryAffinityKind, display_index_to_buffer_col,
        local_text_y_for_hit, nearest_table_cell_in_row, table_cell_offset_for_visual_line,
        table_cell_position, table_cell_visual_position, table_cell_wrapped_line_for_y,
        visible_line_hit,
    };

    #[test]
    fn double_click_word_selection_handles_unicode_words() {
        for text in ["después", "español", "despue\u{301}s", "l'amour"] {
            assert_eq!(
                EditorView::word_selection_range(text, 2),
                0..text.len(),
                "{text}"
            );
        }
        assert_eq!(EditorView::word_selection_range("¿español?!", 3), 2..10);
    }

    #[test]
    fn front_matter_scrolled_hits_use_unclipped_origin_and_exact_scalar_byte_map() {
        use super::super::helpers::front_matter_item_source_map;
        use super::FrontMatterTextHit;
        use gpui::{Bounds, WrappedLineLayout, size};
        use std::sync::Arc;

        let mut shaped = test_wrapped_line(&[24.0]);
        let layout: &mut Arc<WrappedLineLayout> = &mut *shaped;
        Arc::make_mut(layout).wrap_boundaries.clear();
        Arc::make_mut(layout).wrap_width = None;
        let token = "\"a\\u0062cdef\"";
        let hit = FrontMatterTextHit {
            bounds: Bounds::new(point(px(100.0), px(50.0)), size(px(30.0), px(24.0))),
            origin: point(px(70.0), px(50.0)),
            shaped,
            line_height: 24.0,
            source_map: front_matter_item_source_map(token, "abcdef", 200),
        };
        assert_eq!(
            hit.offset_for_position(point(px(100.0), px(60.0))),
            200 + token.find('d').unwrap()
        );
        assert_eq!(
            hit.offset_for_position(point(px(110.0), px(60.0))),
            200 + token.find('e').unwrap()
        );
        assert_eq!(
            hit.offset_for_position(point(px(130.0), px(60.0))),
            200 + token.len()
        );
        assert!(!hit.bounds.contains(&point(px(99.0), px(60.0))));
        assert!(!hit.bounds.contains(&point(px(110.0), px(80.0))));
    }

    #[cfg(all(test, feature = "gpui-test-support"))]
    mod preview_double_click_tests {
        use super::*;
        use crate::editor::{Buffer, Cursor, FileKind, FormatMenuAction, helpers, line_diff};
        use crate::{
            AddCursorBelow, Copy, Cut, Delete, MoveToDocumentStart, SelectAll,
            SelectAllOccurrences, SelectDown, SelectNextOccurrence, SelectRight, SelectWordRight,
            Undo,
        };
        use gpui::{
            AppContext as _, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
            Render, Subscription, Window, div, prelude::*, size,
        };
        use std::{path::PathBuf, sync::Arc};

        #[gpui::test]
        fn math_overlay_opens_at_double_zoom_without_changing_other_previews(
            cx: &mut gpui::TestAppContext,
        ) {
            let source = "$$\nx^2 + y^2\n$$\n\n```mermaid\nflowchart LR\n A --> B\n```\n";
            let editor = cx.update(|cx| {
                cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(source), false))
            });
            editor.update(cx, |editor, _| {
                for (line, expected) in [(0, 2.0), (4, 1.0)] {
                    let key = editor.decorations[line]
                        .block_preview
                        .as_ref()
                        .unwrap()
                        .cache_key
                        .clone();
                    editor.open_preview_overlay(key, Some(line));
                    assert_eq!(
                        editor.preview_overlay.as_ref().unwrap().view.zoom_factor,
                        expected
                    );
                    editor.close_preview_overlay();
                }
                editor.open_preview_overlay("image".into(), None);
                assert_eq!(
                    editor.preview_overlay.as_ref().unwrap().view.zoom_factor,
                    1.0
                );
            });
        }

        struct PaintedEditor {
            editor: Entity<EditorView>,
            caret: crate::editor::caret::SharedCaretGeometry,
            _observer: Subscription,
        }

        impl Render for PaintedEditor {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let pane_id = self.editor.read(cx).current_pane_id();
                div()
                    .id("preview-click-test")
                    .size_full()
                    .key_context("MarkdownEditor")
                    .track_focus(&self.editor.read(cx).focus_handle)
                    .on_action(cx.listener(|this, action: &SelectDown, window, cx| {
                        this.editor
                            .update(cx, |editor, cx| editor.on_select_down(action, window, cx));
                    }))
                    .on_action(cx.listener(|this, action: &SelectRight, window, cx| {
                        this.editor
                            .update(cx, |editor, cx| editor.on_select_right(action, window, cx));
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.editor
                                .update(cx, |editor, cx| editor.on_mouse_down(event, window, cx));
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, window, cx| {
                            this.editor
                                .update(cx, |editor, cx| editor.on_mouse_up(event, window, cx));
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                        this.editor
                            .update(cx, |editor, cx| editor.on_mouse_move(event, window, cx));
                    }))
                    .child(crate::editor::element::EditorElement {
                        editor: self.editor.clone(),
                        pane_id,
                        caret_geometry: self.caret.clone(),
                    })
            }
        }

        fn mouse_down(
            cx: &mut gpui::VisualTestContext,
            editor: &Entity<EditorView>,
            event: MouseDownEvent,
        ) {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| editor.on_mouse_down(&event, window, cx))
            });
        }

        fn mouse_up(
            cx: &mut gpui::VisualTestContext,
            editor: &Entity<EditorView>,
            event: MouseUpEvent,
        ) {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| editor.on_mouse_up(&event, window, cx))
            });
        }

        #[gpui::test]
        fn keyboard_shift_arrows_can_leave_table_edges_without_losing_the_anchor(
            cx: &mut gpui::TestAppContext,
        ) {
            let source = "before\n\n| A | B |\n| --- | --- |\n| one | two |\n\nafter";
            let editor = cx.update(|cx| {
                cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(source), false))
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            cx.run_until_parked();
            for (forward, horizontal) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        let line = if forward { 4 } else { 2 };
                        let table = editor.decorations[line].table_info.as_ref().unwrap();
                        let ranges = crate::editor::input_table::rendered_cell_ranges(
                            table,
                            editor.buffer.line_content(line),
                        );
                        let range = if forward {
                            ranges.last().unwrap()
                        } else {
                            &ranges[0]
                        };
                        let offset = editor.buffer.line_start(line)
                            + if forward { range.end } else { range.start };
                        editor.cursor = Cursor { anchor: 0, offset };
                        editor.reset_cursor_visual_targeting();
                        let expected = editor.table_selection_exit_offset(forward).unwrap();
                        match (forward, horizontal) {
                            (false, false) => editor.on_select_up(&crate::SelectUp, window, cx),
                            (true, false) => editor.on_select_down(&SelectDown, window, cx),
                            (false, true) => editor.on_select_left(&crate::SelectLeft, window, cx),
                            (true, true) => editor.on_select_right(&SelectRight, window, cx),
                        }
                        assert_eq!(
                            editor.cursor.offset, expected,
                            "forward={forward} horizontal={horizontal}"
                        );
                        assert_eq!(editor.cursor.anchor, 0);
                    })
                });
                cx.run_until_parked();
            }
        }

        #[gpui::test]
        fn keyboard_vertical_navigation_exits_metadata_and_folded_blocks(
            cx: &mut gpui::TestAppContext,
        ) {
            let source = "---\ntitle: long title\ntags:\n  - first\n  - second\nbody: |-\n  first line\n  second line\n---\n\n# First\n\n## Folded\nhidden\nhidden too\n\n# Next\n\n> [!NOTE]- Folded callout\n> hidden body\n\nparagraph\nanother paragraph\n\nlast";
            for selecting in [false, true] {
                for goal_x in [0.0, 100.0, 10000.0] {
                    let editor = cx.update(|cx| {
                        cx.new(|cx| {
                            let mut editor =
                                EditorView::new_with_mode(cx, true, false, Some(source), false);
                            editor.styles.ui.spec.animations_enabled = false;
                            editor.vertical_move_goal_x = Some(goal_x);
                            editor.cursor = Cursor::new();
                            let line = editor
                                .buffer
                                .offset_to_line_col(source.find("## Folded").unwrap())
                                .0;
                            editor.toggle_fold(line, cx);
                            editor
                        })
                    });
                    let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                        editor: editor.clone(),
                        caret: Default::default(),
                        _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                    });
                    cx.run_until_parked();
                    for _ in 0..100 {
                        let done = cx.update(|window, cx| editor.update(cx, |editor, cx| {
                            let before = editor.cursor.offset;
                            if before == source.len() { return true; }
                            if selecting {
                                editor.on_select_down(&SelectDown, window, cx);
                            } else {
                                editor.on_down(&crate::Down, window, cx);
                            }
                            assert!(editor.cursor.offset > before,
                                "stuck moving down: selecting={selecting} goal={goal_x} line={} offset={before}",
                                editor.cursor_line());
                            false
                        }));
                        cx.run_until_parked();
                        if done {
                            break;
                        }
                    }
                    cx.update(|_, cx| assert_eq!(editor.read(cx).cursor.offset, source.len()));
                }
            }
        }

        #[gpui::test]
        fn keyboard_vertical_navigation_traverses_markdown_and_wrapped_rows(
            cx: &mut gpui::TestAppContext,
        ) {
            let source = format!(
                "# Heading\n\n{}\n\n- item {}\n  - nested {}\n- [ ] task {}\n\n> quote {}\n\n```rust\n{}\n```\n\n---\n\n| A | B |\n| --- | --- |\n| one | two |\n\n$$\nx^2\n$$\n\n[link](https://moeka.app)\n\n![image](missing.png)\n\nlast",
                "paragraph **bold** *emphasis* `code` 中文 $x^2$ ~~gone~~ [link](https://moeka.app) ".repeat(24),
                "wrapped list text ".repeat(24),
                "nested text ".repeat(24),
                "task text ".repeat(24),
                "quoted text ".repeat(24),
                "let value = 42; ".repeat(24),
            );
            for mode in 0..3 {
                for selecting in [false, true] {
                    for goal_x in [0.0, 100.0, 10000.0] {
                        let editor = cx.update(|cx| {
                            cx.new(|cx| {
                                let mut editor = EditorView::new_with_mode(
                                    cx,
                                    true,
                                    false,
                                    Some(&source),
                                    false,
                                );
                                editor.source_mode = mode == 1;
                                editor.vertical_move_goal_x = Some(goal_x);
                                editor.cursor = Cursor::new();
                                if mode == 2 {
                                    let kind = FileKind::Code {
                                        language: "Rust".into(),
                                    };
                                    let index = editor.active_tab_index().unwrap();
                                    editor.tabs[index].file_kind = Some(kind.clone());
                                    editor.active_file_kind_cache = Some(kind);
                                }
                                editor
                            })
                        });
                        let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                            editor: editor.clone(),
                            caret: Default::default(),
                            _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                        });
                        cx.run_until_parked();
                        for downward in [true, false] {
                            for _ in 0..600 {
                                let done = cx.update(|window, cx| {
                                editor.update(cx, |editor, cx| {
                                    let first = if mode == 0 && !selecting {
                                        editor.line_content_start_col(0)
                                    } else {
                                        0
                                    };
                                    if (downward && editor.cursor.offset == editor.buffer.len())
                                        || (!downward && editor.cursor.offset == first)
                                    {
                                        return true;
                                    }
                                    let before = (
                                        editor.cursor.offset,
                                        editor.wrap_boundary_affinity_for_offset(editor.cursor.offset),
                                    );
                                    match (downward, selecting) {
                                        (true, false) => editor.on_down(&crate::Down, window, cx),
                                        (false, false) => editor.on_up(&crate::Up, window, cx),
                                        (true, true) => editor.on_select_down(&SelectDown, window, cx),
                                        (false, true) => editor.on_select_up(&crate::SelectUp, window, cx),
                                    }
                                    assert_ne!(
                                        before,
                                        (editor.cursor.offset, editor.wrap_boundary_affinity_for_offset(editor.cursor.offset)),
                                        "stuck: mode={mode} selecting={selecting} downward={downward} goal={goal_x} line={} row={:?}",
                                        editor.cursor_line(), editor.cursor_visual_position(editor.cursor.offset),
                                    );
                                    false
                                })
                            });
                                cx.run_until_parked();
                                if done {
                                    break;
                                }
                            }
                            cx.update(|_, cx| {
                                let editor = editor.read(cx);
                                let first = if mode == 0 && !selecting {
                                    editor.line_content_start_col(0)
                                } else {
                                    0
                                };
                                assert_eq!(
                                    editor.cursor.offset,
                                    if downward { source.len() } else { first }
                                );
                            });
                        }
                    }
                }
            }
        }

        fn image() -> Arc<gpui::RenderImage> {
            Arc::new(gpui::RenderImage::new([image::Frame::new(
                image::RgbaImage::new(8, 4),
            )]))
        }

        fn check_painted_double_click(
            cx: &mut gpui::TestAppContext,
            source: &'static str,
            generated: bool,
        ) {
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(source), false);
                    let key = editor
                        .decorations
                        .iter()
                        .find_map(|decoration| {
                            if generated {
                                decoration
                                    .block_preview
                                    .as_ref()
                                    .filter(|preview| preview.is_anchor)
                                    .map(|preview| preview.cache_key.clone())
                            } else {
                                decoration.image_path.clone()
                            }
                        })
                        .expect("preview metadata");
                    editor.image_cache.insert(key, Some(image()));
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            let position = cx.update(|_, cx| {
                let editor = editor.read(cx);
                let areas = if generated {
                    &editor.generated_preview_image_areas
                } else {
                    &editor.image_click_areas
                };
                let (bounds, _) = areas.first().expect("painted preview");
                if !generated && bounds.size.width >= px(60.0) {
                    let (zoom_bounds, _) = editor
                        .image_preview_zoom_areas
                        .first()
                        .expect("image fullscreen button");
                    assert_eq!(
                        zoom_bounds.top() - bounds.top(),
                        bounds.right() - zoom_bounds.right()
                    );
                }
                bounds.center()
            });
            cx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position,
                click_count: 1,
                ..Default::default()
            });
            cx.simulate_event(MouseUpEvent {
                button: MouseButton::Left,
                position,
                click_count: 1,
                ..Default::default()
            });
            assert!(cx.update(|_, cx| editor.read(cx).preview_overlay.is_none()));
            cx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position,
                click_count: 2,
                ..Default::default()
            });
            cx.simulate_event(MouseUpEvent {
                button: MouseButton::Left,
                position,
                click_count: 2,
                ..Default::default()
            });
            assert!(cx.update(|_, cx| editor.read(cx).preview_overlay.is_some()));
        }

        #[gpui::test]
        fn line_diff_toggles_preserve_scroll_and_measured_document(cx: &mut gpui::TestAppContext) {
            let source = format!(
                "{}changed first\n\n{}changed second\n\n{}",
                "before paragraph that wraps across the viewport ".repeat(120),
                "middle\n\n".repeat(100),
                "after\n\n".repeat(100),
            );
            let saved = source
                .replace("changed first", "saved first")
                .replace("changed second", "saved second");
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(&source), false);
                    editor.embedded_editor = false;
                    editor.styles.ui.spec.animations_enabled = false;
                    let index = editor.active_tab_index().unwrap();
                    editor.tabs[index].history.saved_text = Some(saved);
                    editor.line_diff_dirty = true;
                    editor.refresh_line_diff_if_needed();
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            cx.run_until_parked();

            for (at_bottom, animated) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.styles.ui.spec.animations_enabled = animated;
                        editor.fold_scroll_guard = None;
                        editor.scroll_offset = if at_bottom {
                            editor.max_scroll_offset()
                        } else {
                            editor.total_content_height() * 0.4
                        };
                        cx.notify();
                    });
                });
                cx.run_until_parked();
                for hunk_index in [0, 1, 1, 0, 0] {
                    let (scroll_before, cursor_before) = cx.update(|_, cx| {
                        editor.update(cx, |editor, cx| {
                            let before = (editor.scroll_offset, editor.cursor);
                            let cache_len = editor.line_display_info.len();
                            let measured = editor
                                .line_display_info
                                .iter()
                                .filter(|info| info.is_some())
                                .count();
                            assert!(measured > 0);
                            assert_eq!(editor.line_diff.hunks.len(), 2);
                            assert!(!editor.active_tab_is_read_only());
                            editor.pending_scroll_to_cursor = true;
                            editor.pending_center_cursor_in_view = true;
                            editor.toggle_line_diff_hunk(hunk_index, cx);
                            assert_eq!(editor.line_diff_reveal_started.is_some(), animated,);
                            if animated
                                && editor.line_diff_reveal_target == 1.0
                                && editor.line_diff_reveal_from == 0.0
                            {
                                let line = editor.line_diff.hunks[hunk_index].current_lines.start;
                                assert_eq!(
                                    editor.line_diff_preview_height_for_line(line, false),
                                    0.0
                                );
                                editor.line_diff_reveal_progress = 0.5;
                                let half_height =
                                    editor.line_diff_preview_height_for_line(line, false);
                                editor.line_diff_reveal_progress = 1.0;
                                let full_height =
                                    editor.line_diff_preview_height_for_line(line, false);
                                assert!(half_height > 0.0);
                                assert_eq!(half_height * 2.0, full_height);
                                editor.line_diff_reveal_progress = 0.0;
                            }
                            assert_eq!(editor.fold_scroll_guard, Some(before.0));
                            assert_eq!(editor.line_display_info.len(), cache_len);
                            assert_eq!(
                                editor
                                    .line_display_info
                                    .iter()
                                    .filter(|info| info.is_some())
                                    .count(),
                                measured
                            );
                            before
                        })
                    });
                    cx.run_until_parked();
                    cx.update(|_, cx| {
                        let editor = editor.read(cx);
                        assert_eq!(editor.scroll_offset, scroll_before);
                        assert_eq!(editor.cursor, cursor_before);
                        assert_eq!(editor.fold_scroll_guard, Some(scroll_before));
                        assert!(!editor.pending_scroll_to_cursor);
                        assert!(!editor.pending_center_cursor_in_view);
                    });
                }
            }
        }

        #[gpui::test]
        fn line_diff_closing_reverses_from_current_height_and_added_rows_stay_closed(
            cx: &mut gpui::TestAppContext,
        ) {
            cx.update(|cx| {
                let editor = cx.new(|cx| {
                    let mut editor = EditorView::new_with_mode(
                        cx,
                        true,
                        false,
                        Some("changed line\nextra"),
                        false,
                    );
                    editor.embedded_editor = false;
                    let index = editor.active_tab_index().unwrap();
                    editor.tabs[index].history.saved_text = Some("saved line".into());
                    editor.refresh_line_diff();
                    editor
                });
                editor.update(cx, |editor, cx| {
                    let modified = editor
                        .line_diff
                        .hunks
                        .iter()
                        .position(|hunk| hunk.kind == line_diff::LineDiffKind::Modified)
                        .unwrap();
                    let added = editor
                        .line_diff
                        .hunks
                        .iter()
                        .position(|hunk| hunk.kind == line_diff::LineDiffKind::Added)
                        .unwrap();
                    editor.styles.ui.spec.animations_enabled = true;
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_progress, 0.0);
                    let duration = editor.styles.line_diff.reveal_duration;
                    let started = editor.line_diff_reveal_started.unwrap();
                    assert!(editor.update_line_diff_reveal(started + duration / 2));
                    let partial = editor.line_diff_reveal_progress;
                    assert!(partial > 0.0 && partial < 1.0);
                    // Set an actual mid-animation timestamp before reversing.
                    editor.line_diff_reveal_started =
                        Some(std::time::Instant::now() - duration / 2);
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_target, 0.0);
                    assert!(editor.line_diff.hunks[modified].expanded);
                    let closing_height = editor.line_diff_reveal_progress;
                    let closing_started = editor.line_diff_reveal_started.unwrap();
                    assert!(editor.update_line_diff_reveal(closing_started + duration / 2));
                    assert!(editor.line_diff_reveal_progress < closing_height);
                    editor.line_diff_reveal_started =
                        Some(std::time::Instant::now() - duration / 2);
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_target, 1.0);
                    assert!(editor.line_diff_reveal_progress > 0.0);
                    let started = editor.line_diff_reveal_started.unwrap();
                    assert!(!editor.update_line_diff_reveal(started + duration));
                    assert_eq!(editor.line_diff_reveal_progress, 1.0);
                    editor.toggle_line_diff_hunk(added, cx);
                    assert!(!editor.line_diff.hunks[added].expanded);
                    assert!(editor.line_diff.hunks[modified].expanded);
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_progress, 1.0);
                    let started = editor.line_diff_reveal_started.unwrap();
                    assert!(!editor.update_line_diff_reveal(started + duration));
                    assert_eq!(editor.line_diff_reveal_progress, 0.0);
                    assert!(editor.line_diff.hunks.iter().all(|hunk| !hunk.expanded));
                    editor.styles.ui.spec.animations_enabled = false;
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_progress, 1.0);
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_progress, 0.0);
                    assert!(!editor.line_diff.hunks[modified].expanded);
                    editor.styles.ui.spec.animations_enabled = true;
                    editor.styles.line_diff.reveal_duration = std::time::Duration::ZERO;
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_progress, 1.0);
                    assert!(editor.line_diff_reveal_started.is_none());
                    editor.toggle_line_diff_hunk(modified, cx);
                    assert_eq!(editor.line_diff_reveal_progress, 0.0);
                    assert!(!editor.line_diff.hunks[modified].expanded);
                });
            });
        }

        #[gpui::test]
        fn line_diff_deleted_group_renders_all_rows_and_supports_cross_row_selection_and_restore(
            cx: &mut gpui::TestAppContext,
        ) {
            let old = format!("first\n{}\n\nlast", "wide 字 words ".repeat(90));
            let saved = format!("before\n{old}\nafter");
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some("before\nafter"), false);
                    editor.embedded_editor = false;
                    editor.app_settings.auto_save = false;
                    editor.styles.ui.spec.animations_enabled = false;
                    let index = editor.active_tab_index().unwrap();
                    editor.tabs[index].history.saved_text = Some(saved.clone());
                    editor.refresh_line_diff();
                    assert_eq!(editor.line_diff.hunks.len(), 1);
                    editor.toggle_line_diff_hunk(0, cx);
                    editor
                })
            });
            let caret = std::rc::Rc::new(std::cell::RefCell::new(
                super::super::caret::CaretGeometrySnapshot::default(),
            ));
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret,
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    let layouts = editor.line_diff_selection_layouts.clone();
                    assert_eq!(layouts.len(), 4);
                    assert_eq!(layouts[0].source_text.as_ref(), old);
                    let rows: usize = layouts
                        .iter()
                        .map(|layout| layout.wrapped.wrap_boundaries().len() + 1)
                        .sum();
                    assert_eq!(editor.line_diff_preview_row_count(1, false), rows);
                    assert!(rows > 4);
                    assert!(
                        layouts
                            .windows(2)
                            .all(|pair| pair[0].clip_bottom() <= pair[1].clip_top() + px(0.01))
                    );
                    editor.line_diff_selection = Some(line_diff::LineDiffSelection {
                        hunk_index: 0,
                        anchor: 0,
                        head: 0,
                        ..Default::default()
                    });
                    editor.move_line_diff_selection_vertical(true, true, cx);
                    assert_eq!(
                        editor.line_diff_selection.unwrap().head,
                        layouts[1].source_range.start
                    );
                    editor.move_line_diff_selection_line_edge(false, true, cx);
                    assert_eq!(
                        editor.line_diff_selection.unwrap().head,
                        layouts[1].source_range.start
                    );
                    let target =
                        layouts[3].origin + point(px(0.0), px(layouts[3].line_height * 0.5));
                    let (offset, _) = editor.line_diff_selection_offset_at(target, 0).unwrap();
                    assert_eq!(offset, layouts[3].source_range.start);
                    editor.on_select_all(&SelectAll, window, cx);
                    editor.on_copy(&Copy, window, cx);
                    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), old);
                    editor.restore_line_diff_hunk(0, cx);
                    assert_eq!(editor.buffer.text(), saved);
                })
            });
        }

        #[gpui::test]
        fn line_diff_old_caret_tracks_soft_rows_and_publishes_blinking_geometry(
            cx: &mut gpui::TestAppContext,
        ) {
            let saved = format!("old words {} end", "wide 界 e\u{301} 👩‍💻 text ".repeat(120));
            let source = format!("{}\nunchanged tail", saved.replacen("old", "new", 1));
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(&source), false);
                    editor.embedded_editor = false;
                    editor.styles.ui.spec.animations_enabled = false;
                    let index = editor.active_tab_index().unwrap();
                    editor.tabs[index].history.saved_text =
                        Some(format!("{saved}\nunchanged tail"));
                    editor.refresh_line_diff();
                    editor.toggle_line_diff_hunk(0, cx);
                    editor
                })
            });
            let caret = std::rc::Rc::new(std::cell::RefCell::new(
                super::super::caret::CaretGeometrySnapshot::default(),
            ));
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: caret.clone(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    let layout = editor.line_diff_selection_layouts[0].clone();
                    let boundaries =
                        helpers::wrap_boundary_byte_indices(&layout.wrapped, &layout.display_text);
                    assert!(boundaries.len() >= 3);
                    let second_row = layout.wrapped.row_top(1, px(layout.line_height));
                    let (head, affinity) = layout.selection_at(
                        layout.origin + point(px(0.0), second_row + px(layout.line_height * 0.5)),
                    );
                    assert_eq!(head, layout.source_offset_for_index(boundaries[0]));
                    assert_eq!(affinity, Some(WrapBoundaryAffinityKind::Downstream));
                    editor.line_diff_selection = Some(line_diff::LineDiffSelection {
                        hunk_index: 0,
                        anchor: head,
                        head,
                        affinity,
                        ..Default::default()
                    });
                    let original_cursor = editor.cursor;
                    let original_text = editor.buffer.text().to_string();
                    editor.move_line_diff_selection_line_edge(false, false, cx);
                    assert_eq!(editor.line_diff_selection.unwrap().head, head);
                    editor.move_line_diff_selection_vertical(true, true, cx);
                    let selection = editor.line_diff_selection.unwrap();
                    assert_eq!(selection.anchor, head);
                    assert_eq!(
                        selection.head,
                        layout.source_offset_for_index(boundaries[1])
                    );
                    assert_eq!(
                        selection.affinity,
                        Some(WrapBoundaryAffinityKind::Downstream)
                    );
                    let start_position =
                        layout.origin + point(px(0.0), second_row + px(layout.line_height * 0.5));
                    editor.on_mouse_down(
                        &MouseDownEvent {
                            position: start_position,
                            button: MouseButton::Left,
                            click_count: 1,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    let end_position = layout.origin
                        + point(
                            px(0.0),
                            layout.wrapped.row_top(2, px(layout.line_height))
                                + px(layout.line_height * 0.5),
                        );
                    editor.on_mouse_move_inner(
                        &gpui::MouseMoveEvent {
                            position: end_position,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Default::default(),
                        },
                        window,
                        cx,
                    );
                    assert_eq!(
                        editor.line_diff_selection.unwrap().range(),
                        selection.range()
                    );
                    assert_eq!(
                        editor.line_diff_selection.unwrap().affinity,
                        selection.affinity
                    );
                    editor.on_mouse_up(
                        &MouseUpEvent {
                            position: end_position,
                            button: MouseButton::Left,
                            click_count: 1,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    editor.on_editor_context_menu(
                        &MouseDownEvent {
                            position: start_position,
                            button: MouseButton::Right,
                            click_count: 1,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    assert_eq!(
                        editor.line_diff_selection.unwrap().range(),
                        selection.range()
                    );
                    editor.execute_format_menu_action(FormatMenuAction::Copy, cx);
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        saved[selection.range()].to_string()
                    );
                    editor.execute_format_menu_action(FormatMenuAction::Heading1, cx);
                    editor.move_line_diff_selection_vertical(true, true, cx);
                    assert_eq!(
                        editor.line_diff_selection.unwrap().head,
                        layout.source_offset_for_index(boundaries[2])
                    );
                    editor.move_line_diff_selection_vertical(false, true, cx);
                    assert_eq!(editor.line_diff_selection.unwrap().head, selection.head);
                    editor.on_copy(&Copy, window, cx);
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        saved[selection.range()].to_string()
                    );
                    editor.on_select_all(&SelectAll, window, cx);
                    assert_eq!(editor.line_diff_selection.unwrap().range(), 0..saved.len());
                    editor.on_move_to_document_start(&MoveToDocumentStart, window, cx);
                    editor.on_select_word_right(&SelectWordRight, window, cx);
                    let mut reference = Cursor::new();
                    reference.select_word_right(&Buffer::from_text(&saved));
                    assert_eq!(
                        editor.line_diff_selection.unwrap().range(),
                        reference.selection_range()
                    );
                    editor.on_delete(&Delete, window, cx);
                    editor.on_cut(&Cut, window, cx);
                    editor.on_undo(&Undo, window, cx);
                    editor.on_add_cursor_below(&AddCursorBelow, window, cx);
                    editor.on_select_next_occurrence(&SelectNextOccurrence, window, cx);
                    editor.on_select_all_occurrences(&SelectAllOccurrences, window, cx);
                    assert!(editor.extra_cursors.is_empty());
                    assert_eq!(editor.buffer.text(), original_text);
                    assert_eq!(editor.cursor, original_cursor);
                    editor.on_move_to_document_start(&MoveToDocumentStart, window, cx);
                    assert!(editor.blink_epoch > 0);
                });
            });
            cx.run_until_parked();
            let snapshot = caret.borrow();
            assert!(snapshot.line_diff_selection.is_some());
            assert_eq!(snapshot.paints.len(), 1);
            assert!(!snapshot.paints[0].force_visible);
        }

        #[gpui::test]
        fn line_diff_pointer_drag_paints_each_wrap_row_and_restore_click_restores_saved_text(
            cx: &mut gpui::TestAppContext,
        ) {
            cx.update(|cx| {
                cx.bind_keys([
                    gpui::KeyBinding::new("shift-down", SelectDown, Some("MarkdownEditor")),
                    gpui::KeyBinding::new("shift-right", SelectRight, Some("MarkdownEditor")),
                ])
            });
            let saved_line = format!("old value {} end", "wide 字 e\u{301} words ".repeat(120));
            let saved = format!("{saved_line}\nlast");
            let current = saved.replacen("old", "new", 1);
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(&current), false);
                    editor.embedded_editor = false;
                    editor.app_settings.auto_save = false;
                    editor.styles.ui.spec.animations_enabled = false;
                    let index = editor.active_tab_index().unwrap();
                    editor.tabs[index].history.saved_text = Some(saved.clone());
                    editor.refresh_line_diff();
                    editor.toggle_line_diff_hunk(0, cx);
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            cx.run_until_parked();
            let (start, end, expected, scroll_before) = cx.update(|_, cx| {
                let editor = editor.read(cx);
                let layout = &editor.line_diff_selection_layouts[0];
                let point_for_row = |row| {
                    layout.origin
                        + point(
                            px(10.0),
                            layout.wrapped.row_top(row, px(layout.line_height))
                                + px(layout.line_height * 0.5),
                        )
                };
                let start = point_for_row(1);
                let end = point_for_row(3);
                (
                    start,
                    end,
                    layout.selection_at(start).0..layout.selection_at(end).0,
                    editor.scroll_offset,
                )
            });
            cx.simulate_mouse_move(start, None, Default::default());
            cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
            cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
            cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |editor, _| {
                    let selection = editor.line_diff_selection.unwrap();
                    assert_eq!(selection.range(), expected);
                    assert!(!editor.line_diff_selection_dragging);
                    let layout = &editor.line_diff_selection_layouts[0];
                    let map = (0..=saved_line.len()).collect::<Vec<_>>();
                    let mut backgrounds = Vec::new();
                    crate::editor::element::add_line_diff_text_background(
                        &selection.range(),
                        &map,
                        &layout.display_text,
                        &layout.wrapped,
                        f32::from(layout.origin.y),
                        layout.line_height,
                        f32::from(layout.origin.x),
                        layout.wrap_width,
                        0.0,
                        editor.styles.document_selection_color(false),
                        &mut backgrounds,
                    );
                    assert_eq!(backgrounds.len(), 3);
                    for (quad, row) in backgrounds.iter().zip(1..=3) {
                        assert_eq!(
                            quad.bounds.top(),
                            layout.origin.y + layout.wrapped.row_top(row, px(layout.line_height))
                        );
                        assert!(quad.bounds.left() >= layout.origin.x);
                        assert!(
                            quad.bounds.right() <= layout.origin.x + px(layout.wrap_width + 1.0)
                        );
                    }
                });
            });
            cx.simulate_mouse_move(start, None, Default::default());
            cx.simulate_click(start, Default::default());
            cx.simulate_keystrokes("shift-down shift-down shift-right");
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                let selection = editor.line_diff_selection.unwrap();
                assert_eq!(selection.anchor, expected.start);
                assert!(selection.head > expected.end);
                assert_eq!(editor.buffer.text(), current);
            });
            let button_center = cx.update(|_, cx| {
                let editor = editor.read(cx);
                let (button, _) = editor.line_diff_restore_areas[0];
                assert!(button.top() >= editor.line_diff_selection_layouts[0].clip_top());
                assert!(
                    button.left()
                        <= editor.line_diff_selection_layouts[0].origin.x
                            + px(editor.scaled(editor.styles.line_diff.restore_inset_px) + 0.5)
                );
                button.center()
            });
            cx.simulate_mouse_move(button_center, None, Default::default());
            cx.simulate_click(button_center, Default::default());
            cx.run_until_parked();
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    assert_eq!(editor.buffer.text(), saved);
                    assert!(editor.line_diff.hunks.is_empty());
                    assert!(editor.line_diff_selection.is_none());
                    assert_eq!(editor.scroll_offset, scroll_before);
                    editor.on_undo(&Undo, window, cx);
                    assert_eq!(editor.buffer.text(), current);
                    assert_eq!(editor.line_diff.hunks.len(), 1);
                })
            });
        }

        #[gpui::test]
        fn line_diff_current_wrapped_rows_support_pointer_keyboard_selection_and_edits(
            cx: &mut gpui::TestAppContext,
        ) {
            for mode in 0..3 {
                let current = format!("new value {}\nlast", "wide 字 words ".repeat(60));
                let editor = cx.update(|cx| {
                    cx.new(|cx| {
                        let mut editor =
                            EditorView::new_with_mode(cx, true, false, Some(&current), false);
                        editor.embedded_editor = false;
                        editor.app_settings.auto_save = false;
                        editor.styles.ui.spec.animations_enabled = false;
                        editor.source_mode = mode == 1;
                        editor.editor_left_aligned = false;
                        let index = editor.active_tab_index().unwrap();
                        if mode == 2 {
                            let kind = FileKind::Code {
                                language: "Rust".into(),
                            };
                            editor.tabs[index].file_kind = Some(kind.clone());
                            editor.active_file_kind_cache = Some(kind);
                        }
                        editor.tabs[index].history.saved_text = Some("old value\nlast".into());
                        editor.refresh_line_diff();
                        editor.toggle_line_diff_hunk(0, cx);
                        editor
                    })
                });
                let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                    editor: editor.clone(),
                    caret: Default::default(),
                    _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                });
                cx.run_until_parked();
                let (positions, offsets) = cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    assert!(editor.line_diff.hunks[0].expanded);
                    let info = editor.line_display_info[0].as_ref().unwrap();
                    assert!(info.wrapped_line.wrap_boundaries().len() >= 3);
                    let mut positions = Vec::new();
                    let mut offsets = Vec::new();
                    for row in 0..=3 {
                        let local = point(
                            px(25.0),
                            info.wrapped_line.row_top(row, px(info.line_height))
                                + info.wrapped_line.row_height(row, px(info.line_height)) * 0.5,
                        );
                        let position = point(
                            px(info.content_x) + local.x,
                            editor.content_origin.y
                                + px(editor.text_y_offset_for_line(0) - editor.scroll_offset)
                                + local.y,
                        );
                        let display = match info
                            .wrapped_line
                            .closest_index_for_position(local, px(info.line_height))
                        {
                            Ok(index) | Err(index) => index,
                        };
                        let offset = info.display_to_buffer[display];
                        assert_eq!(
                            editor.offset_for_position(position),
                            offset,
                            "mode {mode}, current visual row {row}"
                        );
                        positions.push(position);
                        offsets.push(offset);
                    }
                    (positions, offsets)
                });
                cx.simulate_click(positions[0], Default::default());
                cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    assert!(editor.line_diff_selection.is_none());
                    assert_eq!(editor.cursor.offset, offsets[0]);
                    assert_eq!(
                        editor
                            .cursor_visual_position(editor.cursor.offset)
                            .unwrap()
                            .0,
                        0
                    );
                });
                cx.simulate_mouse_down(positions[0], MouseButton::Left, Default::default());
                cx.simulate_mouse_move(positions[3], Some(MouseButton::Left), Default::default());
                cx.simulate_mouse_up(positions[3], MouseButton::Left, Default::default());
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        assert!(editor.line_diff_selection.is_none());
                        assert_eq!(editor.cursor.selection_range(), offsets[0]..offsets[3]);
                        editor.on_copy(&Copy, window, cx);
                        assert_eq!(
                            cx.read_from_clipboard().unwrap().text().unwrap(),
                            current[offsets[0]..offsets[3]].to_string()
                        );
                    });
                });
                cx.simulate_click(positions[2], Default::default());
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        for row in [1, 0] {
                            editor.on_up(&crate::Up, window, cx);
                            assert_eq!(
                                editor
                                    .cursor_visual_position(editor.cursor.offset)
                                    .unwrap()
                                    .0,
                                row
                            );
                        }
                        let start = editor.cursor.offset;
                        editor.on_select_down(&SelectDown, window, cx);
                        editor.on_select_down(&SelectDown, window, cx);
                        assert_eq!(editor.cursor.anchor, start);
                        assert_eq!(
                            editor
                                .cursor_visual_position(editor.cursor.offset)
                                .unwrap()
                                .0,
                            2
                        );
                        let selected = editor.cursor.selection_range();
                        let mut expected = current.clone();
                        expected.replace_range(selected, "");
                        editor.on_delete(&Delete, window, cx);
                        assert_eq!(editor.buffer.text(), expected);
                        editor.on_undo(&Undo, window, cx);
                        assert_eq!(editor.buffer.text(), current);
                    });
                });
            }
        }

        #[gpui::test]
        fn line_diff_live_comparison_survives_edits_no_difference_and_history_until_cursor_leaves(
            cx: &mut gpui::TestAppContext,
        ) {
            for mode in 0..3 {
                let saved = "old value\nlast";
                let editor = cx.update(|cx| {
                    cx.new(|cx| {
                        let mut editor = EditorView::new_with_mode(
                            cx,
                            true,
                            false,
                            Some("new value\nlast"),
                            false,
                        );
                        editor.embedded_editor = false;
                        editor.app_settings.auto_save = false;
                        editor.styles.ui.spec.animations_enabled = false;
                        editor.source_mode = mode == 1;
                        let index = editor.active_tab_index().unwrap();
                        if mode == 2 {
                            let kind = FileKind::Code {
                                language: "Rust".into(),
                            };
                            editor.tabs[index].file_kind = Some(kind.clone());
                            editor.active_file_kind_cache = Some(kind);
                        }
                        editor.tabs[index].history.saved_text = Some(saved.into());
                        editor.refresh_line_diff();
                        editor.toggle_line_diff_hunk(0, cx);
                        editor
                    })
                });
                let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                    editor: editor.clone(),
                    caret: Default::default(),
                    _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.cursor.anchor = 0;
                        editor.cursor.offset = 3;
                        editor.on_delete(&Delete, window, cx);
                        assert!(editor.line_diff.hunks.iter().any(|hunk| hunk.expanded));
                        editor.on_undo(&Undo, window, cx);
                        assert!(editor.line_diff.hunks.iter().any(|hunk| hunk.expanded));
                        editor.on_redo(&crate::Redo, window, cx);
                        assert!(editor.line_diff.hunks.iter().any(|hunk| hunk.expanded));
                        editor.on_undo(&Undo, window, cx);
                        editor.checkpoint();
                        editor.queue_non_markdown_line_display_transfer(&(0..3), "old");
                        editor.buffer.replace(0..3, "old");
                        editor.cursor.move_to(3, &editor.buffer);
                        editor.reparse();
                        editor.pause_blinking(cx);
                        assert_eq!(editor.buffer.text(), saved);
                        assert!(editor.line_diff.hunks.iter().any(|hunk| hunk.expanded));
                        assert!(editor.line_diff.markers().is_empty());
                        assert!(
                            editor.line_diff_preview_lines(0, false)[0]
                                .changes
                                .is_empty()
                        );
                    });
                });
                cx.run_until_parked();
                let old = cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    assert!(!editor.line_diff_selection_layouts.is_empty());
                    let layout = &editor.line_diff_selection_layouts[0];
                    layout.origin + point(px(100.0), px(layout.line_height * 0.5))
                });
                cx.simulate_click(old, Default::default());
                cx.update(|_, cx| {
                    assert!(editor.read(cx).line_diff_selection.is_some());
                    assert!(editor.read(cx).line_diff_comparison.is_some());
                });
                let new = cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    point(
                        px(editor.line_display_info[0].as_ref().unwrap().content_x + 50.0),
                        editor.content_origin.y
                            + px(editor.text_y_offset_for_line(0) - editor.scroll_offset + 8.0),
                    )
                });
                cx.simulate_click(new, Default::default());
                cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    assert!(editor.line_diff_selection.is_none());
                    assert!(editor.line_diff_comparison.is_some());
                });
                if mode == 1 {
                    let outside = cx.update(|_, cx| {
                        let editor = editor.read(cx);
                        point(
                            px(editor.line_display_info[1].as_ref().unwrap().content_x + 25.0),
                            editor.content_origin.y
                                + px(editor.text_y_offset_for_line(1) - editor.scroll_offset + 8.0),
                        )
                    });
                    cx.simulate_click(outside, Default::default());
                    cx.update(|_, cx| {
                        assert!(editor.read(cx).line_diff_comparison.is_none());
                        assert!(editor.read(cx).line_diff.hunks.is_empty());
                    });
                    continue;
                }
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.on_down(&crate::Down, window, cx);
                        assert_eq!(editor.cursor_line(), 1);
                        assert!(editor.line_diff_comparison.is_none());
                        assert!(editor.line_diff.hunks.is_empty());
                        editor.on_up(&crate::Up, window, cx);
                        assert!(editor.line_diff.hunks.is_empty());
                    });
                });
            }
        }

        #[gpui::test]
        fn line_diff_live_comparison_tracks_split_lines_and_closes_with_animation(
            cx: &mut gpui::TestAppContext,
        ) {
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some("new value\nlast"), false);
                    editor.embedded_editor = false;
                    editor.app_settings.auto_save = false;
                    editor.styles.ui.spec.animations_enabled = false;
                    let index = editor.active_tab_index().unwrap();
                    editor.tabs[index].history.saved_text = Some("old value\nlast".into());
                    editor.refresh_line_diff();
                    editor.toggle_line_diff_hunk(0, cx);
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.cursor.move_to(3, &editor.buffer);
                    editor.on_enter(&crate::Enter, window, cx);
                    let hunk = editor
                        .line_diff
                        .hunks
                        .iter()
                        .find(|hunk| hunk.expanded)
                        .unwrap();
                    assert_eq!(hunk.current_lines, 0..2);
                    assert_eq!(editor.cursor_line(), 1);
                    assert!(editor.line_diff_comparison.is_some());
                    editor.on_backspace(&crate::Backspace, window, cx);
                    let hunk = editor
                        .line_diff
                        .hunks
                        .iter()
                        .find(|hunk| hunk.expanded)
                        .unwrap();
                    assert_eq!(hunk.current_lines, 0..1);
                    editor
                        .cursor
                        .move_to(editor.buffer.line_end(0), &editor.buffer);
                    editor.on_enter(&crate::Enter, window, cx);
                    assert_eq!(
                        editor
                            .line_diff_comparison
                            .as_ref()
                            .unwrap()
                            .hunk
                            .current_lines,
                        0..2
                    );
                    editor.on_undo(&Undo, window, cx);
                    assert_eq!(
                        editor
                            .line_diff_comparison
                            .as_ref()
                            .unwrap()
                            .hunk
                            .current_lines,
                        0..1
                    );
                    editor.styles.ui.spec.animations_enabled = true;
                    editor
                        .cursor
                        .move_to(editor.buffer.line_start(1), &editor.buffer);
                    editor.pause_cursor_blinking(cx);
                    assert!(editor.line_diff_comparison.is_none());
                    assert_eq!(editor.line_diff_reveal_target, 0.0);
                    assert!(editor.line_diff.hunks.iter().any(|hunk| hunk.expanded));
                    let started = editor.line_diff_reveal_started.unwrap();
                    editor
                        .update_line_diff_reveal(started + editor.styles.line_diff.reveal_duration);
                    assert!(editor.line_diff.hunks.iter().all(|hunk| !hunk.expanded));
                });
            });
        }

        #[gpui::test]
        fn line_diff_restore_handles_deleted_first_and_final_lines_and_crlf(
            cx: &mut gpui::TestAppContext,
        ) {
            for (saved, current) in [
                ("removed\nlast", "last"),
                ("first\nremoved", "first"),
                ("first\r\nremoved\r\nlast", "first\r\nlast"),
                ("first\r\nold value", "first\r\nnew value"),
            ] {
                let editor = cx.update(|cx| {
                    cx.new(|cx| {
                        let mut editor =
                            EditorView::new_with_mode(cx, true, false, Some(current), false);
                        editor.embedded_editor = false;
                        editor.styles.ui.spec.animations_enabled = false;
                        let index = editor.active_tab_index().unwrap();
                        editor.tabs[index].history.saved_text = Some(saved.into());
                        editor.refresh_line_diff();
                        editor.toggle_line_diff_hunk(0, cx);
                        editor
                    })
                });
                let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                    editor: editor.clone(),
                    caret: Default::default(),
                    _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                });
                cx.run_until_parked();
                let button =
                    cx.update(|_, cx| editor.read(cx).line_diff_restore_areas[0].0.center());
                cx.simulate_mouse_move(button, None, Default::default());
                cx.simulate_click(button, Default::default());
                cx.run_until_parked();
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        assert_eq!(editor.buffer.text(), saved);
                        assert!(editor.line_diff.hunks.is_empty());
                        editor.on_undo(&Undo, window, cx);
                        assert_eq!(editor.buffer.text(), current);
                    })
                });
            }
        }

        #[gpui::test]
        fn checkbox_clicks_do_not_reveal_an_unchanged_offscreen_cursor(
            cx: &mut gpui::TestAppContext,
        ) {
            for block in [
                "- [ ] task",
                "| Task | Value |\n| --- | --- |\n| - [ ] task | value |",
            ] {
                for cursor_at_end in [false, true] {
                    let source = format!(
                        "{}{block}\n\n{}",
                        "before\n\n".repeat(80),
                        "after\n\n".repeat(100),
                    );
                    let editor = cx.update(|cx| {
                        cx.new(|cx| {
                            let mut editor =
                                EditorView::new_with_mode(cx, true, false, Some(&source), false);
                            editor.embedded_editor = false;
                            editor.styles.ui.spec.animations_enabled = false;
                            editor
                        })
                    });
                    let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                        editor: editor.clone(),
                        caret: Default::default(),
                        _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                    });
                    cx.update(|_, cx| {
                        editor.update(cx, |editor, cx| {
                            let offset = if cursor_at_end {
                                editor.buffer.len()
                            } else {
                                0
                            };
                            editor.cursor.move_to(offset, &editor.buffer);
                            let line = editor
                                .buffer
                                .offset_to_line_col(editor.buffer.text().find("[ ]").unwrap())
                                .0;
                            editor.scroll_offset = editor.text_y_offset_for_line(line) - 80.0;
                            editor.clamp_scroll();
                            cx.notify();
                        });
                    });
                    cx.run_until_parked();
                    let (position, before_scroll, before_cursor) = cx.update(|_, cx| {
                        let editor = editor.read(cx);
                        assert!(!editor.cursor_visible_in_viewport());
                        let bounds = editor.checkbox_areas.first().expect("painted checkbox").0;
                        assert!(bounds.top() >= editor.content_origin.y);
                        (bounds.center(), editor.scroll_offset, editor.cursor)
                    });
                    cx.simulate_event(MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        ..Default::default()
                    });
                    cx.simulate_event(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        ..Default::default()
                    });
                    cx.run_until_parked();
                    cx.update(|_, cx| {
                        let editor = editor.read(cx);
                        assert!(editor.buffer.text().contains("[x]"), "{block}");
                        assert_eq!(editor.cursor.offset, before_cursor.offset);
                        assert_eq!(editor.cursor.anchor, before_cursor.anchor);
                        assert!(
                            (editor.scroll_offset - before_scroll).abs() < 0.5,
                            "{block}: unchanged offscreen cursor moved viewport {} -> {}",
                            before_scroll,
                            editor.scroll_offset,
                        );
                        assert!(!editor.pending_scroll_to_cursor);
                        assert!(!editor.cursor_visible_in_viewport());
                    });
                }
            }
        }

        #[gpui::test]
        fn forward_deletion_without_cursor_movement_preserves_offscreen_viewport(
            cx: &mut gpui::TestAppContext,
        ) {
            let source = "paragraph\n\n".repeat(160);
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(&source), false);
                    editor.embedded_editor = false;
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            for offset in [0, source.len()] {
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.cursor.move_to(offset, &editor.buffer);
                        editor.scroll_offset = 1000.0;
                        cx.notify();
                    });
                });
                cx.run_until_parked();
                let before_scroll = cx.update(|_, cx| editor.read(cx).scroll_offset);
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.on_delete(&crate::actions::Delete, window, cx);
                    });
                });
                cx.run_until_parked();
                cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    assert_eq!(editor.cursor.offset, offset.min(editor.buffer.len()));
                    assert!((editor.scroll_offset - before_scroll).abs() < 0.5);
                    assert!(!editor.pending_scroll_to_cursor);
                });
            }
        }

        #[gpui::test]
        fn visible_occurrence_and_table_edits_preserve_viewport(cx: &mut gpui::TestAppContext) {
            let source = format!(
                "{}word\n\nword\n\n| A | B |\n| --- | --- |\n| one | two |\n| three | four |\n\n{}",
                "paragraph\n\n".repeat(30),
                "tail\n\n".repeat(80),
            );
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(&source), false);
                    editor.embedded_editor = false;
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            let before = cx.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    editor.styles.ui.spec.animations_enabled = false;
                    let start = editor.buffer.text().find("word").unwrap();
                    editor.cursor = crate::cursor::Cursor {
                        anchor: start,
                        offset: start + 4,
                    };
                    editor.scroll_offset =
                        editor.text_y_offset_for_line(editor.cursor_line()) - 30.0;
                    editor.clamp_scroll();
                    cx.notify();
                    editor.scroll_offset
                })
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    assert!(!editor.layout_dirty);
                    assert!(editor.cursor_vertical_bounds().is_some());
                    editor.on_select_next_occurrence(
                        &crate::actions::SelectNextOccurrence,
                        window,
                        cx,
                    );
                    assert!(!editor.pending_center_cursor_in_view);
                });
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    assert!(
                        (editor.scroll_offset - before).abs() < 0.5,
                        "CMD+D moved {} -> {}",
                        before,
                        editor.scroll_offset
                    );
                    assert_eq!(editor.extra_cursors.len(), 1);
                    let table = editor
                        .decorations
                        .iter()
                        .position(|row| row.table_info.is_some())
                        .unwrap();
                    editor.table_delete_rows(table, &[2], cx);
                });
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    assert!(
                        (editor.scroll_offset - before).abs() < 0.5,
                        "first row deletion moved viewport"
                    );
                    let table = editor
                        .decorations
                        .iter()
                        .position(|row| row.table_info.is_some())
                        .unwrap();
                    editor.table_delete_columns(table, &[1], cx);
                });
            });
            cx.run_until_parked();
            assert!(cx.update(|_, cx| (editor.read(cx).scroll_offset - before).abs() < 0.5));
        }

        #[gpui::test]
        fn offscreen_occurrence_still_reveals_its_cursor(cx: &mut gpui::TestAppContext) {
            let source = format!("word\n{}word", "paragraph\n\n".repeat(150));
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(&source), false);
                    editor.embedded_editor = false;
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                editor: editor.clone(),
                caret: Default::default(),
                _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
            });
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.styles.ui.spec.animations_enabled = false;
                    editor.scroll_offset = 0.0;
                    editor.cursor = crate::cursor::Cursor {
                        anchor: 0,
                        offset: 4,
                    };
                    editor.on_select_next_occurrence(
                        &crate::actions::SelectNextOccurrence,
                        window,
                        cx,
                    );
                });
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert!(editor.scroll_offset > 0.0);
                assert!(
                    editor.cursor_visible_in_viewport(),
                    "cursor={:?}, scroll={}, viewport={}, bounds={:?}, dirty={}, pending={}",
                    editor.cursor,
                    editor.scroll_offset,
                    editor.editor_visible_height(),
                    editor.cursor_vertical_bounds(),
                    editor.layout_dirty,
                    editor.pending_scroll_to_cursor
                );
            });
        }

        #[gpui::test]
        fn trailing_blank_line_requires_an_editable_final_rendered_block(
            cx: &mut gpui::TestAppContext,
        ) {
            let table = "| A | B |\n| --- | --- |\n| one | two |";
            let with_newline = format!("{table}\n");
            for (source, read_only, reading, source_mode) in [
                (table, true, false, false),
                (table, false, true, false),
                (table, false, false, true),
                ("ordinary text", false, false, false),
                (with_newline.as_str(), false, false, false),
            ] {
                let editor = cx.update(|cx| {
                    cx.new(|cx| {
                        let mut editor =
                            EditorView::new_with_mode(cx, true, read_only, Some(source), false);
                        editor.reading_mode = reading;
                        editor.source_mode = source_mode;
                        editor
                    })
                });
                let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                    editor: editor.clone(),
                    caret: Default::default(),
                    _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                });
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        assert!(!editor.layout_dirty);
                        let bottom = editor.content_origin.y
                            + px(editor.total_content_height()
                                - editor.padding_bottom()
                                - editor.scroll_offset);
                        let position = point(px(editor.last_content_left + 20.0), bottom + px(5.0));
                        assert!(!editor.append_line_after_final_rendered_block(position, cx));
                        assert_eq!(editor.buffer.text(), source);
                    });
                });
            }
        }

        #[gpui::test]
        fn clicking_after_final_rendered_blocks_creates_one_undoable_blank_line(
            cx: &mut gpui::TestAppContext,
        ) {
            for source in [
                "| A | B |\n| --- | --- |\n| one | two |",
                "$$\nx+y\n$$",
                "```mermaid\ngraph TD\nA-->B\n```",
            ] {
                let editor = cx.update(|cx| {
                    cx.new(|cx| {
                        let mut editor =
                            EditorView::new_with_mode(cx, true, false, Some(source), false);
                        if let Some(key) = editor.decorations.iter().find_map(|row| {
                            row.block_preview
                                .as_ref()
                                .map(|preview| preview.cache_key.clone())
                        }) {
                            editor.image_cache.insert(key, Some(image()));
                        }
                        editor
                    })
                });
                let (_, cx) = cx.add_window_view(|_, cx| PaintedEditor {
                    editor: editor.clone(),
                    caret: Default::default(),
                    _observer: cx.observe(&editor, |_, _, cx| cx.notify()),
                });
                let position = cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        assert!(!editor.layout_dirty);
                        let bottom = editor.content_origin.y
                            + px(editor.total_content_height()
                                - editor.padding_bottom()
                                - editor.scroll_offset);
                        let x = px(editor.last_editor_left + editor.last_editor_width - 20.0);
                        assert!(!editor.append_line_after_final_rendered_block(
                            point(x, bottom - px(5.0)),
                            cx,
                        ));
                        assert_eq!(editor.buffer.text(), source);
                        point(x, bottom + px(5.0))
                    })
                });
                cx.simulate_event(MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    click_count: 1,
                    ..Default::default()
                });
                cx.simulate_event(MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    click_count: 1,
                    ..Default::default()
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        assert_eq!(editor.buffer.text(), format!("{source}\n"));
                        assert_eq!(editor.cursor.offset, editor.buffer.len());
                        assert_eq!(editor.buffer.offset_to_line_col(editor.cursor.offset).1, 0);
                        editor.on_undo(&crate::actions::Undo, window, cx);
                        assert_eq!(editor.buffer.text(), source);
                    });
                });
            }
        }

        #[gpui::test]
        fn double_click_survives_editor_repaint_between_clicks(cx: &mut gpui::TestAppContext) {
            for (source, generated) in [
                ("![image](image.png)", false),
                ("before\n\n$$\nx+y\n$$\n\nafter", true),
                ("before\n\n```mermaid\ngraph TD\nA-->B\n```\n\nafter", true),
            ] {
                check_painted_double_click(cx, source, generated);
            }
        }

        #[gpui::test]
        fn double_click_opens_markdown_previews_in_edit_and_read_only_modes(
            cx: &mut gpui::TestAppContext,
        ) {
            let cx = cx.add_empty_window();
            let bounds = Bounds::new(point(px(10.0), px(10.0)), size(px(80.0), px(80.0)));
            let position = point(px(40.0), px(40.0));
            for read_only in [false, true] {
                for (source, generated) in [
                    ("![image](image.png)", false),
                    ("$$\nx+y\n$$", true),
                    ("```mermaid\ngraph TD\nA-->B\n```", true),
                ] {
                    let editor = cx.update(|_, cx| {
                        cx.new(|cx| {
                            EditorView::new_with_mode(cx, true, read_only, Some(source), false)
                        })
                    });
                    let (line, cache_key) = cx.update(|_, cx| {
                        editor.update(cx, |editor, _| {
                            let (line, cache_key) = editor
                                .decorations
                                .iter()
                                .enumerate()
                                .find_map(|(line, decoration)| {
                                    if generated {
                                        decoration
                                            .block_preview
                                            .as_ref()
                                            .filter(|preview| preview.is_anchor)
                                            .map(|preview| (line, preview.cache_key.clone()))
                                    } else {
                                        decoration.image_path.clone().map(|key| (line, key))
                                    }
                                })
                                .expect("preview metadata");
                            editor.image_cache.insert(cache_key.clone(), Some(image()));
                            if generated {
                                editor.generated_preview_image_areas.push((bounds, line));
                            } else {
                                editor.image_click_areas.push((bounds, line));
                            }
                            (line, cache_key)
                        })
                    });
                    mouse_down(
                        cx,
                        &editor,
                        MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            click_count: 1,
                            ..Default::default()
                        },
                    );
                    assert!(cx.update(|_, cx| editor.read(cx).preview_overlay.is_none()));
                    mouse_up(
                        cx,
                        &editor,
                        MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            click_count: 1,
                            ..Default::default()
                        },
                    );
                    mouse_down(
                        cx,
                        &editor,
                        MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            click_count: 2,
                            ..Default::default()
                        },
                    );
                    cx.update(|_, cx| {
                        let editor = editor.read(cx);
                        let overlay = editor.preview_overlay.as_ref().expect("fullscreen preview");
                        assert_eq!(overlay.line_idx, Some(line));
                        assert_eq!(overlay.cache_key, cache_key);
                        assert!(editor.generated_preview_drag_state.is_none());
                    });
                    mouse_up(
                        cx,
                        &editor,
                        MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            click_count: 2,
                            ..Default::default()
                        },
                    );
                    assert!(cx.update(|_, cx| editor.read(cx).preview_overlay.is_some()));
                }
            }
        }

        #[gpui::test]
        fn double_click_on_standalone_image_opens_preview_but_background_resets(
            cx: &mut gpui::TestAppContext,
        ) {
            let cx = cx.add_empty_window();
            let editor = cx.update(|_, cx| {
                cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false))
            });
            cx.update(|_, cx| {
                editor.update(cx, |editor, _| {
                    editor.tabs[0].file_kind = Some(FileKind::Image);
                    editor.tabs[0].path = Some(PathBuf::from("image.png"));
                    editor.tabs[0].image_view.zoom_factor = 0.5;
                    editor.active_file_kind_cache = Some(FileKind::Image);
                    editor.image_cache.insert("image.png".into(), Some(image()));
                });
            });
            let pane_id = cx.update(|_, cx| editor.read(cx).current_pane_id());
            cx.draw(
                point(px(0.0), px(0.0)),
                size(px(100.0), px(100.0)),
                |_, _| crate::editor::image_view::ImageViewElement {
                    editor: editor.clone(),
                    pane_id,
                },
            );
            cx.update(|_, cx| {
                assert_eq!(
                    editor.read(cx).active_image_bounds,
                    Some(Bounds::new(
                        point(px(25.0), px(37.5)),
                        size(px(50.0), px(25.0)),
                    ))
                );
            });
            mouse_down(
                cx,
                &editor,
                MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(px(90.0), px(90.0)),
                    click_count: 2,
                    ..Default::default()
                },
            );
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert!(editor.preview_overlay.is_none());
                assert_eq!(editor.active_image_view_state().unwrap().zoom_factor, 1.0);
            });
            mouse_down(
                cx,
                &editor,
                MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(px(50.0), px(50.0)),
                    click_count: 2,
                    ..Default::default()
                },
            );
            cx.update(|_, cx| {
                let overlay = editor
                    .read(cx)
                    .preview_overlay
                    .as_ref()
                    .expect("fullscreen preview");
                assert_eq!(overlay.line_idx, None);
                assert_eq!(overlay.cache_key, "image.png");
            });
        }
    }

    #[test]
    fn front_matter_key_and_continuation_rows_share_the_input_inset() {
        use super::super::{element::front_matter_content_geometry, front_matter};
        use crate::buffer::Buffer;

        let source = "---\nobject:\n  title: '你好🌸'\nitems:\n  - first\n  - second\nbody: |\n  key: literal\n  another line\n---\nordinary: mapping";
        let buffer = Buffer::from_text(source);
        let decorations = crate::markdown::parse_decorations(source);
        let mut keyed_rows = Vec::new();
        for (line, decoration) in decorations.iter().enumerate() {
            let has_type_column = decoration
                .table_info
                .as_ref()
                .is_some_and(|table| table.is_front_matter && !table.is_separator);
            let geometry = front_matter_content_geometry(has_type_column, 100.0, 300.0, 24.0);
            assert_eq!(
                geometry,
                if (1..9).contains(&line) {
                    (124.0, 276.0)
                } else {
                    (100.0, 300.0)
                }
            );
            if front_matter::property_type(&buffer, &decorations, line).is_some() {
                keyed_rows.push(line);
            }
        }
        assert_eq!(keyed_rows, vec![1, 2, 3, 6]);
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn front_matter_column_keeps_active_utf8_and_ime_offsets_source_relative() {
        use super::super::{element::front_matter_content_geometry, helpers};

        let source = "  title: '你好🌸'";
        let document = format!("---\nobject:\n{source}\n---");
        let decorations = crate::markdown::parse_decorations(&document);
        let decoration = &decorations[2];
        let marked_start = source.find('你').unwrap();
        let active_ranges = [marked_start..marked_start + "你好🌸".len()];
        let (display, display_to_buffer, buffer_to_display, _) = helpers::build_display_text(
            source,
            decoration,
            &active_ranges,
            true,
            None,
            0,
            3,
            3,
            2,
            0,
            0.7,
            0.7,
            0.7,
        );
        assert!(display.starts_with("  title:"));
        let (content_x, wrap_width) = front_matter_content_geometry(true, 100.0, 300.0, 24.0);
        assert_eq!((content_x, wrap_width), (124.0, 276.0));
        for col in marked_start..=source.len() {
            if !source.is_char_boundary(col) {
                continue;
            }
            let display_col = buffer_to_display[col];
            assert!(display.is_char_boundary(display_col));
            assert_eq!(
                display_index_to_buffer_col(&display, &display_to_buffer, display_col),
                col,
            );
        }
        assert_eq!(
            &display[buffer_to_display[marked_start]..],
            &source[marked_start..]
        );
    }

    fn test_table_cell() -> TableCellDisplayInfo {
        let display_text = "abcdef\nabcdef".to_string();
        TableCellDisplayInfo {
            wrapped_lines: vec![
                test_wrapped_line(&[24.0, 60.0, 24.0]),
                test_wrapped_line(&[24.0, 24.0, 80.0]),
            ],
            hard_line_ranges: vec![0..6, 7..13],
            hanging_wraps: Vec::new(),
            alignment: crate::markdown::TableAlign::Left,
            display_to_buffer: (0..display_text.len()).collect(),
            buffer_to_display: (0..display_text.len()).collect(),
            cell_range: 0..display_text.len(),
            display_source_end: display_text.len(),
            display_text,
            line_height: 24.0,
        }
    }

    #[test]
    fn table_hit_accumulates_actual_hard_line_heights_and_clamps_sibling_space() {
        let info = test_table_cell();
        assert_eq!(info.hard_line_top(0), 0.0);
        assert_eq!(info.hard_line_top(1), 108.0);
        assert_eq!(info.hard_line_top(2), 236.0);
        assert_eq!(table_cell_wrapped_line_for_y(&info, 70.0), Some((0, 70.0)));
        assert_eq!(table_cell_wrapped_line_for_y(&info, 108.0), Some((1, 0.0)));
        assert_eq!(table_cell_wrapped_line_for_y(&info, 130.0), Some((1, 22.0)));
        assert_eq!(table_cell_wrapped_line_for_y(&info, 400.0), Some((1, 88.0)));
    }

    #[test]
    fn table_vertical_navigation_targets_actual_soft_row_centers() {
        let info = test_table_cell();
        assert_eq!(
            table_cell_visual_position(&info, 5, 100.0, None),
            Some((2, 10.0))
        );
        assert_eq!(
            table_cell_visual_position(&info, 12, 100.0, None),
            Some((5, 10.0))
        );
        for (visual_row, offset) in [(0, 1), (1, 3), (2, 5), (3, 8), (4, 10), (5, 12)] {
            assert_eq!(
                table_cell_offset_for_visual_line(&info, visual_row, 10.0, 100.0),
                Some((offset, None))
            );
        }
    }

    #[test]
    fn table_zero_column_down_preserves_exact_wrap_boundary_row() {
        let info = test_table_cell();
        let mut target = (0, None);
        for (row, expected_offset) in [(1, 2), (2, 4), (3, 7), (4, 9), (5, 11)] {
            let (current_row, x) =
                table_cell_visual_position(&info, target.0, 100.0, target.1).unwrap();
            assert_eq!(current_row + 1, row);
            assert_eq!(x, 0.0);
            target = table_cell_offset_for_visual_line(&info, row, 0.0, 100.0).unwrap();
            assert_eq!(target.0, expected_offset);
            assert_eq!(
                table_cell_visual_position(&info, target.0, 100.0, target.1),
                Some((row, 0.0))
            );
        }
        for row in (0..5).rev() {
            let (offset, affinity) =
                table_cell_offset_for_visual_line(&info, row, 0.0, 100.0).unwrap();
            assert_eq!(
                table_cell_visual_position(&info, offset, 100.0, affinity),
                Some((row, 0.0))
            );
        }
        assert_eq!(
            table_cell_visual_position(&info, 2, 100.0, Some(WrapBoundaryAffinityKind::Upstream)),
            Some((0, 20.0)),
        );
    }

    #[test]
    fn table_affinity_geometry_includes_tall_prior_hard_lines() {
        let info = test_table_cell();
        for (offset, hard_line, expected_y) in [(2, 0, 24.0), (9, 1, 132.0), (11, 1, 156.0)] {
            let (line, pos) =
                table_cell_position(&info, offset, Some(WrapBoundaryAffinityKind::Downstream))
                    .unwrap();
            assert_eq!(line, hard_line);
            assert_eq!(info.hard_line_top(line) + f32::from(pos.y), expected_y);
            assert_eq!(pos.x, px(0.0));
        }
    }

    #[test]
    fn table_hanging_navigation_and_ime_positions_share_fragment_indents_and_tall_rows() {
        let mut info = test_table_cell();
        info.hanging_wraps = vec![
            super::super::table_cell_render::HangingWrap { indent: 13.5 },
            super::super::table_cell_render::HangingWrap { indent: 37.25 },
        ];
        for (row, offset, indent, y) in [
            (0, 0, 0.0, 0.0),
            (1, 2, 13.5, 24.0),
            (2, 4, 13.5, 84.0),
            (3, 7, 0.0, 108.0),
            (4, 9, 37.25, 132.0),
            (5, 11, 37.25, 156.0),
        ] {
            let target = table_cell_offset_for_visual_line(&info, row, indent, 100.0).unwrap();
            assert_eq!(target.0, offset);
            assert_eq!(
                table_cell_visual_position(&info, offset, 100.0, target.1),
                Some((row, indent))
            );
            let (hard_line, ime) = table_cell_position(&info, offset, target.1).unwrap();
            assert_eq!(f32::from(ime.x), indent);
            assert_eq!(info.hard_line_top(hard_line) + f32::from(ime.y), y);
        }
        assert_eq!(
            table_cell_visual_position(
                &info,
                4,
                100.0,
                Some(super::WrapBoundaryAffinityKind::Upstream)
            ),
            Some((1, 33.5))
        );
        assert_eq!(
            table_cell_visual_position(
                &info,
                4,
                100.0,
                Some(super::WrapBoundaryAffinityKind::Downstream)
            ),
            Some((2, 13.5))
        );
    }

    #[test]
    fn bottom_padding_hits_stay_in_last_measured_row() {
        let wrapped = test_wrapped_line(&[24.0, 60.0, 24.0]);
        for y in [108.0, 120.0, 400.0] {
            let y = EditorView::clamp_wrapped_hit_y(&wrapped, y, 24.0);
            assert_eq!(wrapped.row_for_y(px(y), px(24.0)), 2);
            assert_eq!(
                wrapped.closest_index_for_position(point(px(5.0), px(y)), px(24.0)),
                Ok(4)
            );
        }
        assert_eq!(EditorView::clamp_wrapped_hit_y(&wrapped, 70.0, 24.0), 70.0);
    }
    use gpui::{Bounds, point, px, size};

    #[test]
    fn table_drag_stays_on_row_in_empty_space_but_not_across_rows() {
        let cells = [
            (
                Bounds::new(point(px(10.), px(20.)), size(px(40.), px(25.))),
                2,
                0,
                0,
            ),
            (
                Bounds::new(point(px(50.), px(20.)), size(px(40.), px(25.))),
                2,
                0,
                1,
            ),
            (
                Bounds::new(point(px(10.), px(45.)), size(px(40.), px(25.))),
                2,
                1,
                0,
            ),
        ];
        assert_eq!(
            nearest_table_cell_in_row(&cells, 2, 0, point(px(150.), px(30.)))
                .unwrap()
                .3,
            1
        );
        assert_eq!(
            nearest_table_cell_in_row(&cells, 2, 0, point(px(-20.), px(30.)))
                .unwrap()
                .3,
            0
        );
        assert!(nearest_table_cell_in_row(&cells, 2, 0, point(px(150.), px(50.))).is_none());
    }

    #[test]
    fn visible_line_hit_uses_real_screen_geometry_at_far_document_offsets() {
        let geometry = [(1_999_990, 120.0, 72.0), (1_999_991, 192.0, 24.0)];
        assert_eq!(visible_line_hit(&geometry, 155.0), Some((1_999_990, 120.0)));
        assert_eq!(visible_line_hit(&geometry, 192.0), Some((1_999_991, 192.0)));
        assert_eq!(visible_line_hit(&geometry, 216.0), None);
    }

    #[test]
    fn local_text_y_prefers_visible_title_and_preview_insets() {
        let local_y = local_text_y_for_hit(190.0, 40_000_190.0, Some(120.0), 39_998_000.0, 30.0);
        assert_eq!(local_y, 40.0);
    }

    #[test]
    fn local_text_y_selects_soft_wrap_visual_row_from_screen_coordinates() {
        let local_y = local_text_y_for_hit(178.0, 39_000_178.0, Some(100.0), 38_990_000.0, 6.0);
        assert_eq!((local_y / 24.0) as usize, 3);
    }

    #[test]
    fn screen_hit_stays_on_tall_row_until_its_actual_bottom() {
        let wrapped = test_wrapped_line(&[24.0, 60.0, 24.0]);
        for (screen_y, expected_row) in [(178.0, 1), (189.9, 1), (190.0, 2)] {
            let local_y = local_text_y_for_hit(
                screen_y,
                39_000_000.0 + screen_y,
                Some(100.0),
                38_990_000.0,
                6.0,
            );
            assert_eq!(wrapped.row_for_y(px(local_y), px(24.0)), expected_row);
        }
    }

    #[test]
    fn local_text_y_falls_back_to_document_geometry_outside_visible_rows() {
        assert_eq!(local_text_y_for_hit(10.0, 530.0, None, 500.0, 99.0), 30.0);
    }

    #[test]
    fn display_index_mapping_snaps_cjk_and_emoji_to_utf8_boundaries() {
        let text = "A中🙂B";
        let display_to_buffer = (0..text.len()).collect::<Vec<_>>();

        assert_eq!(display_index_to_buffer_col(text, &display_to_buffer, 3), 1);
        assert_eq!(display_index_to_buffer_col(text, &display_to_buffer, 7), 4);
        assert_eq!(
            display_index_to_buffer_col(text, &display_to_buffer, text.len()),
            text.len()
        );
        for display_index in 0..=text.len() {
            let buffer_col = display_index_to_buffer_col(text, &display_to_buffer, display_index);
            assert!(text.is_char_boundary(buffer_col));
        }
    }
}
