use crate::markdown::{LineDecoration, TableLineInfo};

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

pub(super) fn visible_table_line_indices(
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

/// Share the renderer's source ownership, including real list indentation and
/// trailing whitespace. Formatting padding and delimiter bytes stay outside.
pub(super) fn rendered_cell_ranges(info: &TableLineInfo, source: &str) -> Vec<Range<usize>> {
    (0..info.cell_ranges.len())
        .map(|column| super::table_cell_render::content_with_range(info, source, column).1)
        .collect()
}

fn table_cell_at_offset(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    offset: usize,
) -> Option<(usize, usize, usize)> {
    let range = super::table_cell_edit::cell_range_for_selection(
        buffer,
        decorations,
        true,
        &(offset..offset),
    )?;
    let (line, _) = buffer.offset_to_line_col(offset);
    let info = decorations.get(line)?.table_info.as_ref()?;
    let line_start = buffer.line_start(line);
    let column = rendered_cell_ranges(info, buffer.line_content(line))
        .iter()
        .position(|cell| {
            line_start + cell.start == range.start && line_start + cell.end == range.end
        })?;
    Some((info.table_start_line, line, column))
}

pub(super) fn table_boundary_offsets(
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
    if super::table_cell_edit::cell_range_for_selection(
        buffer,
        decorations,
        true,
        &(cursor_offset..cursor_offset),
    )
    .is_some()
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

fn table_boundary_after_reparse(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    cursor: Cursor,
    has_extra_cursors: bool,
    previous: Option<TableBoundaryCaret>,
) -> Option<TableBoundaryCaret> {
    if cursor.has_selection() || has_extra_cursors {
        return None;
    }
    // An explicitly chosen boundary can share its source offset with a cell's
    // end. Retain that intent only while it still points at the same table edge.
    previous
        .filter(|boundary| {
            table_boundary_offsets(buffer, decorations, *boundary)
                .is_some_and(|(offset, _)| offset == cursor.offset)
        })
        .or_else(|| table_boundary_for_cursor(buffer, decorations, cursor.offset))
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

    pub(crate) fn table_visible_word_target(
        &self,
        offset: usize,
        raw_target: usize,
        forward: bool,
    ) -> Option<usize> {
        let (_, source_line, column) = self.table_cell_for_offset(offset)?;
        let table_info = self.decorations.get(source_line)?.table_info.as_ref()?;
        let ranges = rendered_cell_ranges(table_info, self.buffer.line_content(source_line));
        let local_range = ranges.get(column)?;
        let line_start = self.buffer.line_start(source_line);
        let cell_range = line_start + local_range.start..line_start + local_range.end;
        if raw_target >= cell_range.start && raw_target <= cell_range.end {
            return Some(raw_target);
        }
        self.table_visible_horizontal_target(
            if forward {
                cell_range.end
            } else {
                cell_range.start
            },
            forward,
        )
    }
    pub(crate) fn sync_table_boundary_caret_after_reparse(&mut self) {
        self.table_boundary_caret = table_boundary_after_reparse(
            &self.buffer,
            &self.decorations,
            self.cursor,
            self.has_extra_cursors(),
            self.table_boundary_caret,
        );
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
        let ranges = rendered_cell_ranges(table_info, self.buffer.line_content(line));
        let edge_range = if first { ranges.first() } else { ranges.last() };
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
            .and_then(|table_info| {
                rendered_cell_ranges(table_info, self.buffer.line_content(source_line))
                    .get(column)
                    .cloned()
            })
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
        if let Some(target) = self.table_visible_horizontal_target(self.cursor.offset, forward) {
            self.cursor.move_to(target, &self.buffer);
            self.table_boundary_caret = None;
            self.reset_cursor_visual_targeting();
            return true;
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
            let ranges = rendered_cell_ranges(target_info, self.buffer.line_content(target_line));
            let cell_len = ranges[last_column].len();
            return self.move_to_table_cell_offset(target_line, last_column, cell_len);
        }
        let ranges = rendered_cell_ranges(table_info, self.buffer.line_content(line));
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
            let ranges =
                rendered_cell_ranges(previous_info, self.buffer.line_content(previous_line));
            let cell_len = ranges[last_column].len();
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
        let ranges = rendered_cell_ranges(table_info, self.buffer.line_content(line));
        let Some(column) = ranges
            .iter()
            .position(|range| (range.start..=range.end).contains(&col))
            .or_else(|| {
                ranges
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
        self.table_cell_for_offset(self.cursor.offset)
    }

    pub(crate) fn table_cell_for_offset(&self, offset: usize) -> Option<(usize, usize, usize)> {
        table_cell_at_offset(&self.buffer, &self.decorations, offset)
    }

    pub(crate) fn table_cursor_cell_range(&self) -> Option<std::ops::Range<usize>> {
        super::table_cell_edit::cell_range_for_selection(
            &self.buffer,
            &self.decorations,
            true,
            &(self.cursor.offset..self.cursor.offset),
        )
    }

    pub(crate) fn table_visible_horizontal_target(
        &self,
        offset: usize,
        forward: bool,
    ) -> Option<usize> {
        let (line, column) = self.buffer.offset_to_line_col(offset);
        let table_info = self.decorations.get(line)?.table_info.as_ref()?;
        if table_info.is_front_matter {
            return None;
        }
        let table_start = table_info.table_start_line;
        let rows = visible_table_line_indices(&self.decorations, table_start)?;
        let row = rows.iter().position(|candidate| *candidate == line)?;
        let line_start = self.buffer.line_start(line);

        let ranges = rendered_cell_ranges(table_info, self.buffer.line_content(line));
        if let Some(cell) = ranges
            .iter()
            .position(|range| (range.start..=range.end).contains(&column))
        {
            let range = &ranges[cell];
            let absolute_range = line_start + range.start..line_start + range.end;
            if (forward && offset < absolute_range.end)
                || (!forward && offset > absolute_range.start)
            {
                return super::table_cell_interaction::cell_horizontal_target(
                    &self.buffer,
                    &self.decorations,
                    self.table_source.is_none(),
                    offset,
                    forward,
                );
            }
            if forward && cell + 1 < ranges.len() {
                return Some(line_start + ranges[cell + 1].start);
            }
            if !forward && cell > 0 {
                return Some(line_start + ranges[cell - 1].end);
            }
        }

        let target_line = if forward {
            rows.get(row + 1).copied()
        } else {
            row.checked_sub(1)
                .and_then(|index| rows.get(index).copied())
        }?;
        let target_info = self.decorations.get(target_line)?.table_info.as_ref()?;
        let ranges = rendered_cell_ranges(target_info, self.buffer.line_content(target_line));
        let range = if forward {
            ranges.first()?
        } else {
            ranges.last()?
        };
        Some(self.buffer.line_start(target_line) + if forward { range.start } else { range.end })
    }

    pub(crate) fn table_visible_selection_target(&self, anchor: usize, target: usize) -> usize {
        let (anchor_line, _) = self.buffer.offset_to_line_col(anchor);
        let Some(anchor_table) = self
            .decorations
            .get(anchor_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .filter(|table_info| !table_info.is_front_matter)
        else {
            return target;
        };
        let (target_line, target_column) = self.buffer.offset_to_line_col(target);
        if target_line < anchor_table.table_start_line || target_line >= anchor_table.table_end_line
        {
            return target;
        }
        let Some(target_info) = self
            .decorations
            .get(target_line)
            .and_then(|decoration| decoration.table_info.as_ref())
        else {
            return target;
        };
        let forward = target >= anchor;
        if target_info.is_separator {
            let Some(rows) =
                visible_table_line_indices(&self.decorations, anchor_table.table_start_line)
            else {
                return target;
            };
            let target_visible_line = if forward {
                rows.iter().copied().find(|line| *line > target_line)
            } else {
                rows.iter().copied().rev().find(|line| *line < target_line)
            };
            let Some(target_visible_line) = target_visible_line else {
                return target;
            };
            let Some(info) = self.decorations[target_visible_line].table_info.as_ref() else {
                return target;
            };
            let ranges = rendered_cell_ranges(info, self.buffer.line_content(target_visible_line));
            let Some(range) = (if forward {
                ranges.first()
            } else {
                ranges.last()
            }) else {
                return target;
            };
            return self.buffer.line_start(target_visible_line)
                + if forward { range.start } else { range.end };
        }

        let line_start = self.buffer.line_start(target_line);
        let ranges = rendered_cell_ranges(target_info, self.buffer.line_content(target_line));
        if ranges
            .iter()
            .any(|range| (range.start..=range.end).contains(&target_column))
        {
            return target;
        }
        if forward {
            ranges
                .iter()
                .find(|range| range.start > target_column)
                .or_else(|| ranges.last())
                .map_or(target, |range| line_start + range.start)
        } else {
            ranges
                .iter()
                .rev()
                .find(|range| range.end < target_column)
                .or_else(|| ranges.first())
                .map_or(target, |range| line_start + range.end)
        }
    }

    pub(super) fn table_selection_exit_offset(&self, forward: bool) -> Option<usize> {
        let (table_start, _, _) = self.table_cursor_cell()?;
        let table = self.decorations.get(table_start)?.table_info.as_ref()?;
        Some(if forward {
            if table.table_end_line < self.buffer.line_count() {
                self.buffer.line_start(table.table_end_line)
            } else {
                self.buffer.len()
            }
        } else if table_start > 0 {
            self.buffer.line_end(table_start - 1)
        } else {
            0
        })
    }

    pub(crate) fn move_table_cursors_to_cell_edge(&mut self, start: bool, select: bool) -> bool {
        let rendered_markdown = self.active_tab_is_markdown() && self.table_source.is_none();
        if !std::iter::once(&self.cursor)
            .chain(&self.extra_cursors)
            .any(|cursor| {
                super::table_cell_interaction::cell_edge_target(
                    &self.buffer,
                    &self.decorations,
                    rendered_markdown,
                    cursor.offset,
                    start,
                )
                .is_some()
            })
        {
            return false;
        }
        for cursor in std::iter::once(&mut self.cursor).chain(&mut self.extra_cursors) {
            super::table_cell_interaction::move_cursor_to_edge(
                &self.buffer,
                &self.decorations,
                rendered_markdown,
                cursor,
                start,
                select,
            );
        }
        self.reset_cursor_visual_targeting();
        self.dedup_cursors();
        true
    }

    pub(super) fn table_block_format_allowed(&self, cell_local_list: bool) -> bool {
        let rendered_markdown = self.active_tab_is_markdown() && self.table_source.is_none();
        self.all_cursor_ranges()
            .into_iter()
            .all(|(offset, anchor)| {
                super::table_cell_interaction::block_format_allowed(
                    &self.buffer,
                    &self.decorations,
                    rendered_markdown,
                    &(offset.min(anchor)..offset.max(anchor)),
                    cell_local_list,
                )
            })
    }

    /// Selects the whole text of `column` on `source_line`.
    fn select_table_cell(&mut self, source_line: usize, column: usize) -> bool {
        let Some(range) = self
            .decorations
            .get(source_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .and_then(|table_info| {
                rendered_cell_ranges(table_info, self.buffer.line_content(source_line))
                    .get(column)
                    .cloned()
            })
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
}

#[cfg(test)]
mod tests {
    use super::{
        nearest_boundary_index, rendered_cell_ranges, table_boundary_after_reparse,
        table_boundary_for_cursor, table_boundary_offsets, table_cell_at_offset,
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
    fn indented_first_fragment_lookup_shares_rendered_ranges_for_navigation_and_ime() {
        for row in [
            "|   - 中文 words<br>  - next | plain |",
            "|    1. ordered words | plain |",
            "|   - [x] task words | plain |",
            "|\t- tab words | plain |",
            "| plain |   - second column |",
            "| - ordinary padding | plain |",
        ] {
            let text = format!("before\n\n| A | B |\n| --- | --- |\n{row}");
            let buffer = Buffer::from_text(&text);
            let decorations = crate::markdown::parse_decorations(&text);
            let info = decorations[4].table_info.as_ref().unwrap();
            let start = buffer.line_start(4);
            let ranges = rendered_cell_ranges(info, row);
            for (column, range) in ranges.iter().enumerate() {
                for local in range.start..=range.end {
                    if row.is_char_boundary(local) {
                        assert_eq!(
                            table_cell_at_offset(&buffer, &decorations, start + local),
                            Some((2, 4, column)),
                            "{row:?} at {local}"
                        );
                    }
                }
                if range.start > 0 {
                    assert_eq!(
                        table_cell_at_offset(&buffer, &decorations, start + range.start - 1),
                        None,
                        "formatting padding/pipe must stay outside: {row:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn rendered_cell_lookup_owns_only_real_trailing_whitespace_and_before_pipe() {
        for closing_pipe in [false, true] {
            let text = format!(
                "| Neighbor | Input |\n| --- | --- |\n| 隣🙂 | first<br>-  \t{}",
                if closing_pipe { "|" } else { "" }
            );
            let buffer = Buffer::from_text(&text);
            let decorations = crate::markdown::parse_decorations(&text);
            let info = decorations[2].table_info.as_ref().unwrap();
            let line_start = buffer.line_start(2);
            let ranges = rendered_cell_ranges(info, buffer.line_content(2));
            assert_eq!(ranges[1].start, info.cell_ranges[1].start);
            assert!(ranges[1].end > info.cell_ranges[1].end);
            assert_eq!(&buffer.line_content(2)[ranges[1].clone()], "first<br>-  \t");
            for col in info.cell_ranges[1].end..=ranges[1].end {
                let offset = line_start + col;
                assert_eq!(
                    table_cell_at_offset(&buffer, &decorations, offset),
                    Some((0, 2, 1))
                );
                assert_eq!(
                    crate::editor::table_cell_edit::cell_range_for_selection(
                        &buffer,
                        &decorations,
                        true,
                        &(offset..offset)
                    ),
                    Some(line_start + ranges[1].start..line_start + ranges[1].end)
                );
            }
            // Do not expand the semantic start into the leading pipe/padding.
            for col in 0..info.cell_ranges[0].start {
                assert_eq!(
                    table_cell_at_offset(&buffer, &decorations, line_start + col),
                    None
                );
            }
            let leading = line_start + ranges[1].start - 1;
            assert_eq!(table_cell_at_offset(&buffer, &decorations, leading), None);
            let mid_utf8 = line_start + ranges[0].start + 1;
            assert_eq!(table_cell_at_offset(&buffer, &decorations, mid_utf8), None);
            let separator = buffer.line_start(1) + 2;
            assert_eq!(table_cell_at_offset(&buffer, &decorations, separator), None);
            if closing_pipe {
                assert_eq!(
                    table_cell_at_offset(&buffer, &decorations, buffer.len()),
                    None
                );
            }
        }
        let text = "---\ntitle: value  \n---";
        let buffer = Buffer::from_text(text);
        let decorations = crate::markdown::parse_decorations(text);
        assert_eq!(
            table_cell_at_offset(&buffer, &decorations, buffer.line_end(1)),
            None
        );
    }

    #[test]
    fn restored_pipe_less_eof_is_owned_by_last_fragment_including_padding() {
        for content in [
            "last",
            "last  \t",
            "first<br>",
            "first<br> \t",
            "first<br>- ",
            "first<br>1. ",
            "first<br>- [ ] ",
            "先🙂<br>末尾  ",
        ] {
            let text = format!("| Neighbor | Input |\n| --- | --- |\n| 隣🙂 | {content}");
            let buffer = Buffer::from_text(&text);
            let decorations = crate::markdown::parse_decorations(&text);
            let cursor = crate::cursor::Cursor {
                anchor: buffer.len(),
                offset: buffer.len(),
            };
            assert_eq!(
                table_cell_at_offset(&buffer, &decorations, cursor.offset),
                Some((0, 2, 1))
            );
            assert_eq!(
                table_boundary_for_cursor(&buffer, &decorations, cursor.offset),
                None,
                "{content:?}"
            );
            assert_eq!(
                table_boundary_after_reparse(&buffer, &decorations, cursor, false, None),
                None
            );
        }
    }

    #[test]
    fn explicit_below_boundary_survives_reparse_at_shared_pipe_less_eof() {
        let below = TableBoundaryCaret {
            table_start_line: 0,
            side: TableBoundarySide::Below,
        };
        for content in ["last", "first<br>  \t", "first<br>- [ ] ", "先🙂<br>- "] {
            let text = format!("| A | B |\n| --- | --- |\n| 隣🙂 | {content}");
            let mut buffer = Buffer::from_text(&text);
            let decorations = crate::markdown::parse_decorations(&text);
            let (offset, _) = table_boundary_offsets(&buffer, &decorations, below).unwrap();
            let cursor = crate::cursor::Cursor {
                offset,
                anchor: offset,
            };
            // The source offset is intentionally ambiguous; existing visual
            // intent must win over automatic cell/boundary reconstruction.
            assert!(table_cell_at_offset(&buffer, &decorations, offset).is_some());
            assert_eq!(
                table_boundary_after_reparse(&buffer, &decorations, cursor, false, Some(below)),
                Some(below)
            );
            let (insert_at, caret) = table_boundary_offsets(&buffer, &decorations, below).unwrap();
            buffer.insert(insert_at, "\n");
            assert_eq!(buffer.line_content(2), text.lines().last().unwrap());
            assert_eq!(buffer.line_content(3), "");
            assert_eq!(caret, buffer.line_start(3));
        }
    }

    #[test]
    fn closed_final_pipe_and_padding_after_it_restore_below_boundary() {
        let below = TableBoundaryCaret {
            table_start_line: 0,
            side: TableBoundarySide::Below,
        };
        for tail in ["|", "|  \t"] {
            let text = format!("| A | B |\n| --- | --- |\n| 隣🙂 | first<br>-  \t{tail}");
            let buffer = Buffer::from_text(&text);
            let decorations = crate::markdown::parse_decorations(&text);
            let cursor = crate::cursor::Cursor {
                anchor: buffer.len(),
                offset: buffer.len(),
            };
            assert_eq!(
                table_cell_at_offset(&buffer, &decorations, cursor.offset),
                None
            );
            assert_eq!(
                table_boundary_after_reparse(&buffer, &decorations, cursor, false, None),
                Some(below)
            );
            let pipe = buffer.line_start(2) + buffer.line_content(2).rfind('|').unwrap();
            assert_eq!(
                table_cell_at_offset(&buffer, &decorations, pipe),
                Some((0, 2, 1))
            );
            assert_eq!(table_boundary_for_cursor(&buffer, &decorations, pipe), None);
        }
    }

    #[test]
    fn reparse_preserves_only_valid_single_caret_boundary_intent() {
        let text = "A | B\n--- | ---\nC | D  \t";
        let buffer = Buffer::from_text(text);
        let decorations = crate::markdown::parse_decorations(text);
        let below = TableBoundaryCaret {
            table_start_line: 0,
            side: TableBoundarySide::Below,
        };
        let above = TableBoundaryCaret {
            table_start_line: 0,
            side: TableBoundarySide::Above,
        };
        let eof = crate::cursor::Cursor {
            anchor: buffer.len(),
            offset: buffer.len(),
        };
        let start = crate::cursor::Cursor {
            anchor: 0,
            offset: 0,
        };
        assert_eq!(table_boundary_for_cursor(&buffer, &decorations, 0), None);
        assert_eq!(
            table_boundary_after_reparse(&buffer, &decorations, start, false, Some(above)),
            Some(above)
        );
        assert_eq!(
            table_boundary_after_reparse(&buffer, &decorations, eof, true, Some(below)),
            None
        );
        let selection = crate::cursor::Cursor {
            anchor: buffer.len() - 1,
            ..eof
        };
        assert_eq!(
            table_boundary_after_reparse(&buffer, &decorations, selection, false, Some(below)),
            None
        );
        let moved = crate::cursor::Cursor {
            offset: buffer.len() - 1,
            anchor: buffer.len() - 1,
        };
        assert_eq!(
            table_boundary_after_reparse(&buffer, &decorations, moved, false, Some(below)),
            None
        );
        let wrong_table = TableBoundaryCaret {
            table_start_line: 2,
            ..below
        };
        assert_eq!(
            table_boundary_after_reparse(&buffer, &decorations, eof, false, Some(wrong_table)),
            None
        );
        let front = Buffer::from_text("---\ntitle: value\n---");
        let front_decorations = crate::markdown::parse_decorations(front.text());
        let front_eof = crate::cursor::Cursor {
            offset: front.len(),
            anchor: front.len(),
        };
        assert_eq!(
            table_boundary_after_reparse(&front, &front_decorations, front_eof, false, Some(below)),
            None
        );
    }

    #[test]
    fn offset_only_snapshot_restoration_prefers_cell_without_inheriting_boundary() {
        let text = "| A | B |\n| --- | --- |\n| 隣🙂 | first<br>- ";
        let buffer = Buffer::from_text(text);
        let decorations = crate::markdown::parse_decorations(text);
        let snapshot = crate::editor::EditorSnapshot {
            text: text.to_owned(),
            cursor_offset: text.len(),
            cursor_anchor: text.len(),
            ..Default::default()
        };
        let restored = crate::cursor::Cursor {
            offset: snapshot.cursor_offset,
            anchor: snapshot.cursor_anchor,
        };
        let below = TableBoundaryCaret {
            table_start_line: 0,
            side: TableBoundarySide::Below,
        };
        assert_eq!(
            table_boundary_after_reparse(&buffer, &decorations, restored, false, Some(below)),
            Some(below)
        );
        // Undo/tab restoration clears live intent: these offset-only snapshots
        // cannot distinguish an EOF cell caret from a deliberately chosen edge.
        assert_eq!(
            table_boundary_after_reparse(&buffer, &decorations, restored, false, None),
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
