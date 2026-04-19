use crate::markdown::TableAlign;

#[derive(Debug, Clone)]
pub struct TableModel {
    pub col_count: usize,
    pub header_cells: Vec<String>,
    pub aligns: Vec<TableAlign>,
    pub data_rows: Vec<Vec<String>>,
}

fn parse_row(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    let inner = if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    inner.split('|').map(|s| s.trim().to_string()).collect()
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

fn serialize_row(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

fn serialize_align_row(aligns: &[TableAlign]) -> String {
    let cells: Vec<String> = aligns
        .iter()
        .map(|a| match a {
            TableAlign::Left => "---".to_string(),
            TableAlign::Center => ":---:".to_string(),
            TableAlign::Right => "---:".to_string(),
        })
        .collect();
    format!("| {} |", cells.join(" | "))
}

pub fn serialize_table(model: &TableModel) -> Vec<String> {
    let col_count = model.col_count.max(model.header_cells.len()).max(1);
    let header = pad_cells(&model.header_cells, col_count);
    let mut aligns = model.aligns.clone();
    aligns.resize(col_count, TableAlign::Left);

    let mut result = vec![serialize_row(&header), serialize_align_row(&aligns)];
    for row in &model.data_rows {
        result.push(serialize_row(&pad_cells(row, col_count)));
    }
    result
}

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
    let row_count = model.data_rows.len();
    let clamped = insert_at.min(row_count);
    let empty_row = vec![String::new(); model.col_count];
    model.data_rows.insert(clamped, empty_row);
    serialize_table(&model)
}

pub fn delete_row(lines: &[String], row_index: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    if row_index < model.data_rows.len() {
        model.data_rows.remove(row_index);
    }
    serialize_table(&model)
}

pub fn move_row(lines: &[String], from: usize, to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    if from >= model.data_rows.len() {
        return serialize_table(&model);
    }
    let row = model.data_rows.remove(from);
    let adjusted_to = if to > from { to.saturating_sub(1) } else { to };
    let clamped = adjusted_to.min(model.data_rows.len());
    model.data_rows.insert(clamped, row);
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

pub fn move_column(lines: &[String], from: usize, to: usize) -> Vec<String> {
    let mut model = parse_table_block(lines);
    if from >= model.col_count || to >= model.col_count || from == to {
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
        assert_eq!(output[0], "| Name | Age |");
        assert_eq!(output[1], "| --- | --- |");
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
    fn test_insert_row() {
        let input = lines(&["| H1 | H2 |", "| --- | --- |", "| A | B |"]);
        let result = insert_row(&input, 0);
        let model = parse_table_block(&result);
        assert_eq!(model.data_rows.len(), 2);
        assert_eq!(model.data_rows[0], vec!["", ""]);
        assert_eq!(model.data_rows[1], vec!["A", "B"]);
    }

    #[test]
    fn test_delete_row() {
        let input = lines(&["| H1 | H2 |", "| --- | --- |", "| A | B |", "| C | D |"]);
        let result = delete_row(&input, 0);
        let model = parse_table_block(&result);
        assert_eq!(model.data_rows.len(), 1);
        assert_eq!(model.data_rows[0], vec!["C", "D"]);
    }

    #[test]
    fn test_move_row() {
        let input = lines(&["| H |", "| --- |", "| A |", "| B |", "| C |"]);
        let result = move_row(&input, 0, 2);
        let model = parse_table_block(&result);
        assert_eq!(model.data_rows[0], vec!["B"]);
        assert_eq!(model.data_rows[1], vec!["A"]);
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
