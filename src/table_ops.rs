use crate::markdown::{TableAlign, parse_table_cells_with_ranges};

pub const TABLE_SIZE_PICKER_MAX_ROWS: usize = 8;
pub const TABLE_SIZE_PICKER_MAX_COLUMNS: usize = 8;

#[derive(Debug, Clone)]
pub struct TableModel {
    pub col_count: usize,
    pub header_cells: Vec<String>,
    pub aligns: Vec<TableAlign>,
    pub data_rows: Vec<Vec<String>>,
}

fn all_rows(model: &TableModel) -> Vec<Vec<String>> {
    std::iter::once(model.header_cells.clone())
        .chain(model.data_rows.clone())
        .collect()
}

fn set_all_rows(model: &mut TableModel, mut rows: Vec<Vec<String>>) {
    let col_count = model
        .col_count
        .max(rows.iter().map(|row| row.len()).max().unwrap_or(0))
        .max(1);
    if rows.is_empty() {
        rows.push(vec![String::new(); col_count]);
    }

    let mut padded_rows: Vec<Vec<String>> =
        rows.iter().map(|row| pad_cells(row, col_count)).collect();
    model.col_count = col_count;
    model.aligns.resize(col_count, TableAlign::Left);
    model.header_cells = padded_rows.remove(0);
    model.data_rows = padded_rows;
}

fn parse_row(text: &str) -> Vec<String> {
    parse_table_cells_with_ranges(text)
        .into_iter()
        .map(|(cell, _)| cell)
        .collect()
}

fn parse_align_cell(cell: &str) -> TableAlign {
    let t = cell.trim();
    let starts = t.starts_with(':');
    let ends = t.ends_with(':');
    if starts && ends {
        TableAlign::Center
    } else if ends {
        TableAlign::Right
    } else {
        TableAlign::Left
    }
}

fn pad_cells(cells: &[String], count: usize) -> Vec<String> {
    let mut result = cells.to_vec();
    result.resize(count, String::new());
    result
}

pub(crate) fn cell_display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            if (0x1100..=0x115F).contains(&code)
                || (0x2E80..=0x33BF).contains(&code)
                || (0x3040..=0x9FFF).contains(&code)
                || (0xA000..=0xA4CF).contains(&code)
                || (0xAC00..=0xD7AF).contains(&code)
                || (0xF900..=0xFAFF).contains(&code)
                || (0xFE30..=0xFE6F).contains(&code)
                || (0xFF01..=0xFF60).contains(&code)
                || (0xFFE0..=0xFFE6).contains(&code)
                || (0x20000..=0x2FA1F).contains(&code)
            {
                2
            } else {
                1
            }
        })
        .sum()
}

fn pad_cell_to_width(cell: &str, width: usize) -> String {
    let padding = width.saturating_sub(cell_display_width(cell));
    if padding == 0 {
        cell.to_string()
    } else {
        format!("{cell}{}", " ".repeat(padding))
    }
}

fn table_column_widths(model: &TableModel, col_count: usize) -> Vec<usize> {
    let mut widths = vec![1; col_count];
    for (index, cell) in pad_cells(&model.header_cells, col_count).iter().enumerate() {
        widths[index] = widths[index].max(cell_display_width(cell));
    }
    for row in &model.data_rows {
        for (index, cell) in pad_cells(row, col_count).iter().enumerate() {
            widths[index] = widths[index].max(cell_display_width(cell));
        }
    }
    widths
}

pub fn parse_table_block(lines: &[String]) -> TableModel {
    if lines.is_empty() {
        return TableModel {
            col_count: 1,
            header_cells: vec![String::new()],
            aligns: vec![TableAlign::Left],
            data_rows: Vec::new(),
        };
    }

    let raw_header = parse_row(lines.first().map(|s| s.as_str()).unwrap_or(""));
    let raw_aligns: Vec<TableAlign> = if lines.len() > 1 {
        parse_row(&lines[1])
            .iter()
            .map(|c| parse_align_cell(c))
            .collect()
    } else {
        Vec::new()
    };
    let raw_rows: Vec<Vec<String>> = lines.iter().skip(2).map(|l| parse_row(l)).collect();

    let col_count = raw_header
        .len()
        .max(raw_aligns.len())
        .max(raw_rows.iter().map(|r| r.len()).max().unwrap_or(0))
        .max(1);

    let header_cells = pad_cells(&raw_header, col_count);
    let aligns: Vec<TableAlign> = {
        let mut a = raw_aligns;
        a.resize(col_count, TableAlign::Left);
        a
    };
    let data_rows: Vec<Vec<String>> = raw_rows.iter().map(|r| pad_cells(r, col_count)).collect();

    TableModel {
        col_count,
        header_cells,
        aligns,
        data_rows,
    }
}

