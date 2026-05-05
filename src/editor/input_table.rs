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
}

#[cfg(test)]
mod tests {
    use super::{nearest_boundary_index, visible_table_line_indices};
    use crate::markdown::{LineDecoration, TableLineInfo};

    fn table_line_info(is_separator: bool, table_end_line: usize) -> TableLineInfo {
        TableLineInfo {
            cells: vec![],
            cell_ranges: vec![],
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

        assert_eq!(visible_table_line_indices(&decorations, 0), Some(vec![0, 2, 3]));
    }
}
