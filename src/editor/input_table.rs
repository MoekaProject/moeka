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

impl EditorView {
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
            .position(|range| (range.start..=range.end).contains(&col))
            .or_else(|| {
                table_info
                    .cell_ranges
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, range)| col.clamp(range.start, range.end).abs_diff(col))
                    .map(|(index, _)| index)
            })?;
        Some((table_info.table_start_line, line, index))
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
    use super::{nearest_boundary_index, visible_table_line_indices};
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
}
