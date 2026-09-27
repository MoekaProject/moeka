use std::time::{Duration, Instant};

use gpui::{Context, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window};

use super::{EditorView, TableDragState};

pub(super) type HandleKey = (usize, bool, usize);

pub(super) fn top_padding(
    decoration: Option<&crate::markdown::LineDecoration>,
    diff: bool,
    thickness: f32,
) -> f32 {
    if !diff
        && decoration
            .and_then(|decoration| decoration.table_info.as_ref())
            .is_some_and(|table| table.is_header && !table.is_front_matter)
    {
        thickness
    } else {
        0.0
    }
}

pub(super) struct HandlePress {
    target: HandleKey,
    start: Point<Pixels>,
    shift: bool,
    secondary: bool,
    started: Instant,
    moved: bool,
    dragging: bool,
    selection_applied: bool,
}

impl HandlePress {
    fn update(&mut self, position: Point<Pixels>, threshold: f32, now: Instant) {
        let delta = position - self.start;
        self.moved |= f32::from(delta.x).hypot(f32::from(delta.y)) >= threshold;
        self.dragging |=
            self.moved && now.saturating_duration_since(self.started) >= Duration::from_millis(240);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TableBorderSelection {
    pub table_start_line: usize,
    pub is_column: bool,
    pub anchor: usize,
    pub indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TableCellSelection {
    pub table_start_line: usize,
    pub rows: std::ops::RangeInclusive<usize>,
    pub columns: std::ops::RangeInclusive<usize>,
    pub focus: (usize, usize),
}

impl TableCellSelection {
    pub fn between(table: usize, first: (usize, usize), last: (usize, usize)) -> Self {
        Self {
            table_start_line: table,
            rows: first.0.min(last.0)..=first.0.max(last.0),
            columns: first.1.min(last.1)..=first.1.max(last.1),
            focus: last,
        }
    }

    pub fn contains(&self, table: usize, row: usize, column: usize) -> bool {
        self.table_start_line == table && self.rows.contains(&row) && self.columns.contains(&column)
    }

    pub fn whole_border(
        &self,
        row_count: usize,
        column_count: usize,
    ) -> Option<TableBorderSelection> {
        let (is_column, indices) =
            if self.columns.start() == &0 && *self.columns.end() + 1 == column_count {
                (false, self.rows.clone().collect::<Vec<_>>())
            } else if self.rows.start() == &0 && *self.rows.end() == row_count {
                (true, self.columns.clone().collect::<Vec<_>>())
            } else {
                return None;
            };
        Some(TableBorderSelection {
            table_start_line: self.table_start_line,
            is_column,
            anchor: indices[0],
            indices,
        })
    }
}

fn selection_covers_table(indices: &[usize], count: usize) -> bool {
    (0..count).all(|index| indices.contains(&index))
}

fn insertion_after_selection(indices: &[usize]) -> Option<usize> {
    indices.iter().max().map(|index| index + 1)
}

impl TableBorderSelection {
    pub fn contains(&self, index: usize) -> bool {
        self.indices.contains(&index)
    }

    fn for_press(
        previous: Option<Self>,
        (table, is_column, index): HandleKey,
        shift: bool,
        secondary: bool,
    ) -> Option<Self> {
        if let Some(previous) = previous.filter(|selection| {
            selection.table_start_line == table && selection.is_column == is_column
        }) {
            if secondary {
                let mut next = previous;
                if let Some(position) = next.indices.iter().position(|item| *item == index) {
                    next.indices.remove(position);
                } else {
                    next.indices.push(index);
                    next.indices.sort_unstable();
                }
                return (!next.indices.is_empty()).then_some(next);
            }
            if previous.contains(index) && !shift {
                return Some(previous);
            }
            if shift {
                let mut next = previous.clone();
                next.indices = (previous.anchor.min(index)..=previous.anchor.max(index)).collect();
                return Some(next);
            }
        }
        Some(Self {
            table_start_line: table,
            is_column,
            anchor: index,
            indices: vec![index],
        })
    }

    pub fn click(
        previous: Option<Self>,
        table: usize,
        is_column: bool,
        index: usize,
        shift: bool,
        secondary: bool,
    ) -> Option<Self> {
        if (shift || secondary)
            && previous.as_ref().is_some_and(|selection| {
                selection.table_start_line == table && selection.is_column == is_column
            })
        {
            Self::for_press(previous, (table, is_column, index), shift, secondary)
        } else {
            Some(Self {
                table_start_line: table,
                is_column,
                anchor: index,
                indices: vec![index],
            })
        }
    }
}

#[derive(Default)]
pub(crate) struct TableHandles {
    press: Option<HandlePress>,
    pub selection: Option<TableBorderSelection>,
}

impl EditorView {
    pub(super) fn ensure_table_cell_context_selection(
        &mut self,
        table: usize,
        row: usize,
        column: usize,
    ) {
        if self
            .table_cell_selection
            .as_ref()
            .is_none_or(|selection| !selection.contains(table, row, column))
        {
            self.table_cell_selection = Some(TableCellSelection::between(
                table,
                (row, column),
                (row, column),
            ));
        }
    }

    pub(super) fn reset_table_handles(&mut self) {
        self.table_handles = TableHandles::default();
        self.table_cell_selection = None;
        self.table_drag = None;
        self.hovering_handle = None;
        self.selected_handle = None;
        self.table_column_border_segments.clear();
        self.table_row_border_segments.clear();
    }

    pub(super) fn clear_table_handle_hover(&mut self, cx: &mut Context<Self>) {
        let changed = self.hovering_handle.take().is_some();
        if changed {
            cx.notify();
        }
    }

    pub(super) fn cancel_table_handle_press(&mut self, cx: &mut Context<Self>) -> bool {
        if self.table_handles.press.take().is_some() {
            self.table_drag = None;
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(super) fn begin_table_handle_press(
        &mut self,
        target: HandleKey,
        position: Point<Pixels>,
        shift: bool,
        secondary: bool,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() || self.context_menu.is_some() {
            return;
        }
        self.marked_range = None;
        self.table_cell_selection = None;
        self.clear_extra_cursors();
        self.is_selecting = false;
        self.table_drag = None;
        self.table_handles.press = Some(HandlePress {
            target,
            start: position,
            shift,
            secondary,
            started: Instant::now(),
            moved: false,
            dragging: false,
            selection_applied: false,
        });
        cx.notify();
    }

    fn set_table_border_drag_position(&mut self, position: Point<Pixels>) {
        let Some(press) = self
            .table_handles
            .press
            .as_ref()
            .filter(|press| press.dragging)
        else {
            return;
        };
        let (table_start_line, is_column, index) = press.target;
        let selection = self
            .table_handles
            .selection
            .clone()
            .filter(|selection| {
                selection.table_start_line == table_start_line && selection.is_column == is_column
            })
            .unwrap_or(TableBorderSelection {
                table_start_line,
                is_column,
                anchor: index,
                indices: vec![index],
            });
        self.table_drag = if is_column {
            Some(TableDragState::DraggingColumn {
                table_start_line,
                source_indices: selection.indices.clone(),
                current_x: position.x.into(),
            })
        } else {
            Some(TableDragState::DraggingRow {
                table_start_line,
                source_indices: selection.indices.clone(),
                current_y: position.y.into(),
            })
        };
    }

    pub(super) fn update_table_handle_press(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.table_handles.press.is_none() {
            return false;
        }
        if event.pressed_button != Some(MouseButton::Left) || self.active_tab_is_read_only() {
            self.cancel_table_handle_press(cx);
            return true;
        }
        let threshold = self.scaled(self.styles.table.table_border_drag_threshold_px);
        self.table_handles.press.as_mut().unwrap().update(
            event.position,
            threshold,
            Instant::now(),
        );
        self.apply_border_press_selection_if_dragging();
        self.set_table_border_drag_position(event.position);
        cx.notify();
        true
    }

    pub(super) fn finish_table_handle_press(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.table_handles.press.is_none() {
            return false;
        }
        let threshold = self.scaled(self.styles.table.table_border_drag_threshold_px);
        self.table_handles.press.as_mut().unwrap().update(
            event.position,
            threshold,
            Instant::now(),
        );
        self.apply_border_press_selection_if_dragging();
        self.set_table_border_drag_position(event.position);
        let press = self.table_handles.press.take().unwrap();
        let viewport = gpui::Bounds::new(
            self.content_origin,
            gpui::size(
                gpui::px(self.last_viewport_width),
                gpui::px(self.last_viewport_height),
            ),
        );
        if self.active_tab_is_read_only() || !viewport.contains(&event.position) {
            self.table_drag = None;
        } else if press.dragging {
            self.commit_table_border_drag(press.target, event.position, cx);
        } else {
            self.select_table_border(press.target, press.shift, press.secondary);
        }
        self.table_drag = None;
        cx.notify();
        true
    }

    fn select_table_border(
        &mut self,
        (table, is_column, index): HandleKey,
        shift: bool,
        secondary: bool,
    ) {
        self.table_handles.selection = TableBorderSelection::click(
            self.table_handles.selection.clone(),
            table,
            is_column,
            index,
            shift,
            secondary,
        );
        self.cursor.anchor = self.cursor.offset;
    }

    fn apply_border_press_selection_if_dragging(&mut self) {
        let Some(press) = self.table_handles.press.as_ref() else {
            return;
        };
        if !press.dragging || press.selection_applied {
            return;
        }
        let (table, is_column, index) = press.target;
        let shift = press.shift;
        let secondary = press.secondary;
        self.table_handles.selection = TableBorderSelection::for_press(
            self.table_handles.selection.clone(),
            (table, is_column, index),
            shift,
            secondary,
        );
        self.table_handles.press.as_mut().unwrap().selection_applied = true;
    }

    fn commit_table_border_drag(
        &mut self,
        (table, is_column, index): HandleKey,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let selection = self
            .table_handles
            .selection
            .clone()
            .filter(|selection| {
                selection.table_start_line == table && selection.is_column == is_column
            })
            .unwrap_or(TableBorderSelection {
                table_start_line: table,
                is_column,
                anchor: index,
                indices: vec![index],
            });
        let Some((start, end, lines)) = self.table_lines_for_block(table) else {
            return;
        };
        let insert_at = if is_column {
            self.table_col_insert_at_x(table, position.x.into())
        } else {
            self.table_row_insert_at_y(table, position.y.into())
        };
        let Some(insert_at) = insert_at else {
            return;
        };
        let new_lines = if is_column {
            crate::table_ops::move_selected_columns(&lines, &selection.indices, insert_at)
        } else if self.block_is_front_matter(table) {
            return;
        } else {
            crate::table_ops::move_selected_rows(&lines, &selection.indices, insert_at)
        };
        if new_lines != lines {
            self.replace_table_block(start, end, &new_lines, cx);
            let count = selection.indices.len();
            let new_start = insert_at.saturating_sub(
                selection
                    .indices
                    .iter()
                    .filter(|selected| **selected < insert_at)
                    .count(),
            );
            self.table_handles.selection = Some(TableBorderSelection {
                table_start_line: table,
                is_column,
                anchor: new_start,
                indices: (new_start..new_start + count).collect(),
            });
        }
        cx.notify();
    }

    fn table_border_selection_data(
        &self,
    ) -> Option<(usize, bool, Vec<usize>, Vec<String>, usize, usize)> {
        let selection = self.table_handles.selection.clone()?;
        let (start, end, lines) = self.table_lines_for_block(selection.table_start_line)?;
        Some((
            selection.table_start_line,
            selection.is_column,
            selection.indices,
            lines,
            start,
            end,
        ))
    }

    pub(crate) fn copy_table_border_selection(&self) -> Option<String> {
        let (_, is_column, indices, lines, _, _) = self.table_border_selection_data()?;
        Some(crate::table_ops::selected_table_matrix_to_tsv(
            &lines, is_column, &indices,
        ))
    }

    pub(super) fn open_table_border_context_menu(
        &mut self,
        (table_start_line, is_column, index): HandleKey,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() {
            return;
        }
        let same_set = self
            .table_handles
            .selection
            .as_ref()
            .is_some_and(|selection| {
                selection.table_start_line == table_start_line
                    && selection.is_column == is_column
                    && selection.contains(index)
            });
        if !same_set {
            self.table_handles.selection = Some(TableBorderSelection {
                table_start_line,
                is_column,
                anchor: index,
                indices: vec![index],
            });
        }
        self.open_context_menu(
            super::ContextMenu {
                position,
                kind: super::ContextMenuKind::TableBorder {
                    table_start_line,
                    is_column,
                    index,
                },
            },
            true,
            cx,
        );
    }

    pub(crate) fn delete_table_border_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.active_tab_can_start_content_edit() {
            return false;
        }
        let Some((table, is_column, indices, lines, block_start, block_end)) =
            self.table_border_selection_data()
        else {
            return false;
        };
        if self.block_is_front_matter(table) && is_column {
            return false;
        }
        let model = crate::table_ops::parse_table_block(&lines);
        let selected_all = selection_covers_table(
            &indices,
            if is_column {
                model.col_count
            } else {
                model.data_rows.len() + 1
            },
        );
        if selected_all && !self.block_is_front_matter(table) {
            self.table_delete_table(table, cx);
            self.pause_blinking(cx);
            return true;
        }
        let updated = if is_column {
            crate::table_ops::delete_columns_at(&lines, &indices)
        } else {
            crate::table_ops::delete_rows_at(&lines, &indices)
        };
        self.replace_table_block(block_start, block_end, &updated, cx);
        self.table_handles.selection = None;
        self.pause_blinking(cx);
        cx.notify();
        true
    }

    pub(crate) fn cut_table_border_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.begin_content_edit(cx) {
            return false;
        }
        let Some(text) = self.copy_table_border_selection() else {
            return false;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.delete_table_border_selection(cx)
    }

    pub(crate) fn paste_table_border_selection(
        &mut self,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.paste_table_border_selection_with_mode(text, false, cx)
    }

    pub(crate) fn paste_table_border_selection_overwrite(
        &mut self,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.paste_table_border_selection_with_mode(text, true, cx)
    }

    fn paste_table_border_selection_with_mode(
        &mut self,
        text: &str,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((table, is_column, indices, lines, block_start, block_end)) =
            self.table_border_selection_data()
        else {
            return false;
        };
        if self.block_is_front_matter(table) && is_column {
            return false;
        }
        if text.is_empty() {
            return false;
        }
        let matrix = text
            .trim_end_matches(['\r', '\n'])
            .split('\n')
            .map(|line| {
                line.trim_end_matches('\r')
                    .split('\t')
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let model = crate::table_ops::parse_table_block(&lines);
        let updated = if !overwrite {
            let Some(insert_at) = insertion_after_selection(&indices) else {
                return false;
            };
            if is_column {
                crate::table_ops::insert_columns_at(&lines, insert_at, &matrix)
            } else {
                crate::table_ops::insert_rows_at(&lines, insert_at, &matrix)
            }
        } else if is_column {
            let columns = (0..matrix.first().map(Vec::len).unwrap_or(0))
                .map(|column| {
                    matrix
                        .iter()
                        .map(|row| row.get(column).cloned().unwrap_or_default())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            (matrix.len() == model.data_rows.len() + 1 && columns.len() == indices.len())
                .then(|| crate::table_ops::replace_columns_at(&lines, &indices, &columns))
                .flatten()
        } else {
            (matrix.len() == indices.len() && matrix.iter().all(|row| row.len() == model.col_count))
                .then(|| crate::table_ops::replace_rows_at(&lines, &indices, &matrix))
                .flatten()
        };
        let Some(updated) = updated else {
            self.notify_error(
                crate::i18n::table_paste_dimension_error(self.ui_language).to_string(),
                cx,
            );
            return false;
        };
        self.replace_table_block(block_start, block_end, &updated, cx);
        self.pause_blinking(cx);
        cx.notify();
        true
    }

    fn table_cell_selection_data(&self) -> Option<(TableCellSelection, Vec<String>, usize, usize)> {
        let selection = self.table_cell_selection.clone()?;
        let (start, end, lines) = self.table_lines_for_block(selection.table_start_line)?;
        Some((selection, lines, start, end))
    }

    pub(crate) fn copy_table_cell_selection(&self) -> Option<String> {
        let (selection, lines, _, _) = self.table_cell_selection_data()?;
        let model = crate::table_ops::parse_table_block(&lines);
        if let Some(border) = selection.whole_border(model.data_rows.len(), model.col_count) {
            return Some(crate::table_ops::selected_table_matrix_to_tsv(
                &lines,
                border.is_column,
                &border.indices,
            ));
        }
        Some(crate::table_ops::selected_cells_to_tsv(
            &lines,
            selection.rows,
            selection.columns,
        ))
    }

    pub(crate) fn delete_table_cell_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.active_tab_can_start_content_edit() {
            return false;
        }
        let Some((selection, lines, start, end)) = self.table_cell_selection_data() else {
            return false;
        };
        let model = crate::table_ops::parse_table_block(&lines);
        if let Some(border) = selection.whole_border(model.data_rows.len(), model.col_count) {
            self.table_handles.selection = Some(border);
            self.table_cell_selection = None;
            return self.delete_table_border_selection(cx);
        }
        let blank = selection
            .rows
            .clone()
            .map(|_| {
                selection
                    .columns
                    .clone()
                    .map(|_| String::new())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let Some(updated) = crate::table_ops::replace_selected_cells(
            &lines,
            selection.rows,
            selection.columns,
            &blank,
        ) else {
            return false;
        };
        self.replace_table_block(start, end, &updated, cx);
        self.pause_blinking(cx);
        cx.notify();
        true
    }

    pub(crate) fn cut_table_cell_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.begin_content_edit(cx) {
            return false;
        }
        let Some(text) = self.copy_table_cell_selection() else {
            return false;
        };
        if self.delete_table_cell_selection(cx) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            return true;
        }
        false
    }

    pub(crate) fn paste_table_cell_selection(
        &mut self,
        text: &str,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.begin_content_edit(cx) {
            return false;
        }
        let Some((selection, lines, start, end)) = self.table_cell_selection_data() else {
            return false;
        };
        let model = crate::table_ops::parse_table_block(&lines);
        if let Some(border) = selection.whole_border(model.data_rows.len(), model.col_count) {
            self.table_handles.selection = Some(border);
            self.table_cell_selection = None;
            return if overwrite {
                self.paste_table_border_selection_overwrite(text, cx)
            } else {
                self.paste_table_border_selection(text, cx)
            };
        }
        let matrix = text
            .trim_end_matches(['\r', '\n'])
            .split('\n')
            .map(|line| {
                line.trim_end_matches('\r')
                    .split('\t')
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let Some(updated) = crate::table_ops::replace_selected_cells(
            &lines,
            selection.rows,
            selection.columns,
            &matrix,
        ) else {
            self.notify_error(
                crate::i18n::table_paste_dimension_error(self.ui_language).to_string(),
                cx,
            );
            return false;
        };
        self.replace_table_block(start, end, &updated, cx);
        self.pause_blinking(cx);
        cx.notify();
        true
    }
}

impl TableHandles {
    pub fn needs_render(&self) -> bool {
        self.press.is_some() || self.selection.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    #[test]
    fn cell_drag_rectangles_classify_full_rows_and_columns() {
        let cells = TableCellSelection::between(4, (2, 3), (1, 1));
        assert_eq!(cells.focus, (1, 1));
        assert!(cells.contains(4, 1, 2));
        assert!(!cells.contains(4, 0, 2));
        assert!(cells.whole_border(3, 4).is_none());
        let rows = TableCellSelection::between(4, (2, 3), (1, 0))
            .whole_border(3, 4)
            .unwrap();
        assert!(!rows.is_column);
        assert_eq!(rows.indices, vec![1, 2]);
        let columns = TableCellSelection::between(4, (0, 1), (3, 2))
            .whole_border(3, 4)
            .unwrap();
        assert!(columns.is_column);
        assert_eq!(columns.indices, vec![1, 2]);
    }

    #[test]
    fn deleting_all_noncontiguous_segments_targets_the_whole_table() {
        assert!(selection_covers_table(&[2, 0, 1], 3));
        assert!(!selection_covers_table(&[2, 0], 3));
    }

    #[test]
    fn paste_insert_follows_last_selected_segment() {
        assert_eq!(insertion_after_selection(&[1, 3]), Some(4));
        assert_eq!(insertion_after_selection(&[3, 1]), Some(4));
        assert_eq!(insertion_after_selection(&[]), None);
    }

    #[test]
    fn shift_selection_expands_and_reverses_contiguous_ranges() {
        let selection = TableBorderSelection::click(None, 7, true, 2, false, false).unwrap();
        let extended =
            TableBorderSelection::click(Some(selection.clone()), 7, true, 5, true, false).unwrap();
        assert_eq!(extended.indices, vec![2, 3, 4, 5]);
        let reverse =
            TableBorderSelection::click(Some(selection.clone()), 7, true, 1, true, false).unwrap();
        assert_eq!(reverse.indices, vec![1, 2]);
        assert_eq!(
            TableBorderSelection::click(Some(selection), 7, false, 1, true, false)
                .unwrap()
                .indices,
            vec![1]
        );
    }

    #[test]
    fn secondary_click_toggles_noncontiguous_segments() {
        let first = TableBorderSelection::click(None, 4, true, 1, false, false).unwrap();
        let second =
            TableBorderSelection::click(Some(first.clone()), 4, true, 4, false, true).unwrap();
        assert_eq!(second.indices, vec![1, 4]);
        let removed = TableBorderSelection::click(Some(second), 4, true, 1, false, true).unwrap();
        assert_eq!(removed.indices, vec![4]);
        assert!(TableBorderSelection::click(Some(removed), 4, true, 4, false, true).is_none());
    }

    #[test]
    fn long_press_requires_movement_and_short_drag_remains_selection() {
        let now = Instant::now();
        let mut press = HandlePress {
            target: (1, true, 2),
            start: point(px(10.0), px(10.0)),
            shift: false,
            secondary: false,
            started: now,
            moved: false,
            dragging: false,
            selection_applied: false,
        };
        press.update(
            point(px(20.0), px(10.0)),
            4.0,
            now + Duration::from_millis(100),
        );
        assert!(press.moved);
        assert!(!press.dragging);
        press.update(
            point(px(30.0), px(10.0)),
            4.0,
            now + Duration::from_millis(240),
        );
        assert!(press.dragging);
        let mut hold = HandlePress {
            moved: false,
            dragging: false,
            ..press
        };
        hold.update(
            point(px(10.0), px(10.0)),
            4.0,
            now + Duration::from_millis(500),
        );
        assert!(!hold.dragging);
    }

    #[test]
    fn shift_border_selection_extends_a_contiguous_range_in_both_directions() {
        let first = TableBorderSelection {
            table_start_line: 8,
            is_column: false,
            anchor: 3,
            indices: vec![3],
        };
        let extended =
            TableBorderSelection::click(Some(first.clone()), 8, false, 1, true, false).unwrap();
        assert_eq!(extended.indices, vec![1, 2, 3]);
        assert_eq!(
            TableBorderSelection::click(Some(first.clone()), 8, false, 5, true, false)
                .unwrap()
                .indices,
            vec![3, 4, 5]
        );
        assert_ne!(
            TableBorderSelection {
                is_column: true,
                ..first.clone()
            }
            .is_column,
            first.is_column
        );
        assert_eq!(
            TableBorderSelection::click(Some(first.clone()), 8, false, 1, true, false).unwrap(),
            extended
        );
        assert_eq!(
            TableBorderSelection::click(Some(first.clone()), 8, true, 1, true, false)
                .unwrap()
                .anchor,
            1
        );
        assert_eq!(
            TableBorderSelection::for_press(Some(first.clone()), (8, false, 3), false, false)
                .unwrap(),
            first
        );
        assert_eq!(
            TableBorderSelection::for_press(Some(first), (8, false, 1), true, false).unwrap(),
            extended
        );
    }

    #[test]
    fn border_press_only_becomes_drag_after_hold_and_threshold() {
        let now = Instant::now();
        let mut press = HandlePress {
            target: (1, true, 2),
            start: point(px(10.0), px(10.0)),
            shift: false,
            secondary: false,
            started: now,
            moved: false,
            dragging: false,
            selection_applied: false,
        };
        press.update(
            point(px(20.0), px(10.0)),
            4.0,
            now + Duration::from_millis(100),
        );
        assert!(press.moved);
        assert!(!press.dragging);
        press.update(
            point(px(24.0), px(10.0)),
            4.0,
            now + Duration::from_millis(240),
        );
        assert!(press.dragging);
        let mut click = HandlePress {
            moved: false,
            dragging: false,
            ..press
        };
        click.update(click.start, 4.0, now + Duration::from_secs(1));
        assert!(!click.dragging);
    }
}
