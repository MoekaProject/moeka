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
        self.refresh_front_matter_input_geometry(window, self.front_matter_input_viewport_bounds());
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
        if self.active_tab_is_whiteboard() {
            self.whiteboard_on_mouse_down(event, window, cx);
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
                if event.click_count >= 2 && !self.active_tab_is_read_only() {
                    self.toggle_generated_preview_code_view(line_idx);
                    self.clear_wrap_boundary_affinity();
                    self.is_selecting = false;
                    self.selection_drag_position = None;
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
            if event.click_count >= 2 {
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

        if let Some(&(_, hunk_index)) = self
            .inline_diff_revert_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.revert_inline_diff_hunk(hunk_index, cx);
            return;
        }

        self.selected_handle = None;
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

        if let Some(&(_, hunk_index)) = self
            .inline_diff_gutter_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.toggle_inline_diff_hunk_expanded(hunk_index, cx);
            return;
        }

        // Read-only text selection over an expanded hunk's removed rows. Clicking
        // a phantom row places a read-only caret there (collapsing any buffer
        // selection); dragging extends a character-level selection that can be
        // copied.
        if let Some((hunk_base_start, row, offset)) =
            self.phantom_offset_at_position(event.position)
        {
            self.phantom_selection = Some(PhantomSelection {
                hunk_base_start,
                anchor_row: row,
                anchor_offset: offset,
                head_row: row,
                head_offset: offset,
            });
            self.phantom_selecting = true;
            self.cursor.anchor = self.cursor.offset;
            self.clear_extra_cursors();
            self.is_selecting = false;
            self.marked_range = None;
            self.pause_blinking(cx);
            cx.notify();
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
            if self.select_image_syntax_on_line(line_idx) {
                cx.notify();
                return;
            }
        }

        if let Some(line) = self
            .checkbox_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
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
                self.clear_phantom_selection();
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
            self.clear_phantom_selection();
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

        self.clear_phantom_selection();
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

        if self
            .preview_overlay_state()
            .is_some_and(|overlay| overlay.line_idx == line_idx && overlay.cache_key == cache_key)
        {
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

        self.line_format_toolbar_line = None;
        self.line_format_toolbar_bounds = None;
        self.preview_overlay = Some(PreviewOverlayState {
            cache_key,
            line_idx,
            view: ImageViewState::default(),
        });
        self.preview_overlay_press = None;
        self.preview_overlay_bounds = None;
        self.preview_overlay_image_bounds = None;
        true
    }

    fn toggle_generated_preview_overlay(&mut self, line_idx: usize) -> bool {
        if self
            .preview_overlay_state()
            .is_some_and(|overlay| overlay.line_idx == line_idx)
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
        self.line_format_toolbar_line = None;
        self.line_format_toolbar_bounds = None;
        self.preview_overlay = Some(PreviewOverlayState {
            cache_key: preview.cache_key.clone(),
            line_idx,
            view: ImageViewState::default(),
        });
        self.preview_overlay_press = None;
        self.preview_overlay_bounds = None;
        self.preview_overlay_image_bounds = None;
        true
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

    pub(super) fn skip_front_matter_caret_navigation(&mut self, previous: &[Cursor]) {
        self.skip_front_matter_navigation(previous, false);
    }

    pub(super) fn skip_folded_front_matter_selection_navigation(&mut self, previous: &[Cursor]) {
        self.skip_front_matter_navigation(previous, true);
    }

    fn skip_front_matter_navigation(&mut self, previous: &[Cursor], selecting: bool) {
        let range = self.collapsed_front_matter_range();
        let hidden: Vec<_> = if range.is_none() && !self.markdown_syntax_expansion_allowed() {
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
            changed |= super::fold::skip_front_matter_navigation_head(
                &self.buffer,
                range.as_ref(),
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

        if self.active_tab_is_whiteboard() {
            self.whiteboard_select_at(event.position);
            self.open_context_menu(
                ContextMenu {
                    position: event.position,
                    kind: ContextMenuKind::Whiteboard,
                },
                true,
                cx,
            );
            return;
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
                self.cursor.move_to(offset, &self.buffer);
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
                .map(|(index, _)| index)
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
                .map(|(index, _)| index)
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
        if self.active_tab_is_whiteboard() {
            self.whiteboard_on_mouse_up(event, cx);
            return;
        }
        if self.stop_code_block_horizontal_scrollbar_drag(event.position, cx) {
            return;
        }
        if self.stop_editor_scrollbar_drag(event.position, cx) {
            return;
        }

        if self.phantom_selecting {
            self.phantom_selecting = false;
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
        if self.preview_overlay.is_some() {
            self.on_preview_overlay_mouse_move(event, cx);
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
        if self.active_tab_is_whiteboard() {
            self.whiteboard_on_mouse_move(event, window, cx);
            return;
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
        // Extend a read-only phantom-row selection while dragging.
        if self.phantom_selecting {
            if event.dragging()
                && let Some(mut sel) = self.phantom_selection
                && let Some((hunk, row, offset)) = self.phantom_offset_at_position(event.position)
                && hunk == sel.hunk_base_start
                && (row != sel.head_row || offset != sel.head_offset)
            {
                sel.head_row = row;
                sel.head_offset = offset;
                self.phantom_selection = Some(sel);
                cx.notify();
            }
            return;
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
            || !self.inline_diff_gutter_areas.is_empty()
            || !self.inline_diff_hunk_areas.is_empty()
            || !self.inline_diff_revert_areas.is_empty()
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

        let hovering_checkbox_line = self
            .checkbox_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line);
        if hovering_checkbox_line != self.hovering_checkbox_line {
            self.hovering_checkbox_line = hovering_checkbox_line;
            self.hovering_checkbox = self.hovering_checkbox_line.is_some();
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
                    self.preview_overlay_state().map(|overlay| overlay.line_idx)
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

        let hovering_inline_diff_gutter = self
            .inline_diff_gutter_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, hunk_index)| *hunk_index);
        if hovering_inline_diff_gutter != self.hovering_inline_diff_gutter_hunk {
            self.hovering_inline_diff_gutter_hunk = hovering_inline_diff_gutter;
            cx.notify();
        }

        let hovering_inline_diff_hunk = self
            .inline_diff_hunk_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, hunk_index)| *hunk_index);
        if hovering_inline_diff_hunk != self.hovering_inline_diff_hunk {
            self.hovering_inline_diff_hunk = hovering_inline_diff_hunk;
            cx.notify();
        }

        let hovering_inline_diff_revert = self
            .inline_diff_revert_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, hunk_index)| *hunk_index);
        if hovering_inline_diff_revert != self.hovering_inline_diff_revert_hunk {
            self.hovering_inline_diff_revert_hunk = hovering_inline_diff_revert;
            cx.notify();
        }

        let gutter_styles = &self.styles.gutter;
        let fold_hit_padding = self.scaled(gutter_styles.gutter_fold_hit_padding_px);
        let pane_left = self.last_editor_left;
        let hover_x = f32::from(event.position.x);
        let hovering_gutter_fold_area =
            hover_x >= pane_left && hover_x <= self.last_content_left + fold_hit_padding;
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
                self.styles.selection.selection_diff_added_inline_bg,
                self.styles.selection.selection_diff_removed_inline_bg,
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
        if self.active_tab_is_diff() {
            return None;
        }
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
        if self.table_cell_for_offset(offset).is_some() && !self.active_tab_is_diff() {
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
        let slot_size = self.scaled(super::element::LINE_FORMAT_ICON_SIZE_PX);
        let (column_inset, _) =
            super::element::front_matter_content_geometry(true, 0.0, 1.0, self.scaled(1.0));
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
                let icon_size = self.scaled(self.styles.gutter.gutter_fold_icon_size_px);
                let hit = super::element::front_matter_fold_hit_bounds(
                    left - icon_size - self.scaled(super::element::LINE_FORMAT_CONTENT_GAP_PX),
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
                self.scaled(1.0),
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
            self.scaled(1.0),
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

        self.append_search_reveal_range(line_idx, &mut active_ranges);
        if group_plan.is_some_and(|plan| plan.active) {
            active_ranges.push(0..line_content.len());
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
                || self.search_reveals_code_fence_group(line_idx)
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
            self.styles.selection.selection_diff_added_inline_bg,
            self.styles.selection.selection_diff_removed_inline_bg,
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
        let wrap_width_param = if self.editor_wraps_to_available_width() {
            Some(px(effective_wrap_width))
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

        if self.active_tab_is_whiteboard() {
            self.whiteboard_on_scroll(event, cx);
            return;
        }
        if self.preview_overlay.is_some() {
            let delta = event.delta.pixel_delta(px(40.0));
            let dx = f32::from(delta.x);
            let dy = f32::from(delta.y);
            let zoom_gesture = event.modifiers.platform || event.modifiers.control;

            if zoom_gesture {
                if dy.abs() < 0.01 {
                    return;
                }
                let zoom_ratio = 1.0 + (dy.abs() / 300.0).clamp(0.05, 0.35);
                let factor = if dy < 0.0 {
                    zoom_ratio
                } else {
                    1.0 / zoom_ratio
                };
                self.zoom_preview_overlay_view(factor);
                cx.notify();
                return;
            }

            if dx == 0.0 && dy == 0.0 {
                return;
            }
            if let Some(overlay) = self.preview_overlay_state_mut() {
                overlay.view.pan_x -= dx;
                overlay.view.pan_y -= dy;
            }
            self.clamp_preview_overlay_view();
            cx.notify();
            return;
        }

        if self.active_tab_is_image() {
            let delta = event.delta.pixel_delta(px(40.0));
            let dx = f32::from(delta.x);
            let dy = f32::from(delta.y);
            let zoom_gesture = event.modifiers.platform || event.modifiers.control;

            if zoom_gesture {
                if dy.abs() < 0.01 {
                    return;
                }
                let zoom_ratio = 1.0 + (dy.abs() / 300.0).clamp(0.05, 0.35);
                if let Some(image_view) = self.active_image_view_state_mut() {
                    if dy < 0.0 {
                        image_view.zoom_factor *= zoom_ratio;
                    } else {
                        image_view.zoom_factor /= zoom_ratio;
                    }
                }
                self.clamp_active_image_view();
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
                + self.image_preview_extra_height_for_line(line);
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
            let geometry = front_matter_content_geometry(has_type_column, 100.0, 300.0, 1.0);
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
        let (content_x, wrap_width) = front_matter_content_geometry(true, 100.0, 300.0, 1.0);
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