pub fn selected_table_matrix_to_tsv(
    lines: &[String],
    is_column: bool,
    indices: &[usize],
) -> String {
    let model = parse_table_block(lines);
    if is_column {
        (0..=model.data_rows.len())
            .map(|row| {
                indices
                    .iter()
                    .map(|column| {
                        if row == 0 {
                            model.header_cells.get(*column)
                        } else {
                            model
                                .data_rows
                                .get(row - 1)
                                .and_then(|cells| cells.get(*column))
                        }
                        .cloned()
                        .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        indices
            .iter()
            .filter_map(|row| {
                if *row == 0 {
                    Some(model.header_cells.join("\t"))
                } else {
                    model.data_rows.get(row - 1).map(|cells| cells.join("\t"))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn selected_cells_to_tsv(
    lines: &[String],
    rows: std::ops::RangeInclusive<usize>,
    columns: std::ops::RangeInclusive<usize>,
) -> String {
    let model = parse_table_block(lines);
    let all = all_rows(&model);
    rows.filter_map(|row| all.get(row))
        .map(|row| {
            columns
                .clone()
                .map(|column| row.get(column).cloned().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn replace_selected_cells(
    lines: &[String],
    rows: std::ops::RangeInclusive<usize>,
    columns: std::ops::RangeInclusive<usize>,
    values: &[Vec<String>],
) -> Option<Vec<String>> {
    let mut model = parse_table_block(lines);
    let row_indices = rows.collect::<Vec<_>>();
    let column_indices = columns.collect::<Vec<_>>();
    if row_indices.is_empty()
        || column_indices.is_empty()
        || values.len() != row_indices.len()
        || values.iter().any(|row| row.len() != column_indices.len())
        || row_indices.iter().any(|row| *row > model.data_rows.len())
        || column_indices.iter().any(|col| *col >= model.col_count)
    {
        return None;
    }
    for (row_index, values) in row_indices.into_iter().zip(values) {
        let row = if row_index == 0 {
            &mut model.header_cells
        } else {
            &mut model.data_rows[row_index - 1]
        };
        for (&column, value) in column_indices.iter().zip(values) {
            row[column] = value.clone();
        }
    }
    Some(serialize_table(&model))
}

#[cfg(test)]
#[test]
fn rectangular_cell_edits_preserve_unselected_cells() {
    let lines = [
        "| A | B | C |",
        "| --- | --- | --- |",
        "| a | b | c |",
        "| d | e | f |",
    ]
    .map(str::to_string);
    assert_eq!(selected_cells_to_tsv(&lines, 1..=2, 1..=2), "b\tc\ne\tf");
    let updated = replace_selected_cells(
        &lines,
        1..=2,
        1..=2,
        &[vec!["x".into(), "y".into()], vec!["z".into(), "w".into()]],
    )
    .unwrap();
    assert_eq!(
        parse_table_block(&updated).data_rows,
        vec![vec!["a", "x", "y"], vec!["d", "z", "w"]]
    );
    assert!(replace_selected_cells(&lines, 1..=2, 1..=2, &[vec!["x".into()]]).is_none());
}

fn serialize_row(cells: &[String], widths: &[usize]) -> String {
    let padded = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| pad_cell_to_width(cell, widths.get(index).copied().unwrap_or(1)))
        .collect::<Vec<_>>();
    format!("| {} |", padded.join(" | "))
}

fn serialize_align_row(aligns: &[TableAlign], widths: &[usize]) -> String {
    let cells: Vec<String> = aligns
        .iter()
        .enumerate()
        .map(|(index, align)| {
            let width = widths.get(index).copied().unwrap_or(1);
            match align {
                TableAlign::Left => "-".repeat(width.max(3)),
                TableAlign::Center => {
                    let width = width.max(5);
                    format!(":{}:", "-".repeat(width - 2))
                }
                TableAlign::Right => {
                    let width = width.max(4);
                    format!("{}:", "-".repeat(width - 1))
                }
            }
        })
        .collect();
    format!("| {} |", cells.join(" | "))
}

pub fn serialize_table(model: &TableModel) -> Vec<String> {
    let col_count = model.col_count.max(model.header_cells.len()).max(1);
    let header = pad_cells(&model.header_cells, col_count);
    let mut aligns = model.aligns.clone();
    aligns.resize(col_count, TableAlign::Left);
    let widths = table_column_widths(model, col_count);

    serialize_table_with_widths(model, &header, &aligns, &widths)
}

pub fn format_table_source(lines: &[String]) -> Vec<String> {
    if lines.iter().any(|line| !line.starts_with('|')) {
        return lines.to_vec();
    }
    let model = parse_table_block(lines);
    let mut widths = table_column_widths(&model, model.col_count);
    for (width, align) in widths.iter_mut().zip(&model.aligns) {
        *width = (*width).max(match align {
            TableAlign::Left => 3,
            TableAlign::Center => 5,
            TableAlign::Right => 4,
        });
    }
    serialize_table_with_widths(&model, &model.header_cells, &model.aligns, &widths)
}

fn serialize_table_with_widths(
    model: &TableModel,
    header: &[String],
    aligns: &[TableAlign],
    widths: &[usize],
) -> Vec<String> {
    let col_count = widths.len();

    let mut result = vec![
        serialize_row(header, widths),
        serialize_align_row(aligns, widths),
    ];
    for row in &model.data_rows {
        result.push(serialize_row(&pad_cells(row, col_count), &widths));
    }
    result
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn update_cell(
    lines: &[String],
    line_offset: usize,
    col_idx: usize,
    text: &str,
) -> Vec<String> {
    let mut model = parse_table_block(lines);
    if line_offset == 0 {
        if col_idx < model.header_cells.len() {
            model.header_cells[col_idx] = text.to_string();
        }
    } else {
        let row_idx = line_offset.saturating_sub(1);
        if row_idx < model.data_rows.len() && col_idx < model.col_count {
            model.data_rows[row_idx][col_idx] = text.to_string();
        }
    }
    serialize_table(&model)
}

pub fn insert_row(lines: &[String], insert_at: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let mut rows = all_rows(&model);
    let clamped = insert_at.min(rows.len());
    let empty_row = vec![String::new(); model.col_count];
    rows.insert(clamped, empty_row);
    set_all_rows(&mut model, rows);
    serialize_table(&model)
}

#[cfg(test)]
pub fn delete_row(lines: &[String], row_index: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let mut rows = all_rows(&model);
    if row_index < rows.len() {
        if rows.len() == 1 {
            rows[0] = vec![String::new(); model.col_count.max(1)];
        } else {
            rows.remove(row_index);
        }
        set_all_rows(&mut model, rows);
    }
    serialize_table(&model)
}

#[cfg(test)]
#[cfg(test)]
pub fn move_row(lines: &[String], from: usize, to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let mut rows = all_rows(&model);
    if from >= rows.len() || to > rows.len() || from == to {
        return serialize_table(&model);
    }
    let row = rows.remove(from);
    let adjusted_to = if to > from { to.saturating_sub(1) } else { to };
    let clamped = adjusted_to.min(rows.len());
    rows.insert(clamped, row);
    set_all_rows(&mut model, rows);
    serialize_table(&model)
}

#[cfg(test)]
pub fn delete_rows(lines: &[String], start: usize, end: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let mut rows = all_rows(&model);
    let start = start.min(rows.len());
    let end = end.min(rows.len()).max(start);
    if start < end {
        if start == 0 && end == rows.len() {
            rows = vec![vec![String::new(); model.col_count.max(1)]];
        } else {
            rows.drain(start..end);
        }
        set_all_rows(&mut model, rows);
    }
    serialize_table(&model)
}

#[cfg(test)]
pub fn replace_rows(lines: &[String], start: usize, rows: &[Vec<String>]) -> Option<Vec<String>> {
    let mut model = parse_table_block(lines);
    if rows.is_empty() || rows.iter().any(|row| row.len() != model.col_count) {
        return None;
    }
    let mut all = all_rows(&model);
    let end = start.checked_add(rows.len())?;
    if end > all.len() {
        return None;
    }
    for (target, replacement) in all[start..end].iter_mut().zip(rows) {
        *target = replacement.clone();
    }
    set_all_rows(&mut model, all);
    Some(serialize_table(&model))
}

#[cfg(test)]
pub fn move_rows(lines: &[String], start: usize, end: usize, to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let mut rows = all_rows(&model);
    let start = start.min(rows.len());
    let end = end.min(rows.len()).max(start);
    let to = to.min(rows.len());
    if start == end || (start <= to && to <= end) {
        return serialize_table(&model);
    }
    let moved: Vec<_> = rows.drain(start..end).collect();
    let destination = if to > end { to - (end - start) } else { to };
    rows.splice(destination..destination, moved);
    set_all_rows(&mut model, rows);
    serialize_table(&model)
}

pub fn insert_column(lines: &[String], insert_at: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let clamped = insert_at.min(model.col_count);
    model.header_cells.insert(clamped, String::new());
    model.aligns.insert(clamped, TableAlign::Left);
    for row in &mut model.data_rows {
        row.insert(clamped, String::new());
    }
    model.col_count += 1;
    serialize_table(&model)
}

#[cfg(test)]
pub fn delete_column(lines: &[String], col_idx: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    if model.col_count <= 1 || col_idx >= model.col_count {
        return serialize_table(&model);
    }
    model.header_cells.remove(col_idx);
    if col_idx < model.aligns.len() {
        model.aligns.remove(col_idx);
    }
    for row in &mut model.data_rows {
        if col_idx < row.len() {
            row.remove(col_idx);
        }
    }
    model.col_count -= 1;
    serialize_table(&model)
}

#[cfg(test)]
pub fn delete_columns(lines: &[String], start: usize, end: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let start = start.min(model.col_count);
    let end = end.min(model.col_count).max(start);
    if start == end {
        return serialize_table(&model);
    }
    if end - start >= model.col_count {
        model.col_count = 1;
        model.header_cells = vec![String::new()];
        model.aligns = vec![TableAlign::Left];
        model.data_rows = vec![vec![String::new()]];
    } else {
        model.header_cells.drain(start..end);
        model.aligns.drain(start..end);
        for row in &mut model.data_rows {
            row.drain(start..end);
        }
        model.col_count -= end - start;
    }
    serialize_table(&model)
}

#[cfg(test)]
pub fn replace_columns(
    lines: &[String],
    start: usize,
    columns: &[Vec<String>],
) -> Option<Vec<String>> {
    let mut model = parse_table_block(lines);
    if columns.is_empty()
        || columns
            .iter()
            .any(|column| column.len() != model.data_rows.len() + 1)
    {
        return None;
    }
    let end = start.checked_add(columns.len())?;
    if end > model.col_count {
        return None;
    }
    for (offset, values) in columns.iter().enumerate() {
        model.header_cells[start + offset] = values[0].clone();
        for (row, value) in model.data_rows.iter_mut().zip(values.iter().skip(1)) {
            row[start + offset] = value.clone();
        }
    }
    Some(serialize_table(&model))
}

pub fn delete_rows_at(lines: &[String], selected: &[usize]) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let mut rows = all_rows(&model);
    let selected: std::collections::HashSet<_> = selected.iter().copied().collect();
    let kept = rows
        .drain(..)
        .enumerate()
        .filter_map(|(index, row)| (!selected.contains(&index)).then_some(row))
        .collect::<Vec<_>>();
    if kept.is_empty() {
        rows.push(vec![String::new(); model.col_count.max(1)]);
    } else {
        rows = kept;
    }
    set_all_rows(&mut model, rows);
    serialize_table(&model)
}

pub fn insert_rows_at(lines: &[String], at: usize, rows: &[Vec<String>]) -> Option<Vec<String>> {
    let mut model = parse_table_block(lines);
    if rows.is_empty()
        || rows
            .iter()
            .any(|row| row.is_empty() || row.len() > model.col_count)
    {
        return None;
    }
    let mut all = all_rows(&model);
    all.splice(
        at.min(all.len())..at.min(all.len()),
        rows.iter().map(|row| pad_cells(row, model.col_count)),
    );
    set_all_rows(&mut model, all);
    Some(serialize_table(&model))
}

pub fn insert_columns_at(lines: &[String], at: usize, rows: &[Vec<String>]) -> Option<Vec<String>> {
    let mut model = parse_table_block(lines);
    let width = rows.first()?.len();
    if width == 0
        || rows.len() > model.data_rows.len() + 1
        || rows.iter().any(|row| row.len() != width)
    {
        return None;
    }
    let at = at.min(model.col_count);
    model.header_cells.splice(at..at, rows[0].clone());
    model
        .aligns
        .splice(at..at, std::iter::repeat_n(TableAlign::Left, width));
    for (index, row) in model.data_rows.iter_mut().enumerate() {
        let values = rows
            .get(index + 1)
            .cloned()
            .unwrap_or_else(|| vec![String::new(); width]);
        row.splice(at..at, values);
    }
    model.col_count += width;
    Some(serialize_table(&model))
}

pub fn delete_columns_at(lines: &[String], selected: &[usize]) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let selected: std::collections::HashSet<_> = selected.iter().copied().collect();
    let kept = (0..model.col_count)
        .filter(|index| !selected.contains(index))
        .collect::<Vec<_>>();
    let kept = if kept.is_empty() { vec![0] } else { kept };
    model.header_cells = kept
        .iter()
        .map(|index| model.header_cells.get(*index).cloned().unwrap_or_default())
        .collect();
    model.aligns = kept
        .iter()
        .map(|index| {
            model
                .aligns
                .get(*index)
                .copied()
                .unwrap_or(TableAlign::Left)
        })
        .collect();
    for row in &mut model.data_rows {
        *row = kept
            .iter()
            .map(|index| row.get(*index).cloned().unwrap_or_default())
            .collect();
    }
    model.col_count = kept.len();
    serialize_table(&model)
}

pub fn replace_rows_at(
    lines: &[String],
    selected: &[usize],
    replacements: &[Vec<String>],
) -> Option<Vec<String>> {
    if selected.len() != replacements.len() {
        return None;
    }
    let mut model = parse_table_block(lines);
    if replacements.iter().any(|row| row.len() != model.col_count) {
        return None;
    }
    for (&index, row) in selected.iter().zip(replacements) {
        if index == 0 {
            model.header_cells = row.clone();
        } else {
            *model.data_rows.get_mut(index - 1)? = row.clone();
        }
    }
    Some(serialize_table(&model))
}

pub fn replace_columns_at(
    lines: &[String],
    selected: &[usize],
    replacements: &[Vec<String>],
) -> Option<Vec<String>> {
    if selected.len() != replacements.len() {
        return None;
    }
    let mut model = parse_table_block(lines);
    if replacements
        .iter()
        .any(|column| column.len() != model.data_rows.len() + 1)
    {
        return None;
    }
    for (&index, column) in selected.iter().zip(replacements) {
        if index >= model.col_count {
            return None;
        }
        model.header_cells[index] = column[0].clone();
        for (row, value) in model.data_rows.iter_mut().zip(column.iter().skip(1)) {
            row[index] = value.clone();
        }
    }
    Some(serialize_table(&model))
}

pub fn move_selected_rows(lines: &[String], selected: &[usize], to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let rows = all_rows(&model);
    let selected_set: std::collections::HashSet<_> = selected.iter().copied().collect();
    let mut moving = selected
        .iter()
        .filter_map(|index| rows.get(*index).cloned())
        .collect::<Vec<_>>();
    let mut remaining = rows
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| (!selected_set.contains(&index)).then_some(row))
        .collect::<Vec<_>>();
    let destination = to
        .saturating_sub(selected.iter().filter(|index| **index < to).count())
        .min(remaining.len());
    remaining.splice(destination..destination, moving.drain(..));
    set_all_rows(&mut model, remaining);
    serialize_table(&model)
}

pub fn move_selected_columns(lines: &[String], selected: &[usize], to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let selected_set: std::collections::HashSet<_> = selected.iter().copied().collect();
    let selected = (0..model.col_count)
        .filter(|index| selected_set.contains(index))
        .collect::<Vec<_>>();
    let mut order = (0..model.col_count)
        .filter(|index| !selected_set.contains(index))
        .collect::<Vec<_>>();
    let destination = to
        .saturating_sub(selected.iter().filter(|index| **index < to).count())
        .min(order.len());
    order.splice(destination..destination, selected);
    model.header_cells = order
        .iter()
        .map(|index| model.header_cells[*index].clone())
        .collect();
    model.aligns = order.iter().map(|index| model.aligns[*index]).collect();
    for row in &mut model.data_rows {
        *row = order.iter().map(|index| row[*index].clone()).collect();
    }
    serialize_table(&model)
}

#[cfg(test)]
pub fn move_columns(lines: &[String], start: usize, end: usize, to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let count = model.col_count;
    let start = start.min(count);
    let end = end.min(count).max(start);
    let to = to.min(count);
    if start == end || (start <= to && to <= end) {
        return serialize_table(&model);
    }
    let destination = if to > end { to - (end - start) } else { to };
    let moved: Vec<_> = model.header_cells.drain(start..end).collect();
    model.header_cells.splice(destination..destination, moved);
    let moved: Vec<_> = model.aligns.drain(start..end).collect();
    model.aligns.splice(destination..destination, moved);
    for row in &mut model.data_rows {
        let moved: Vec<_> = row.drain(start..end).collect();
        row.splice(destination..destination, moved);
    }
    serialize_table(&model)
}

pub fn resize_table(lines: &[String], data_row_count: usize, column_count: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    let column_count = column_count.max(1);
    let data_row_count = data_row_count.max(1);

    model.col_count = column_count;
    model.header_cells.resize(column_count, String::new());
    model.aligns.resize(column_count, TableAlign::Left);
    model
        .data_rows
        .resize_with(data_row_count, || vec![String::new(); column_count]);
    model.data_rows.truncate(data_row_count);
    for row in &mut model.data_rows {
        row.resize(column_count, String::new());
    }

    serialize_table(&model)
}

pub fn resize_table_from_picker(
    lines: &[String],
    selected_data_rows: usize,
    selected_columns: usize,
) -> Vec<String> {
    let model = parse_table_block(lines);
    let data_rows = if model.data_rows.len() > TABLE_SIZE_PICKER_MAX_ROWS {
        model.data_rows.len()
    } else {
        selected_data_rows
    };
    let columns = if model.col_count > TABLE_SIZE_PICKER_MAX_COLUMNS {
        model.col_count
    } else {
        selected_columns
    };
    resize_table(lines, data_rows, columns)
}

#[cfg(test)]
pub fn set_column_alignment(
    lines: &[String],
    col_idx: usize,
    alignment: TableAlign,
) -> Vec<String> {
    set_columns_alignment(lines, &[col_idx], alignment)
}

pub fn set_columns_alignment(
    lines: &[String],
    columns: &[usize],
    alignment: TableAlign,
) -> Vec<String> {
    let mut model = parse_table_block(lines);
    for &column in columns {
        if column < model.col_count {
            model.aligns[column] = alignment;
        }
    }
    serialize_table(&model)
}

#[cfg(test)]
#[cfg(test)]
pub fn move_column(lines: &[String], from: usize, to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    if from >= model.col_count || to > model.col_count || from == to {
        return serialize_table(&model);
    }

    let h = model.header_cells.remove(from);
    let adjusted = if to > from { to.saturating_sub(1) } else { to };
    model
        .header_cells
        .insert(adjusted.min(model.header_cells.len()), h);

    if from < model.aligns.len() {
        let a = model.aligns.remove(from);
        model.aligns.insert(adjusted.min(model.aligns.len()), a);
    }

    for row in &mut model.data_rows {
        if from < row.len() {
            let cell = row.remove(from);
            row.insert(adjusted.min(row.len()), cell);
        }
    }

    serialize_table(&model)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatterEntry {
    pub key: String,
    pub value: String,
}

pub fn parse_front_matter_block(lines: &[String]) -> Vec<FrontMatterEntry> {
    let inner = if lines.len() >= 2
        && lines.first().is_some_and(|line| line.trim() == "---")
        && lines.last().is_some_and(|line| line.trim() == "---")
    {
        &lines[1..lines.len() - 1]
    } else {
        lines
    };

    inner
        .iter()
        .map(|line| {
            if let Some((key, value)) = line.split_once(':') {
                FrontMatterEntry {
                    key: key.trim().to_string(),
                    value: value.trim().to_string(),
                }
            } else {
                FrontMatterEntry {
                    key: line.trim().to_string(),
                    value: String::new(),
                }
            }
        })
        .collect()
}

pub fn serialize_front_matter(entries: &[FrontMatterEntry]) -> Vec<String> {
    let mut result = Vec::with_capacity(entries.len() + 2);
    result.push("---".to_string());
    let normalized = if entries.is_empty() {
        vec![FrontMatterEntry::default()]
    } else {
        entries.to_vec()
    };
    for entry in normalized {
        result.push(format!("{}: {}", entry.key, entry.value));
    }
    result.push("---".to_string());
    result
}

pub fn insert_front_matter_row(lines: &[String], insert_at: usize) -> Vec<String> {
    let mut entries = parse_front_matter_block(lines);
    let clamped = insert_at.min(entries.len());
    entries.insert(clamped, FrontMatterEntry::default());
    serialize_front_matter(&entries)
}

pub fn delete_front_matter_row(lines: &[String], row_index: usize) -> Vec<String> {
    let mut entries = parse_front_matter_block(lines);
    if row_index < entries.len() {
        if entries.len() == 1 {
            entries[0] = FrontMatterEntry::default();
        } else {
            entries.remove(row_index);
        }
    }
    serialize_front_matter(&entries)
}

pub fn move_front_matter_row(lines: &[String], from: usize, to: usize) -> Vec<String> {
    let mut entries = parse_front_matter_block(lines);
    if from >= entries.len() || to > entries.len() || from == to {
        return serialize_front_matter(&entries);
    }

    let row = entries.remove(from);
    let adjusted_to = if to > from { to.saturating_sub(1) } else { to };
    entries.insert(adjusted_to.min(entries.len()), row);
    serialize_front_matter(&entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::TableAlign;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn test_parse_serialize_roundtrip() {
        let input = lines(&[
            "| Name | Age |",
            "| --- | --- |",
            "| Alice | 30 |",
            "| Bob | 25 |",
        ]);
        let model = parse_table_block(&input);
        assert_eq!(model.col_count, 2);
        assert_eq!(model.header_cells, vec!["Name", "Age"]);
        assert_eq!(model.data_rows.len(), 2);
        assert_eq!(model.data_rows[0], vec!["Alice", "30"]);

        let output = serialize_table(&model);
        assert_eq!(output.len(), 4);
        assert_eq!(output[0], "| Name  | Age |");
        assert_eq!(output[1], "| ----- | --- |");
    }

    #[test]
    fn test_serialize_table_pads_columns_to_max_width() {
        let input = lines(&["| Key | Longer value |", "| --- | ---: |", "| Value | x |"]);

        let output = serialize_table(&parse_table_block(&input));
        assert_eq!(output[0], "| Key   | Longer value |");
        assert_eq!(output[1], "| ----- | -----------: |");
        assert_eq!(output[2], "| Value | x            |");
    }

    #[test]
    fn resize_table_preserves_cells_inside_the_new_shape() {
        let lines = vec![
            "| A | B | C |".to_string(),
            "| --- | :---: | ---: |".to_string(),
            "| 1 | 2 | 3 |".to_string(),
            "| 4 | 5 | 6 |".to_string(),
        ];

        let smaller = resize_table(&lines, 1, 2);
        let smaller_model = parse_table_block(&smaller);
        assert_eq!(smaller_model.header_cells, ["A", "B"]);
        assert_eq!(smaller_model.data_rows, [["1", "2"]]);
        assert_eq!(smaller_model.aligns, [TableAlign::Left, TableAlign::Center]);

        let larger = resize_table(&smaller, 3, 4);
        let larger_model = parse_table_block(&larger);
        assert_eq!(larger_model.col_count, 4);
        assert_eq!(larger_model.data_rows.len(), 3);
        assert_eq!(larger_model.header_cells[..2], ["A", "B"]);
        assert_eq!(larger_model.data_rows[0][..2], ["1", "2"]);
        assert!(larger_model.data_rows[2].iter().all(String::is_empty));
    }

    #[test]
    fn picker_resize_preserves_dimensions_that_exceed_its_grid() {
        let rows = (0..10)
            .map(|row| format!("| r{row}a | r{row}b |"))
            .collect::<Vec<_>>();
        let mut tall = vec!["| A | B |".to_string(), "| --- | --- |".to_string()];
        tall.extend(rows);
        let resized = parse_table_block(&resize_table_from_picker(&tall, 3, 4));
        assert_eq!(resized.data_rows.len(), 10);
        assert_eq!(resized.col_count, 4);

        let wide_header = format!(
            "| {} |",
            (0..10)
                .map(|i| format!("C{i}"))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        let wide_separator = format!("| {} |", vec!["---"; 10].join(" | "));
        let wide_row = format!(
            "| {} |",
            (0..10)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(" | ")
        );
        let wide = vec![wide_header, wide_separator, wide_row];
        let resized = parse_table_block(&resize_table_from_picker(&wide, 4, 3));
        assert_eq!(resized.data_rows.len(), 4);
        assert_eq!(resized.col_count, 10);
    }

    #[test]
    fn test_alignment_parsing() {
        let input = lines(&["| L | C | R |", "| --- | :---: | ---: |", "| a | b | c |"]);
        let model = parse_table_block(&input);
        assert_eq!(
            model.aligns,
            vec![TableAlign::Left, TableAlign::Center, TableAlign::Right]
        );
    }

    #[test]
    fn test_set_column_alignment() {
        let input = lines(&["| A | B | C |", "| --- | --- | --- |", "| 1 | 2 | 3 |"]);

        let centered = set_column_alignment(&input, 1, TableAlign::Center);
        assert_eq!(
            parse_table_block(&centered).aligns,
            vec![TableAlign::Left, TableAlign::Center, TableAlign::Left]
        );

        let right_aligned = set_column_alignment(&centered, 2, TableAlign::Right);
        assert_eq!(
            parse_table_block(&right_aligned).aligns,
            vec![TableAlign::Left, TableAlign::Center, TableAlign::Right]
        );

        let unchanged = set_column_alignment(&right_aligned, 3, TableAlign::Left);
        assert_eq!(unchanged, right_aligned);

        let selected = set_columns_alignment(&input, &[0, 2], TableAlign::Center);
        assert_eq!(
            parse_table_block(&selected).aligns,
            vec![TableAlign::Center, TableAlign::Left, TableAlign::Center]
        );
    }

    #[test]
    fn test_insert_row() {
        let input = lines(&["| H1 | H2 |", "| --- | --- |", "| A | B |"]);
        let result = insert_row(&input, 0);
        let model = parse_table_block(&result);
        assert_eq!(model.header_cells, vec!["", ""]);
        assert_eq!(model.data_rows.len(), 2);
        assert_eq!(model.data_rows[0], vec!["H1", "H2"]);
        assert_eq!(model.data_rows[1], vec!["A", "B"]);
    }

    #[test]
    fn test_delete_row() {
        let input = lines(&["| H1 | H2 |", "| --- | --- |", "| A | B |", "| C | D |"]);
        let result = delete_row(&input, 0);
        let model = parse_table_block(&result);
        assert_eq!(model.header_cells, vec!["A", "B"]);
        assert_eq!(model.data_rows.len(), 1);
        assert_eq!(model.data_rows[0], vec!["C", "D"]);
    }

    #[test]
    fn test_move_row() {
        let input = lines(&["| H |", "| --- |", "| A |", "| B |", "| C |"]);
        let result = move_row(&input, 0, 2);
        let model = parse_table_block(&result);
        assert_eq!(model.header_cells, vec!["A"]);
        assert_eq!(model.data_rows[0], vec!["H"]);
        assert_eq!(model.data_rows[1], vec!["B"]);
        assert_eq!(model.data_rows[2], vec!["C"]);
    }

    #[test]
    fn test_insert_column() {
        let input = lines(&["| A | B |", "| --- | --- |", "| 1 | 2 |"]);
        let result = insert_column(&input, 1);
        let model = parse_table_block(&result);
        assert_eq!(model.col_count, 3);
        assert_eq!(model.header_cells, vec!["A", "", "B"]);
        assert_eq!(model.data_rows[0], vec!["1", "", "2"]);
    }

    #[test]
    fn test_delete_column() {
        let input = lines(&["| A | B | C |", "| --- | --- | --- |", "| 1 | 2 | 3 |"]);
        let result = delete_column(&input, 1);
        let model = parse_table_block(&result);
        assert_eq!(model.col_count, 2);
        assert_eq!(model.header_cells, vec!["A", "C"]);
        assert_eq!(model.data_rows[0], vec!["1", "3"]);
    }

    #[test]
    fn test_front_matter_insert_row() {
        let input = lines(&["---", "title: Hello", "---"]);
        let result = insert_front_matter_row(&input, 1);
        assert_eq!(result, lines(&["---", "title: Hello", ": ", "---"]));
    }

    #[test]
    fn test_front_matter_delete_last_row_keeps_editable_block() {
        let input = lines(&["---", "title: Hello", "---"]);
        let result = delete_front_matter_row(&input, 0);
        assert_eq!(result, lines(&["---", ": ", "---"]));
    }

    #[test]
    fn test_front_matter_move_row() {
        let input = lines(&["---", "title: Hello", "tags: Rust", "---"]);
        let result = move_front_matter_row(&input, 0, 2);
        assert_eq!(result, lines(&["---", "tags: Rust", "title: Hello", "---"]));
    }

    #[test]
    fn test_delete_last_column_prevented() {
        let input = lines(&["| A |", "| --- |", "| 1 |"]);
        let result = delete_column(&input, 0);
        let model = parse_table_block(&result);
        assert_eq!(model.col_count, 1);
        assert_eq!(model.header_cells, vec!["A"]);
    }

    #[test]
    fn test_move_column() {
        let input = lines(&["| A | B | C |", "| --- | :---: | ---: |", "| 1 | 2 | 3 |"]);
        let result = move_column(&input, 0, 2);
        let model = parse_table_block(&result);
        assert_eq!(model.header_cells, vec!["B", "A", "C"]);
        assert_eq!(
            model.aligns,
            vec![TableAlign::Center, TableAlign::Left, TableAlign::Right]
        );
        assert_eq!(model.data_rows[0], vec!["2", "1", "3"]);
    }

    #[test]
    fn selected_rows_move_as_a_block_and_preserve_order() {
        let input = lines(&["| H |", "| --- |", "| A |", "| B |", "| C |", "| D |"]);
        let moved = move_rows(&input, 1, 3, 5);
        let model = parse_table_block(&moved);
        assert_eq!(
            model.data_rows,
            vec![
                vec!["C".to_string()],
                vec!["D".to_string()],
                vec!["A".to_string()],
                vec!["B".to_string()]
            ]
        );
        let moved_up = move_rows(&input, 2, 4, 1);
        let model = parse_table_block(&moved_up);
        assert_eq!(
            model.data_rows,
            vec![
                vec!["B".to_string()],
                vec!["C".to_string()],
                vec!["A".to_string()],
                vec!["D".to_string()]
            ]
        );
    }

    #[test]
    fn selected_columns_move_as_a_block_and_keep_alignment() {
        let input = lines(&[
            "| A | B | C | D |",
            "| --- | :---: | ---: | --- |",
            "| 1 | 2 | 3 | 4 |",
        ]);
        let moved = move_columns(&input, 1, 3, 4);
        let model = parse_table_block(&moved);
        assert_eq!(model.header_cells, vec!["A", "D", "B", "C"]);
        assert_eq!(model.data_rows[0], vec!["1", "4", "2", "3"]);
        assert_eq!(
            model.aligns,
            vec![
                TableAlign::Left,
                TableAlign::Left,
                TableAlign::Center,
                TableAlign::Right
            ]
        );
    }

    #[test]
    fn selected_rows_and_columns_can_be_replaced_or_deleted() {
        let input = lines(&[
            "| A | B | C |",
            "| --- | --- | --- |",
            "| 1 | 2 | 3 |",
            "| 4 | 5 | 6 |",
        ]);
        let replaced_rows =
            replace_rows(&input, 1, &[vec!["x".into(), "y".into(), "z".into()]]).unwrap();
        assert_eq!(
            parse_table_block(&replaced_rows).data_rows[0],
            vec!["x", "y", "z"]
        );
        let replaced_columns = replace_columns(
            &input,
            1,
            &[vec!["Header".into(), "top".into(), "bottom".into()]],
        )
        .unwrap();
        let model = parse_table_block(&replaced_columns);
        assert_eq!(model.header_cells[1], "Header");
        assert_eq!(model.data_rows[1][1], "bottom");
        let deleted_rows = delete_rows(&input, 1, 2);
        assert_eq!(
            parse_table_block(&deleted_rows).data_rows,
            vec![vec!["4", "5", "6"]]
        );
        let deleted_columns = delete_columns(&input, 1, 3);
        assert_eq!(parse_table_block(&deleted_columns).header_cells, vec!["A"]);
        assert!(replace_rows(&input, 2, &[vec!["too".into()]]).is_none());
    }

    #[test]
    fn selected_row_and_column_matrix_copies_as_tsv() {
        let input = lines(&[
            "| A | B | C |",
            "| --- | --- | --- |",
            "| 1 | 2 | 3 |",
            "| 4 | 5 | 6 |",
        ]);
        assert_eq!(
            selected_table_matrix_to_tsv(&input, false, &[1, 2]),
            "1\t2\t3\n4\t5\t6"
        );
        assert_eq!(
            selected_table_matrix_to_tsv(&input, true, &[1, 2]),
            "B\tC\n2\t3\n5\t6"
        );
    }

    #[test]
    fn noncontiguous_row_column_selection_copies_deletes_and_moves_only_selected_items() {
        let input = lines(&[
            "| A | B | C | D |",
            "| --- | --- | --- | --- |",
            "| a1 | b1 | c1 | d1 |",
            "| a2 | b2 | c2 | d2 |",
            "| a3 | b3 | c3 | d3 |",
        ]);
        assert_eq!(
            selected_table_matrix_to_tsv(&input, false, &[1, 3]),
            "a1\tb1\tc1\td1\na3\tb3\tc3\td3"
        );
        assert_eq!(
            selected_table_matrix_to_tsv(&input, true, &[1, 3]),
            "B\tD\nb1\td1\nb2\td2\nb3\td3"
        );
        let deleted_rows = parse_table_block(&delete_rows_at(&input, &[1, 3]));
        assert_eq!(
            deleted_rows
                .data_rows
                .iter()
                .map(|row| row[0].as_str())
                .collect::<Vec<_>>(),
            vec!["a2"]
        );
        let deleted_columns = parse_table_block(&delete_columns_at(&input, &[1, 3]));
        assert_eq!(deleted_columns.header_cells, vec!["A", "C"]);
        assert_eq!(deleted_columns.data_rows[1], vec!["a2", "c2"]);
        let moved_rows = parse_table_block(&move_selected_rows(&input, &[1, 3], 5));
        assert_eq!(
            moved_rows
                .data_rows
                .iter()
                .map(|row| row[0].as_str())
                .collect::<Vec<_>>(),
            vec!["a2", "a1", "a3"]
        );
        let moved_columns = parse_table_block(&move_selected_columns(&input, &[1, 3], 4));
        assert_eq!(moved_columns.header_cells, vec!["A", "C", "B", "D"]);
        assert_eq!(moved_columns.data_rows[0], vec!["a1", "c1", "b1", "d1"]);
    }

    #[test]
    fn inserting_clipboard_matrix_keeps_unselected_rows_and_columns() {
        let input = lines(&[
            "| A | B | C |",
            "| --- | --- | --- |",
            "| a1 | b1 | c1 |",
            "| a2 | b2 | c2 |",
        ]);
        let inserted_rows =
            parse_table_block(&insert_rows_at(&input, 2, &[vec!["new".into()]]).unwrap());
        assert_eq!(
            inserted_rows.data_rows,
            vec![
                vec!["a1", "b1", "c1"],
                vec!["new", "", ""],
                vec!["a2", "b2", "c2"],
            ]
        );
        let inserted_columns = parse_table_block(
            &insert_columns_at(
                &input,
                1,
                &[vec!["X".into(), "Y".into()], vec!["x1".into(), "y1".into()]],
            )
            .unwrap(),
        );
        assert_eq!(inserted_columns.header_cells, vec!["A", "X", "Y", "B", "C"]);
        assert_eq!(
            inserted_columns.data_rows[0],
            vec!["a1", "x1", "y1", "b1", "c1"]
        );
        assert_eq!(
            inserted_columns.data_rows[1],
            vec!["a2", "", "", "b2", "c2"]
        );
        let overwritten = parse_table_block(
            &replace_rows_at(
                &input,
                &[1, 2],
                &[
                    vec!["x".into(), "y".into(), "z".into()],
                    vec!["p".into(), "q".into(), "r".into()],
                ],
            )
            .unwrap(),
        );
        assert_eq!(overwritten.header_cells, vec!["A", "B", "C"]);
        assert_eq!(overwritten.data_rows[0], vec!["x", "y", "z"]);
        assert_eq!(overwritten.data_rows[1], vec!["p", "q", "r"]);
        assert!(replace_rows_at(&input, &[1, 2], &[vec!["x".into()]]).is_none());
    }

    #[test]
    fn test_update_cell() {
        let input = lines(&["| A | B |", "| --- | --- |", "| old | val |"]);
        let result = update_cell(&input, 1, 0, "new");
        let model = parse_table_block(&result);
        assert_eq!(model.data_rows[0][0], "new");
    }

    #[test]
    fn test_update_header_cell() {
        let input = lines(&["| A | B |", "| --- | --- |", "| 1 | 2 |"]);
        let result = update_cell(&input, 0, 1, "NewB");
        let model = parse_table_block(&result);
        assert_eq!(model.header_cells[1], "NewB");
    }

    #[test]
    fn test_empty_cells() {
        let input = lines(&["| A |  | C |", "| --- | --- | --- |", "|  |  |  |"]);
        let model = parse_table_block(&input);
        assert_eq!(model.col_count, 3);
        assert_eq!(model.header_cells[1], "");
        assert_eq!(model.data_rows[0], vec!["", "", ""]);
    }
}
