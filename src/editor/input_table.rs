use crate::markdown::LineDecoration;

use super::*;

fn nearest_boundary_index(boundaries: &[f32], coordinate: f32) -> Option<usize> {
    boundaries
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            (*left - coordinate)
                .abs()
                .partial_cmp(&(*right - coordinate).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index)
}

fn visible_table_line_indices(
    decorations: &[LineDecoration],
    table_start: usize,
) -> Option<Vec<usize>> {
    let header_info = decorations.get(table_start)?.table_info.as_ref()?;
    let mut visible_lines = Vec::new();
    for line_idx in table_start..header_info.table_end_line {
        let table_info = decorations
            .get(line_idx)
            .and_then(|decoration| decoration.table_info.as_ref())?;
        if !table_info.is_separator {
            visible_lines.push(line_idx);
        }
    }
    Some(visible_lines)
}

fn table_boundary_offsets(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    boundary: TableBoundaryCaret,
) -> Option<(usize, usize)> {
    let table_info = decorations
        .get(boundary.table_start_line)?
        .table_info
        .as_ref()?;
    if table_info.is_front_matter || table_info.table_start_line != boundary.table_start_line {
        return None;
    }
    match boundary.side {
        TableBoundarySide::Above => {
            let offset = buffer.line_start(table_info.table_start_line);
            Some((offset, offset))
        }
        TableBoundarySide::Below => {
            let offset = buffer.line_end(table_info.table_end_line.saturating_sub(1));
            Some((offset, offset + 1))
        }
    }
}

fn table_boundary_for_cursor(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    cursor_offset: usize,
) -> Option<TableBoundaryCaret> {
    let line = buffer.offset_to_line_col(cursor_offset.min(buffer.len())).0;
    let table_info = decorations.get(line)?.table_info.as_ref()?;
    if table_info.is_front_matter {
        return None;
    }
    let col = cursor_offset.saturating_sub(buffer.line_start(line));
    if table_info
        .cell_ranges
        .iter()
        .any(|range| (range.start..=range.end).contains(&col))
    {
        return None;
    }
    if line == table_info.table_start_line && cursor_offset == buffer.line_start(line) {
        return Some(TableBoundaryCaret {
            table_start_line: table_info.table_start_line,
            side: TableBoundarySide::Above,
        });
    }
    if line == table_info.table_end_line.saturating_sub(1) && cursor_offset == buffer.line_end(line)
    {
        return Some(TableBoundaryCaret {
            table_start_line: table_info.table_start_line,
            side: TableBoundarySide::Below,
        });
    }
    None
}

impl EditorView {
    pub(crate) fn table_boundary_block_range(&self) -> Option<std::ops::Range<usize>> {
        let boundary = self.table_boundary_caret?;
        let table_info = self
            .decorations
            .get(boundary.table_start_line)?
            .table_info
            .as_ref()?;
        if table_info.is_front_matter {
            return None;
        }
        Some(
            self.buffer.line_start(table_info.table_start_line)
                ..self
                    .buffer
                    .line_end(table_info.table_end_line.saturating_sub(1)),
        )
    }

    pub(crate) fn sync_table_boundary_caret_after_reparse(&mut self) {
        if self.cursor.has_selection() || self.has_extra_cursors() {
            self.table_boundary_caret = None;
            return;
        }
        self.table_boundary_caret =
            table_boundary_for_cursor(&self.buffer, &self.decorations, self.cursor.offset);
    }

    fn table_boundary_offset(&self, boundary: TableBoundaryCaret) -> Option<usize> {
        table_boundary_offsets(&self.buffer, &self.decorations, boundary).map(|(offset, _)| offset)
    }

    pub(crate) fn focus_table_boundary(&mut self, boundary: TableBoundaryCaret) -> bool {
        let Some(offset) = self.table_boundary_offset(boundary) else {
            self.table_boundary_caret = None;
            return false;
        };
        self.cursor.move_to(offset, &self.buffer);
        self.table_boundary_caret = Some(boundary);
        self.reset_cursor_visual_targeting();
        true
    }

