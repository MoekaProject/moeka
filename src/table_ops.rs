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

fn cell_display_width(text: &str) -> usize {
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

    let mut result = vec![
        serialize_row(&header, &widths),
        serialize_align_row(&aligns, &widths),
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

pub fn set_column_alignment(
    lines: &[String],
    col_idx: usize,
    alignment: TableAlign,
) -> Vec<String> {
    let mut model = parse_table_block(lines);
    if col_idx < model.col_count {
        model.aligns[col_idx] = alignment;
    }
    serialize_table(&model)
}

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