    fn move_to_table_edge_cell(&mut self, table_start: usize, first: bool) -> bool {
        let Some(rows) = visible_table_line_indices(&self.decorations, table_start) else {
            return false;
        };
        let edge_line = if first { rows.first() } else { rows.last() };
        let Some(&line) = edge_line else {
            return false;
        };
        let Some(table_info) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
        else {
            return false;
        };
        let edge_range = if first {
            table_info.cell_ranges.first()
        } else {
            table_info.cell_ranges.last()
        };
        let Some(range) = edge_range else {
            return false;
        };
        let col = if first { range.start } else { range.end };
        self.cursor
            .move_to(self.buffer.line_start(line) + col, &self.buffer);
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        true
    }

    fn move_to_table_cell_offset(
        &mut self,
        source_line: usize,
        column: usize,
        offset_in_cell: usize,
    ) -> bool {
        let Some(range) = self
            .decorations
            .get(source_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .and_then(|table_info| table_info.cell_ranges.get(column))
        else {
            return false;
        };
        let offset =
            self.buffer.line_start(source_line) + (range.start + offset_in_cell).min(range.end);
        self.cursor.move_to(offset, &self.buffer);
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        true
    }

    /// Moves through only visible table positions in row-major order.
    pub(crate) fn move_table_cursor_horizontal(&mut self, forward: bool) -> bool {
        if let Some(boundary) = self.table_boundary_caret {
            return match (boundary.side, forward) {
                (TableBoundarySide::Above, true) => {
                    self.move_to_table_edge_cell(boundary.table_start_line, true)
                }
                (TableBoundarySide::Below, false) => {
                    self.move_to_table_edge_cell(boundary.table_start_line, false)
                }
                (TableBoundarySide::Above, false) => {
                    if boundary.table_start_line == 0 {
                        return true;
                    }
                    let previous_line = boundary.table_start_line - 1;
                    let offset = self.buffer.line_end(previous_line);
                    self.cursor.move_to(offset, &self.buffer);
                    self.table_boundary_caret = None;
                    self.reset_cursor_visual_targeting();
                    true
                }
                (TableBoundarySide::Below, true) => {
                    let Some(table_info) = self
                        .decorations
                        .get(boundary.table_start_line)
                        .and_then(|decoration| decoration.table_info.as_ref())
                    else {
                        self.table_boundary_caret = None;
                        return false;
                    };
                    if table_info.table_end_line >= self.buffer.line_count() {
                        return true;
                    }
                    let offset = self.buffer.line_start(table_info.table_end_line);
                    self.cursor.move_to(offset, &self.buffer);
                    self.table_boundary_caret = None;
                    self.reset_cursor_visual_targeting();
                    true
                }
            };
        }

        let line = self.cursor_line();
        if forward && self.cursor.offset == self.buffer.line_end(line) {
            let next_line = line + 1;
            if self.decorations.get(next_line).is_some_and(|decoration| {
                decoration.table_info.as_ref().is_some_and(|table_info| {
                    !table_info.is_front_matter && table_info.table_start_line == next_line
                })
            }) {
                return self.focus_table_boundary(TableBoundaryCaret {
                    table_start_line: next_line,
                    side: TableBoundarySide::Above,
                });
            }
        }
        if !forward && self.cursor.offset == self.buffer.line_start(line) && line > 0 {
            let previous_line = line - 1;
            if let Some(table_start_line) = self
                .decorations
                .get(previous_line)
                .and_then(|decoration| decoration.table_info.as_ref())
                .filter(|table_info| {
                    !table_info.is_front_matter && table_info.table_end_line == line
                })
                .map(|table_info| table_info.table_start_line)
            {
                return self.focus_table_boundary(TableBoundaryCaret {
                    table_start_line,
                    side: TableBoundarySide::Below,
                });
            }
        }
        let Some(table_info) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .filter(|table_info| !table_info.is_front_matter)
        else {
            return false;
        };
        let table_start = table_info.table_start_line;
        if table_info.is_separator {
            let Some(rows) = visible_table_line_indices(&self.decorations, table_start) else {
                return false;
            };
            if forward {
                return rows
                    .iter()
                    .copied()
                    .find(|candidate| *candidate > line)
                    .is_some_and(|target_line| self.move_to_table_cell_offset(target_line, 0, 0));
            }
            let Some(target_line) = rows
                .iter()
                .copied()
                .rev()
                .find(|candidate| *candidate < line)
            else {
                return false;
            };
            let Some(target_info) = self.decorations[target_line].table_info.as_ref() else {
                return false;
            };
            let Some(last_column) = target_info.cell_ranges.len().checked_sub(1) else {
                return false;
            };
            let cell_len = target_info.cell_ranges[last_column].end
                - target_info.cell_ranges[last_column].start;
            return self.move_to_table_cell_offset(target_line, last_column, cell_len);
        }
        let ranges = table_info.cell_ranges.clone();
        let col = self
            .cursor
            .offset
            .saturating_sub(self.buffer.line_start(line));
        let Some(rows) = visible_table_line_indices(&self.decorations, table_start) else {
            return false;
        };
        let Some(row) = rows.iter().position(|candidate| *candidate == line) else {
            return false;
        };

        if let Some(column) = ranges
            .iter()
            .position(|range| (range.start..=range.end).contains(&col))
        {
            let range = &ranges[column];
            if forward && col < range.end {
                let next = self.buffer.next_grapheme_boundary(self.cursor.offset);
                self.cursor.move_to(
                    next.min(self.buffer.line_start(line) + range.end),
                    &self.buffer,
                );
                return true;
            }
            if !forward && col > range.start {
                let previous = self.buffer.prev_grapheme_boundary(self.cursor.offset);
                self.cursor.move_to(
                    previous.max(self.buffer.line_start(line) + range.start),
                    &self.buffer,
                );
                return true;
            }
            if forward && column + 1 < ranges.len() {
                return self.move_to_table_cell_offset(line, column + 1, 0);
            }
            if !forward && column > 0 {
                let previous_len = ranges[column - 1].end - ranges[column - 1].start;
                return self.move_to_table_cell_offset(line, column - 1, previous_len);
            }
        } else if forward {
            if let Some(column) = ranges.iter().position(|range| range.start > col) {
                return self.move_to_table_cell_offset(line, column, 0);
            }
        } else if let Some(column) = ranges.iter().rposition(|range| range.end < col) {
            let cell_len = ranges[column].end - ranges[column].start;
            return self.move_to_table_cell_offset(line, column, cell_len);
        }

        if forward && row + 1 < rows.len() {
            return self.move_to_table_cell_offset(rows[row + 1], 0, 0);
        }
        if !forward && row > 0 {
            let previous_line = rows[row - 1];
            let Some(last_column) = self
                .decorations
                .get(previous_line)
                .and_then(|decoration| decoration.table_info.as_ref())
                .and_then(|table_info| table_info.cell_ranges.len().checked_sub(1))
            else {
                return false;
            };
            let Some(previous_info) = self.decorations[previous_line].table_info.as_ref() else {
                return false;
            };
            let cell_len = previous_info.cell_ranges[last_column].end
                - previous_info.cell_ranges[last_column].start;
            return self.move_to_table_cell_offset(previous_line, last_column, cell_len);
        }

        self.focus_table_boundary(TableBoundaryCaret {
            table_start_line: table_start,
            side: if forward {
                TableBoundarySide::Below
            } else {
                TableBoundarySide::Above
            },
        })
    }

    /// Moves vertically into or out of the two visual caret positions beside a table.
    pub(crate) fn move_table_boundary_caret(&mut self, down: bool) -> bool {
        if let Some(boundary) = self.table_boundary_caret {
            return match (boundary.side, down) {
                (TableBoundarySide::Above, true) => {
                    self.move_to_table_edge_cell(boundary.table_start_line, true)
                }
                (TableBoundarySide::Below, false) => {
                    self.move_to_table_edge_cell(boundary.table_start_line, false)
                }
                _ => {
                    self.table_boundary_caret = None;
                    false
                }
            };
        }

        let line = self.cursor_line();
        let Some(table_info) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .filter(|table_info| !table_info.is_front_matter)
        else {
            return false;
        };
        let table_start = table_info.table_start_line;
        let Some(rows) = visible_table_line_indices(&self.decorations, table_start) else {
            return false;
        };
        if table_info.is_separator {
            let target = if down {
                rows.iter().copied().find(|candidate| *candidate > line)
            } else {
                rows.iter()
                    .copied()
                    .rev()
                    .find(|candidate| *candidate < line)
            };
            return target.is_some_and(|target_line| {
                let target_column = 0;
                self.move_to_table_cell_offset(target_line, target_column, 0)
            });
        }
        let col = self
            .cursor
            .offset
            .saturating_sub(self.buffer.line_start(line));
        let Some(column) = table_info
            .cell_ranges
            .iter()
            .position(|range| (range.start..=range.end).contains(&col))
            .or_else(|| {
                table_info
                    .cell_ranges
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, range)| col.clamp(range.start, range.end).abs_diff(col))
                    .map(|(index, _)| index)
            })
        else {
            return false;
        };
        let Some(row) = rows.iter().position(|candidate| *candidate == line) else {
            return false;
        };
        let target_row = if down {
            row.checked_add(1).filter(|target| *target < rows.len())
        } else {
            row.checked_sub(1)
        };
        if let Some(target_row) = target_row {
            let current_range = &self.decorations[line]
                .table_info
                .as_ref()
                .unwrap()
                .cell_ranges[column];
            let offset_in_cell = self
                .cursor
                .offset
                .saturating_sub(self.buffer.line_start(line) + current_range.start);
            let target_line = rows[target_row];
            let target_column = self.decorations[target_line]
                .table_info
                .as_ref()
                .map(|table_info| column.min(table_info.cell_ranges.len().saturating_sub(1)))
                .unwrap_or(0);
            return self.move_to_table_cell_offset(target_line, target_column, offset_in_cell);
        }
        self.focus_table_boundary(TableBoundaryCaret {
            table_start_line: table_start,
            side: if down {
                TableBoundarySide::Below
            } else {
                TableBoundarySide::Above
            },
        })
    }

    pub(crate) fn insert_line_at_table_boundary(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(boundary) = self.table_boundary_caret else {
            return false;
        };
        let Some((offset, cursor_offset)) =
            table_boundary_offsets(&self.buffer, &self.decorations, boundary)
        else {
            self.table_boundary_caret = None;
            return false;
        };
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&(offset..offset), "\n");
        self.buffer.insert(offset, "\n");
        self.cursor.move_to(cursor_offset, &self.buffer);
        self.table_boundary_caret = None;
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        cx.notify();
        true
    }

    pub(super) fn table_col_insert_at_x(&self, table_start_line: usize, x: f32) -> Option<usize> {
        let mut header_cells: Vec<_> = self
            .table_click_areas
            .iter()
            .filter(|(_, tsl, visual_row, _)| *tsl == table_start_line && *visual_row == 0)
            .map(|(bounds, _, _, col)| (*col, *bounds))
            .collect();
        header_cells.sort_by_key(|(col, _)| *col);
        if header_cells.is_empty() {
            return None;
        }

        let mut boundaries = Vec::with_capacity(header_cells.len() + 1);
        boundaries.push(f32::from(header_cells[0].1.origin.x));
        for (_, bounds) in &header_cells {
            boundaries.push(f32::from(bounds.origin.x) + f32::from(bounds.size.width));
        }

        nearest_boundary_index(&boundaries, x)
    }

    pub(super) fn table_row_insert_at_y(&self, table_start_line: usize, y: f32) -> Option<usize> {
        let mut rows: Vec<_> = self
            .table_click_areas
            .iter()
            .filter(|(_, tsl, _, col)| *tsl == table_start_line && *col == 0)
            .map(|(bounds, _, visual_row, _)| (*visual_row, *bounds))
            .collect();
        rows.sort_by_key(|(visual_row, _)| *visual_row);
        if rows.is_empty() {
            return None;
        }

        let mut boundaries = Vec::with_capacity(rows.len() + 1);
        boundaries.push(f32::from(rows[0].1.origin.y));
        for (_, bounds) in &rows {
            boundaries.push(f32::from(bounds.origin.y) + f32::from(bounds.size.height));
        }

        nearest_boundary_index(&boundaries, y)
    }

    pub(super) fn table_source_line_for_visual_row(
        &self,
        table_start: usize,
        visual_row: usize,
    ) -> Option<usize> {
        visible_table_line_indices(&self.decorations, table_start)?
            .get(visual_row)
            .copied()
    }

    pub(super) fn table_visual_row_for_source_line(
        &self,
        table_start: usize,
        source_line: usize,
    ) -> Option<usize> {
        visible_table_line_indices(&self.decorations, table_start)?
            .iter()
            .position(|line_idx| *line_idx == source_line)
    }

    /// `(table_start_line, source_line, column)` of the cell holding the caret,
    /// or `None` when the caret is outside a (non front matter) table body.
    pub(crate) fn table_cursor_cell(&self) -> Option<(usize, usize, usize)> {
        let line = self.cursor_line();
        let table_info = self.decorations.get(line)?.table_info.as_ref()?;
        if table_info.is_separator || table_info.is_front_matter {
            return None;
        }
        let col = self.cursor.offset - self.buffer.line_start(line);
        let index = table_info
            .cell_ranges
            .iter()
            .position(|range| (range.start..=range.end).contains(&col))?;
        Some((table_info.table_start_line, line, index))
    }

    pub(crate) fn table_cursor_cell_range(&self) -> Option<std::ops::Range<usize>> {
        let (_, line, column) = self.table_cursor_cell()?;
        let range = self.decorations[line]
            .table_info
            .as_ref()?
            .cell_ranges
            .get(column)?;
        let line_start = self.buffer.line_start(line);
        Some(line_start + range.start..line_start + range.end)
    }

    pub(crate) fn move_table_cursor_to_cell_edge(&mut self, start: bool) -> bool {
        let Some(range) = self.table_cursor_cell_range() else {
            return false;
        };
        self.cursor
            .move_to(if start { range.start } else { range.end }, &self.buffer);
        self.reset_cursor_visual_targeting();
        true
    }

    /// Selects the whole text of `column` on `source_line`.
    fn select_table_cell(&mut self, source_line: usize, column: usize) -> bool {
        let Some(range) = self
            .decorations
            .get(source_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .and_then(|table_info| table_info.cell_ranges.get(column).cloned())
        else {
            return false;
        };
        let line_start = self.buffer.line_start(source_line);
        self.cursor.anchor = line_start + range.start;
        self.cursor.offset = line_start + range.end;
        self.reset_cursor_visual_targeting();
        true
    }

    /// Tab / Shift+Tab: select the next / previous cell, wrapping across rows.
    /// Tab past the last cell appends a row, matching Typora and Obsidian.
    pub(crate) fn table_move_to_adjacent_cell(
        &mut self,
        forward: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((table_start, line, column)) = self.table_cursor_cell() else {
            return false;
        };
        let Some(rows) = visible_table_line_indices(&self.decorations, table_start) else {
            return false;
        };
        let Some(row) = rows.iter().position(|candidate| *candidate == line) else {
            return false;
        };
        let column_count = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .map(|table_info| table_info.cell_ranges.len())
            .unwrap_or(0);
        if column_count == 0 {
            return false;
        }

        let (target_row, target_column) = if forward {
            if column + 1 < column_count {
                (row, column + 1)
            } else if row + 1 < rows.len() {
                (row + 1, 0)
            } else {
                self.table_insert_row(table_start, row, false, cx);
                (row + 1, 0)
            }
        } else if column > 0 {
            (row, column - 1)
        } else if row > 0 {
            (row - 1, column_count - 1)
        } else {
            (row, column)
        };

        let Some(target_line) = self.table_source_line_for_visual_row(table_start, target_row)
        else {
            return false;
        };
        if !self.select_table_cell(target_line, target_column) {
            return false;
        }
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
        cx.notify();
        true
    }

    /// Enter inside a table selects the cell directly below, appending a row
    /// when the caret is already on the last one.
    pub(crate) fn table_move_to_cell_below(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((table_start, line, column)) = self.table_cursor_cell() else {
            return false;
        };
        let Some(rows) = visible_table_line_indices(&self.decorations, table_start) else {
            return false;
        };
        let Some(row) = rows.iter().position(|candidate| *candidate == line) else {
            return false;
        };
        if row + 1 >= rows.len() {
            self.table_insert_row(table_start, row, false, cx);
        }
        let Some(target_line) = self.table_source_line_for_visual_row(table_start, row + 1) else {
            return false;
        };
        if !self.select_table_cell(target_line, column) {
            return false;
        }
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        nearest_boundary_index, table_boundary_for_cursor, table_boundary_offsets,
        visible_table_line_indices,
    };
    use crate::buffer::Buffer;
    use crate::editor::{TableBoundaryCaret, TableBoundarySide};
    use crate::markdown::{LineDecoration, TableLineInfo};

    fn table_line_info(is_separator: bool, table_end_line: usize) -> TableLineInfo {
        TableLineInfo {
            cells: vec![],
            cell_ranges: vec![],
            column_alignments: vec![],
            is_header: false,
            is_separator,
            is_front_matter: false,
            table_start_line: 0,
            table_end_line,
            column_widths: vec![],
        }
    }

    #[test]
    fn nearest_boundary_picks_closest_index() {
        assert_eq!(nearest_boundary_index(&[10.0, 30.0, 70.0], 24.0), Some(1));
        assert_eq!(nearest_boundary_index(&[10.0, 30.0, 70.0], 65.0), Some(2));
        assert_eq!(nearest_boundary_index(&[], 65.0), None);
    }

    #[test]
    fn visible_table_lines_skip_separator_rows() {
        let mut decorations = vec![
            LineDecoration::default(),
            LineDecoration::default(),
            LineDecoration::default(),
            LineDecoration::default(),
        ];
        decorations[0].table_info = Some(table_line_info(false, 4));
        decorations[1].table_info = Some(table_line_info(true, 4));
        decorations[2].table_info = Some(table_line_info(false, 4));
        decorations[3].table_info = Some(table_line_info(false, 4));

        assert_eq!(
            visible_table_line_indices(&decorations, 0),
            Some(vec![0, 2, 3])
        );
    }

    #[test]
    fn table_boundary_offsets_create_editable_lines_outside_the_table() {
        let text = "| A |\n| --- |\n| B |";
        let buffer = Buffer::from_text(text);
        let decorations = crate::markdown::parse_decorations(text);

        let above = table_boundary_offsets(
            &buffer,
            &decorations,
            TableBoundaryCaret {
                table_start_line: 0,
                side: TableBoundarySide::Above,
            },
        );
        let below = table_boundary_offsets(
            &buffer,
            &decorations,
            TableBoundaryCaret {
                table_start_line: 0,
                side: TableBoundarySide::Below,
            },
        );

        assert_eq!(above, Some((0, 0)));
        assert_eq!(below, Some((text.len(), text.len() + 1)));
    }

    #[test]
    fn restored_cursor_offsets_recover_table_boundary_carets() {
        let text = "before\n| A |\n| --- |\n| B |\nafter";
        let buffer = Buffer::from_text(text);
        let decorations = crate::markdown::parse_decorations(text);
        let table_start = 1;
        let table_end_line = 4;

        assert_eq!(
            table_boundary_for_cursor(&buffer, &decorations, buffer.line_start(table_start),),
            Some(TableBoundaryCaret {
                table_start_line: table_start,
                side: TableBoundarySide::Above,
            })
        );
        assert_eq!(
            table_boundary_for_cursor(&buffer, &decorations, buffer.line_end(table_end_line - 1),),
            Some(TableBoundaryCaret {
                table_start_line: table_start,
                side: TableBoundarySide::Below,
            })
        );
        assert_eq!(
            table_boundary_for_cursor(&buffer, &decorations, buffer.line_start(0)),
            None
        );
    }

    #[test]
    fn visible_edge_cells_are_not_mistaken_for_restored_boundaries() {
        let text = "A | B\n--- | ---\nC | D";
        let buffer = Buffer::from_text(text);
        let decorations = crate::markdown::parse_decorations(text);

        assert_eq!(
            table_boundary_for_cursor(&buffer, &decorations, buffer.line_start(0)),
            None
        );
        assert_eq!(
            table_boundary_for_cursor(&buffer, &decorations, buffer.line_end(2)),
            None
        );
    }
}
