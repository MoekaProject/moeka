use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use gpui::{
    Bounds, Font, FontStyle, FontWeight, Hsla, LineLayout, PaintQuad, Pixels, ShapedGlyph,
    ShapedRun, SharedString, StrikethroughStyle, TextRun, Window, WrappedLine, WrappedLineLayout,
    fill, point, px,
};

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, ScopeSelectors, StyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::SyntaxSet;

use image::Frame;

use crate::markdown::{self, BlockType, InlineKind, LineDecoration, TableLineInfo};
use crate::theme::Theme;

use super::*;

/// Calculate the display width of a string, counting CJK characters as 2 columns.
pub(crate) fn display_width(s: &str) -> usize {
    s.chars().map(|ch| char_display_width(ch)).sum()
}

fn char_display_width(ch: char) -> usize {
    let code = ch as u32;
    if (0x1100..=0x115F).contains(&code)
        || (0x2E80..=0x303E).contains(&code)
        || (0x3040..=0x33BF).contains(&code)
        || (0x3400..=0x4DBF).contains(&code)
        || (0x4E00..=0x9FFF).contains(&code)
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
}

fn prefers_latin_text_font(ch: char) -> bool {
    matches!(
        ch,
        '\u{0000}'..='\u{024F}'
            | '\u{1E00}'..='\u{1EFF}'
            | '\u{2000}'..='\u{206F}'
            | '\u{20A0}'..='\u{20CF}'
    )
}

/// Build display text by selectively hiding syntax ranges.
pub(crate) fn build_display_text(
    line_content: &str,
    decoration: &LineDecoration,
    cursor_col: Option<usize>,
    code_block_syntax_active: bool,
    table_width_chars: Option<usize>,
    table_cell_padding_chars: usize,
    collapsed_task_prefix_chars: usize,
    task_parent_prefix_chars: usize,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    let len = line_content.len();

    if decoration.block_type == BlockType::Table {
        if let Some(ref table_info) = decoration.table_info {
            return build_table_display_text(
                line_content,
                table_info,
                cursor_col,
                code_block_syntax_active,
                table_width_chars,
                table_cell_padding_chars,
                collapsed_task_prefix_chars,
                task_parent_prefix_chars,
            );
        }
    }

    if decoration.block_type == BlockType::CodeBlockFence {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let mut buffer_to_display: Vec<usize> = (0..len).collect();
        buffer_to_display.push(len);
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    if decoration.syntax_ranges.is_empty() {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let mut buffer_to_display: Vec<usize> = (0..len).collect();
        buffer_to_display.push(len);
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    let hidden: Vec<bool> = decoration
        .syntax_ranges
        .iter()
        .map(|sr| {
            if decoration.block_type == BlockType::CodeBlockFence && code_block_syntax_active {
                return false;
            }
            cursor_col.map_or(true, |col| {
                let at_eol = col >= len;
                // For headings: always expand syntax when cursor is on the line and not at EOL;
                // collapse when cursor is at EOL or selection merely passes through.
                if matches!(decoration.block_type, BlockType::Heading(_)) {
                    return at_eol;
                }
                if at_eol {
                    !matches!(
                        decoration.block_type,
                        BlockType::HorizontalRule | BlockType::CodeBlockFence
                    )
                } else {
                    col < sr.trigger_range.start || col >= sr.trigger_range.end
                }
            })
        })
        .collect();

    if !hidden.iter().any(|&h| h) {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let mut buffer_to_display: Vec<usize> = (0..len).collect();
        buffer_to_display.push(len);
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    let mut is_hidden = vec![false; len];
    for (i, sr) in decoration.syntax_ranges.iter().enumerate() {
        if hidden[i] {
            let start = sr.range.start.min(len);
            let end = sr.range.end.min(len);
            for j in start..end {
                is_hidden[j] = true;
            }
        }
    }

    let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
    let trimmed = &line_content[leading_ws..];

    let prefix_range_and_replacement = match decoration.block_type {
        BlockType::UnorderedList => {
            let fold_len = if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                2
            } else {
                0
            };
            (fold_len > 0 && prefix_is_folded(leading_ws, fold_len, cursor_col))
                .then_some((leading_ws..leading_ws + fold_len, "• ".to_string()))
        }
        BlockType::OrderedList => {
            let prefix_len = markdown::ordered_list_prefix_len(trimmed);
            (prefix_len > 0 && prefix_is_folded(leading_ws, prefix_len, cursor_col)).then_some((
                leading_ws..leading_ws + prefix_len,
                trimmed[..prefix_len].to_string(),
            ))
        }
        BlockType::TaskList(_) => {
            let fold_len = task_prefix_len(trimmed);
            let trigger_len = task_syntax_len(trimmed);
            // Absorb parent-prefix indent so sub-task checkbox aligns
            // with parent's text start. Keep collapsed placeholder width.
            let indent_reduction = leading_ws.min(task_parent_prefix_chars);
            let reduced_indent = leading_ws - indent_reduction;
            let replacement = " ".repeat(reduced_indent + collapsed_task_prefix_chars.max(1));
            (fold_len > 0 && prefix_is_folded(leading_ws, trigger_len, cursor_col))
                .then_some((0..leading_ws + fold_len, replacement))
        }
        _ => None,
    };

    let prefix_collapsed = prefix_range_and_replacement.is_some();

    let mut display_text = String::with_capacity(len);
    let mut display_to_buffer = Vec::with_capacity(len);
    let mut buffer_to_display = vec![0usize; len];
    let mut display_pos = 0;

    if let Some((prefix_range, replacement)) = &prefix_range_and_replacement {
        for (buf_pos, ch) in line_content[..prefix_range.start].char_indices() {
            let ch_len = ch.len_utf8();
            display_text.push(ch);
            for offset in 0..ch_len {
                display_to_buffer.push(buf_pos + offset);
                buffer_to_display[buf_pos + offset] = display_pos + offset;
            }
            display_pos += ch_len;
        }

        let collapsed_display_start = display_pos;
        display_text.push_str(replacement);
        for _ in 0..replacement.len() {
            display_to_buffer.push(prefix_range.start);
        }
        for buf_i in prefix_range.start..prefix_range.end.min(len) {
            buffer_to_display[buf_i] = collapsed_display_start;
        }
        display_pos += replacement.len();
    }

    for (buf_pos, ch) in line_content.char_indices() {
        if let Some((prefix_range, _)) = &prefix_range_and_replacement {
            if buf_pos < prefix_range.end {
                continue;
            }
        }

        let ch_len = ch.len_utf8();
        if is_hidden[buf_pos] {
            for offset in 0..ch_len {
                if buf_pos + offset < len {
                    buffer_to_display[buf_pos + offset] = display_pos;
                }
            }
        } else {
            display_text.push(ch);
            for offset in 0..ch_len {
                display_to_buffer.push(buf_pos + offset);
                if buf_pos + offset < len {
                    buffer_to_display[buf_pos + offset] = display_pos + offset;
                }
            }
            display_pos += ch_len;
        }
    }

    buffer_to_display.push(display_text.len());

    (
        display_text,
        display_to_buffer,
        buffer_to_display,
        prefix_collapsed,
    )
}

/// Build display text for a folded table row.
pub(crate) fn build_table_display_text(
    line_content: &str,
    info: &TableLineInfo,
    _cursor_col: Option<usize>,
    _code_block_syntax_active: bool,
    available_chars: Option<usize>,
    table_cell_padding_chars: usize,
    collapsed_task_prefix_chars: usize,
    task_parent_prefix_chars: usize,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    let len = line_content.len();
    let cell_padding = table_cell_padding_chars;
    let balanced_col_widths = compute_table_render_widths(info, available_chars, cell_padding);
    let target_row_chars = available_chars.unwrap_or_else(|| {
        balanced_col_widths
            .iter()
            .map(|width| width + cell_padding * 2)
            .sum::<usize>()
            .max(1)
    });

    if info.is_separator {
        let display = " ".repeat(target_row_chars.max(1));
        let d2b = vec![0; display.len()];
        let mut b2d = vec![0; len];
        b2d.push(display.len());
        return (display, d2b, b2d, false);
    }

    let mut display = String::new();
    let mut d2b: Vec<usize> = Vec::new();
    let mut b2d = vec![0usize; len];
    let mut consumed = 0usize;
    let mut display_pos = 0usize;

    for (ci, cell) in info.cells.iter().enumerate() {
        let cell_range = info
            .cell_ranges
            .get(ci)
            .cloned()
            .unwrap_or(consumed..consumed);

        for buf_i in consumed..cell_range.start.min(len) {
            b2d[buf_i] = display_pos;
        }

        let cell_decoration = markdown::parse_decorations(cell)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (folded_cell_raw, cell_d2b_raw, cell_b2d_raw, _) = build_display_text(
            cell,
            &cell_decoration,
            None,
            false,
            None,
            0,
            collapsed_task_prefix_chars,
            task_parent_prefix_chars,
        );
        let trimmed_start = folded_cell_raw.len() - folded_cell_raw.trim_start().len();
        let trimmed_end = folded_cell_raw.len() - folded_cell_raw.trim_end().len();
        let folded_cell =
            folded_cell_raw[trimmed_start..folded_cell_raw.len() - trimmed_end].to_string();
        let cell_d2b: Vec<usize> =
            cell_d2b_raw[trimmed_start..cell_d2b_raw.len() - trimmed_end].to_vec();
        let cell_b2d: Vec<usize> = cell_b2d_raw
            .iter()
            .map(|&d| d.saturating_sub(trimmed_start))
            .collect();
        let cell_content_chars = display_width(&folded_cell);
        let width = balanced_col_widths
            .get(ci)
            .copied()
            .unwrap_or(cell_content_chars)
            .max(cell_content_chars);
        let left_padding = " ".repeat(cell_padding);
        let right_padding_len = width.saturating_sub(cell_content_chars) + cell_padding;
        let right_padding = " ".repeat(right_padding_len);
        let cell_anchor = cell_range.start.min(len.saturating_sub(1));

        display.push_str(&left_padding);
        for _ in 0..left_padding.len() {
            d2b.push(cell_anchor);
        }
        display_pos += left_padding.len();

        display.push_str(&folded_cell);
        for &local_buf in &cell_d2b {
            d2b.push((cell_range.start + local_buf).min(len.saturating_sub(1)));
        }
        for (local_buf, &local_display) in cell_b2d.iter().enumerate() {
            let global = cell_range.start + local_buf;
            if global < len {
                b2d[global] = display_pos + local_display;
            }
        }
        display_pos += folded_cell.len();

        display.push_str(&right_padding);
        let right_anchor = cell_range.end.min(len.saturating_sub(1));
        for _ in 0..right_padding.len() {
            d2b.push(right_anchor);
        }
        display_pos += right_padding.len();

        consumed = cell_range.end.min(len);
    }

    for buf_i in consumed..len {
        b2d[buf_i] = display_pos;
    }

    b2d.push(display.len());

    (display, d2b, b2d, false)
}

pub(crate) fn compute_table_render_widths(
    info: &TableLineInfo,
    available_chars: Option<usize>,
    table_cell_padding_chars: usize,
) -> Vec<usize> {
    let mut widths = info.column_widths.clone();
    let Some(available_chars) = available_chars else {
        return widths;
    };

    if widths.is_empty() {
        return widths;
    }

    let padding_budget = table_cell_padding_chars
        .saturating_mul(2)
        .saturating_mul(widths.len());
    let min_width = 3usize;
    let min_required = min_width
        .saturating_mul(widths.len())
        .saturating_add(padding_budget);
    if available_chars <= padding_budget {
        return vec![1; widths.len()];
    }
    if available_chars <= min_required {
        let content_budget = available_chars
            .saturating_sub(padding_budget)
            .max(widths.len());
        let base = (content_budget / widths.len()).max(1);
        let mut remainder = content_budget.saturating_sub(base * widths.len());
        for width in &mut widths {
            *width = base;
            if remainder > 0 {
                *width += 1;
                remainder -= 1;
            }
        }
        return widths;
    }

    let content_budget = available_chars.saturating_sub(padding_budget);
    let current_total: usize = widths.iter().sum();
    if current_total <= content_budget {
        return widths;
    }

    widths.fill(min_width);
    let mut remaining = content_budget.saturating_sub(min_width * widths.len());
    let desired_extras: Vec<usize> = info
        .column_widths
        .iter()
        .map(|width| width.saturating_sub(min_width))
        .collect();

    while remaining > 0 {
        let mut made_progress = false;
        for (width, desired_extra) in widths.iter_mut().zip(desired_extras.iter()) {
            if width.saturating_sub(min_width) < *desired_extra {
                *width += 1;
                remaining -= 1;
                made_progress = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if !made_progress {
            let mut index = 0usize;
            let width_count = widths.len();
            while remaining > 0 {
                widths[index % width_count] += 1;
                remaining -= 1;
                index += 1;
            }
        }
    }

    widths
}

/// Layout information for a table rendered as a block element.
#[derive(Debug, Clone)]
pub(crate) struct TableCellLayout {
    /// Pixel bounds of the cell (relative to table origin).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Row index (0 = header, 1+ = data rows; separator rows skipped in visual layout).
    pub visual_row: usize,
    /// Column index.
    pub col: usize,
    /// The display text for this cell (with inline markdown folded).
    pub display_text: String,
    /// Mapping from display offset → buffer offset within the cell.
    pub display_to_buffer: Vec<usize>,
    /// Mapping from buffer offset → display offset within the cell.
    pub buffer_to_display: Vec<usize>,
    /// The original cell byte range within its source line.
    pub cell_range: Range<usize>,
    /// Source line index in the buffer.
    pub source_line: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TableLayout {
    /// Total pixel width of the table.
    pub total_width: f32,
    /// Total pixel height of the table (excluding handles).
    pub total_height: f32,
    /// Pixel widths for each column.
    pub col_widths: Vec<f32>,
    /// Pixel heights for each visual row.
    pub row_heights: Vec<f32>,
    /// Cell layouts indexed as [visual_row][col].
    pub cells: Vec<Vec<TableCellLayout>>,
    /// Number of visual rows (header + data rows, separator excluded).
    pub visual_row_count: usize,
    /// Number of columns.
    pub col_count: usize,
    /// The handle area size in pixels.
    pub handle_size: f32,
    /// Cell padding in pixels (horizontal, each side).
    pub cell_padding_x: f32,
    /// Cell padding in pixels (vertical, each side).
    pub cell_padding_y: f32,
    /// Maps visual_row → source line index in the buffer.
    pub visual_row_to_line: Vec<usize>,
    /// The cell text color for styling.
    pub is_header: Vec<bool>,
}

/// Compute the full table layout from decoration data of all lines in the table block.
/// - `decorations`: all parsed line decorations
/// - `table_start`, `table_end`: line range of the table
/// - `available_width`: pixel width available for the table
/// - `cell_padding_x`, `cell_padding_y`: pixel padding per cell side
/// - `base_char_width`: width of a monospace character in pixels
/// - `line_height`: text line height in pixels
/// - `handle_size`: drag-handle area size in pixels
pub(crate) fn compute_table_layout(
    decorations: &[LineDecoration],
    table_start: usize,
    table_end: usize,
    available_width: f32,
    cell_padding_x: f32,
    cell_padding_y: f32,
    base_char_width: f32,
    line_height: f32,
    handle_size: f32,
) -> TableLayout {
    // Collect non-separator rows with their line indices
    let mut visual_rows: Vec<(usize, &TableLineInfo)> = Vec::new();
    for line_idx in table_start..table_end {
        if let Some(ref ti) = decorations
            .get(line_idx)
            .and_then(|d| d.table_info.as_ref())
        {
            if !ti.is_separator {
                visual_rows.push((line_idx, ti));
            }
        }
    }

    let col_count = visual_rows
        .first()
        .map(|(_, ti)| ti.cells.len())
        .unwrap_or(1);

    // Measure desired column widths based on content
    let mut desired_col_widths_chars = vec![3usize; col_count];
    for (_, ti) in &visual_rows {
        for (ci, cell) in ti.cells.iter().enumerate() {
            if ci < col_count {
                desired_col_widths_chars[ci] =
                    desired_col_widths_chars[ci].max(display_width(cell.trim()));
            }
        }
    }

    // Convert to pixel widths, respecting available_width
    let total_padding = cell_padding_x * 2.0 * col_count as f32;
    let content_width = (available_width - total_padding).max(col_count as f32 * 28.0);
    let desired_total: f32 = desired_col_widths_chars
        .iter()
        .map(|&w| w as f32 * base_char_width)
        .sum();

    let col_widths: Vec<f32> = if desired_total <= content_width {
        // Enough space: use desired widths + distribute remaining space
        let extra = content_width - desired_total;
        let extra_per_col = extra / col_count as f32;
        desired_col_widths_chars
            .iter()
            .map(|&w| w as f32 * base_char_width + extra_per_col)
            .collect()
    } else {
        let min_col = 28.0_f32;
        let mut widths: Vec<f32> = desired_col_widths_chars
            .iter()
            .map(|&w| (w as f32 * base_char_width).max(min_col))
            .collect();

        let mut unresolved: Vec<usize> = (0..col_count).collect();
        let mut remaining_width = content_width;
        loop {
            if unresolved.is_empty() {
                break;
            }

            let avg_width = (remaining_width / unresolved.len() as f32).max(min_col);
            let mut changed = false;
            let mut next_unresolved = Vec::new();

            for &index in &unresolved {
                if widths[index] <= avg_width {
                    remaining_width -= widths[index];
                    changed = true;
                } else {
                    next_unresolved.push(index);
                }
            }

            if !changed {
                for &index in &unresolved {
                    widths[index] = avg_width;
                }
                break;
            }

            unresolved = next_unresolved;
        }

        let actual_total: f32 = widths.iter().sum();
        if actual_total > content_width {
            let excess = actual_total - content_width;
            let shrinkable: Vec<usize> = widths
                .iter()
                .enumerate()
                .filter_map(|(index, width)| (*width > min_col + 0.1).then_some(index))
                .collect();
            let shrinkable_total: f32 = shrinkable
                .iter()
                .map(|&index| widths[index] - min_col)
                .sum();
            if shrinkable_total > 0.0 {
                for index in shrinkable {
                    let share = (widths[index] - min_col) / shrinkable_total;
                    widths[index] = (widths[index] - excess * share).max(min_col);
                }
            }
        }

        widths
    };

    let row_height = line_height + cell_padding_y * 2.0;
    let row_heights = vec![row_height; visual_rows.len()];
    let total_width = col_widths.iter().sum::<f32>() + total_padding;
    let total_height = row_height * visual_rows.len() as f32;

    // Build cell layouts
    let mut all_cells: Vec<Vec<TableCellLayout>> = Vec::new();
    let mut visual_row_to_line = Vec::new();
    let mut is_header_vec = Vec::new();

    let mut current_y = 0.0_f32;
    for (vi, (line_idx, ti)) in visual_rows.iter().enumerate() {
        visual_row_to_line.push(*line_idx);
        is_header_vec.push(ti.is_header);
        let mut row_cells = Vec::new();
        let mut current_x = 0.0_f32;
        for ci in 0..col_count {
            let cell_content = ti.cells.get(ci).cloned().unwrap_or_default();
            let cell_range = ti.cell_ranges.get(ci).cloned().unwrap_or(0..0);

            // Build display text for cell with inline markdown folding
            let cell_decoration = markdown::parse_decorations(&cell_content)
                .into_iter()
                .next()
                .unwrap_or_default();
            let (folded_raw, d2b_raw, b2d_raw, _) =
                build_display_text(&cell_content, &cell_decoration, None, false, None, 0, 0, 0);
            let trimmed = folded_raw.trim();
            let trim_start = folded_raw.len() - folded_raw.trim_start().len();
            let trim_end = folded_raw.len() - folded_raw.trim_end().len();
            let display_text = trimmed.to_string();
            let d2b = d2b_raw[trim_start..d2b_raw.len() - trim_end].to_vec();
            let b2d = b2d_raw
                .iter()
                .map(|&d| d.saturating_sub(trim_start))
                .collect();

            let col_w = col_widths.get(ci).copied().unwrap_or(28.0);
            let cell_w = col_w + cell_padding_x * 2.0;
            row_cells.push(TableCellLayout {
                x: current_x,
                y: current_y,
                width: cell_w,
                height: row_height,
                visual_row: vi,
                col: ci,
                display_text,
                display_to_buffer: d2b,
                buffer_to_display: b2d,
                cell_range,
                source_line: *line_idx,
            });
            current_x += cell_w;
        }
        all_cells.push(row_cells);
        current_y += row_height;
    }

    TableLayout {
        total_width,
        total_height,
        col_widths,
        row_heights,
        cells: all_cells,
        visual_row_count: visual_rows.len(),
        col_count,
        handle_size,
        cell_padding_x,
        cell_padding_y,
        visual_row_to_line,
        is_header: is_header_vec,
    }
}

pub(crate) fn build_selection_quads(
    display_text: &str,
    runs: &[TextRun],
    wrapped: &WrappedLine,
    disp_start: usize,
    disp_end: usize,
    content_x: f32,
    line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    selection_color: Hsla,
    letter_spacing: f32,
    window: &mut Window,
) -> Vec<PaintQuad> {
    if disp_start >= disp_end {
        return Vec::new();
    }

    let mut quads = Vec::new();
    let mut segment_start = 0usize;
    for (segment_index, segment_end) in wrap_boundary_byte_indices(wrapped)
        .into_iter()
        .chain(std::iter::once(display_text.len()))
        .enumerate()
    {
        let overlap_start = disp_start.max(segment_start);
        let overlap_end = disp_end.min(segment_end);
        if overlap_start < overlap_end {
            let start_x = if overlap_start == segment_start {
                0.0
            } else {
                f32::from(
                    wrapped
                        .position_for_index(overlap_start, px(line_height))
                        .unwrap_or_default()
                        .x,
                )
            };

            let end_x = if overlap_end == segment_end {
                let segment_text = &display_text[segment_start..segment_end];
                let segment_runs = extract_runs_for_range(runs, segment_start, segment_end);
                let shared: SharedString = if segment_text.is_empty() {
                    " ".into()
                } else {
                    segment_text.to_string().into()
                };
                if let Ok(mut shaped) = window.text_system().shape_text(
                    shared,
                    px(font_size),
                    &segment_runs,
                    None,
                    None,
                ) {
                    if letter_spacing != 0.0 {
                        for w in shaped.iter_mut() {
                            apply_letter_spacing(w, letter_spacing);
                        }
                    }
                    shaped.first().map_or(0.0, |w| f32::from(w.width()))
                } else {
                    0.0
                }
            } else {
                f32::from(
                    wrapped
                        .position_for_index(overlap_end, px(line_height))
                        .unwrap_or_default()
                        .x,
                )
            };

            let indent = if segment_index > 0 { indent_px } else { 0.0 };
            let segment_y = line_top + segment_index as f32 * line_height;
            quads.push(fill(
                Bounds::from_corners(
                    point(px(content_x + indent + start_x), px(segment_y)),
                    point(px(content_x + indent + end_x), px(segment_y + line_height)),
                ),
                selection_color,
            ));
        }
        segment_start = segment_end;
    }

    quads
}

pub(crate) fn compute_image_preview_size(
    image_width: f32,
    image_height: f32,
    max_width: f32,
) -> (f32, f32) {
    if image_width <= 0.0 || image_height <= 0.0 || max_width <= 0.0 {
        return (0.0, 0.0);
    }

    let scale = max_width / image_width;
    (max_width, image_height * scale)
}

pub(crate) fn image_syntax_range(line_content: &str) -> Option<Range<usize>> {
    let start = line_content.find("![")?;
    let suffix = &line_content[start..];
    let end = suffix.find(')')? + start + 1;
    Some(start..end)
}

pub(crate) fn is_image_syntax_active(
    decoration: &LineDecoration,
    line_content: &str,
    line_start_offset: usize,
    cursor_offset: usize,
    selection: &Range<usize>,
) -> bool {
    if decoration.image_path.is_none() {
        return false;
    }

    let Some(local_range) = image_syntax_range(line_content) else {
        return false;
    };
    let absolute_range =
        (line_start_offset + local_range.start)..(line_start_offset + local_range.end);

    (cursor_offset >= absolute_range.start && cursor_offset <= absolute_range.end)
        || (selection.start < absolute_range.end && selection.end > absolute_range.start)
}

/// Extract TextRuns covering the byte range [seg_start..seg_end) from the original runs.
pub(crate) fn extract_runs_for_range(
    runs: &[TextRun],
    seg_start: usize,
    seg_end: usize,
) -> Vec<TextRun> {
    let mut result = Vec::new();
    let mut pos = 0;
    for run in runs {
        let run_end = pos + run.len;
        let overlap_start = seg_start.max(pos);
        let overlap_end = seg_end.min(run_end);
        if overlap_start < overlap_end {
            result.push(TextRun {
                len: overlap_end - overlap_start,
                ..run.clone()
            });
        }
        pos = run_end;
    }
    result
}

/// Get wrap boundary byte indices from the WrappedLine's shaped runs.
pub(crate) fn wrap_boundary_byte_indices(wrapped: &WrappedLine) -> Vec<usize> {
    let runs = wrapped.runs();
    wrapped
        .wrap_boundaries()
        .iter()
        .map(|wb| runs[wb.run_ix].glyphs[wb.glyph_ix].index)
        .collect()
}

/// Return indent_px if the position y indicates a continuation line (y > 0).
pub(crate) fn indent_for_y(indent_px: f32, y: Pixels, line_height: f32) -> f32 {
    if indent_px > 0.0 && f32::from(y) > line_height * 0.5 {
        indent_px
    } else {
        0.0
    }
}

/// Compute the display prefix byte count for list indent.
pub(crate) fn compute_display_prefix_bytes(
    display_text: &str,
    block_type: &BlockType,
    prefix_collapsed: bool,
    _collapsed_task_prefix_chars: usize,
) -> usize {
    let leading_ws = display_text.len() - display_text.trim_start_matches([' ', '\t']).len();
    let trimmed = &display_text[leading_ws..];
    match block_type {
        BlockType::UnorderedList => {
            if prefix_collapsed {
                leading_ws + "• ".len()
            } else if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                leading_ws + 2
            } else {
                0
            }
        }
        BlockType::TaskList(_) => {
            if prefix_collapsed {
                leading_ws
            } else {
                trimmed.find("] ").map(|p| leading_ws + p + 2).unwrap_or(0)
            }
        }
        BlockType::OrderedList => {
            if prefix_collapsed {
                trimmed
                    .find(". ")
                    .map(|p| leading_ws + p + 2)
                    .unwrap_or(leading_ws + trimmed.len().min(4))
            } else {
                trimmed.find(". ").map(|p| leading_ws + p + 2).unwrap_or(0)
            }
        }
        _ => 0,
    }
}

pub(crate) fn prefix_is_folded(
    leading_ws: usize,
    prefix_len: usize,
    cursor_col: Option<usize>,
) -> bool {
    cursor_col.is_none_or(|col| col < leading_ws || col >= leading_ws + prefix_len)
}

fn task_prefix_len(trimmed: &str) -> usize {
    for marker in ["- [ ] ", "- [x] ", "* [ ] ", "* [x] ", "+ [ ] ", "+ [x] "] {
        if trimmed.starts_with(marker) {
            return marker.len();
        }
    }
    0
}

fn task_syntax_len(trimmed: &str) -> usize {
    task_prefix_len(trimmed)
}

pub(crate) fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

pub(crate) fn syntect_theme() -> &'static SyntectTheme {
    static THEME: OnceLock<SyntectTheme> = OnceLock::new();
    THEME.get_or_init(|| build_one_dark_pro_theme())
}

/// Build a One Dark Pro style syntect theme with comprehensive scope coverage.
fn build_one_dark_pro_theme() -> SyntectTheme {
    // One Dark Pro palette
    let chalky = SyntectColor {
        r: 0xe5,
        g: 0xc0,
        b: 0x7b,
        a: 0xff,
    }; // #e5c07b — yellow
    let coral = SyntectColor {
        r: 0xe0,
        g: 0x6c,
        b: 0x75,
        a: 0xff,
    }; // #e06c75 — red
    let cyan = SyntectColor {
        r: 0x56,
        g: 0xb6,
        b: 0xc2,
        a: 0xff,
    }; // #56b6c2 — cyan
    let fountain_blue = SyntectColor {
        r: 0x61,
        g: 0xaf,
        b: 0xef,
        a: 0xff,
    }; // #61afef — blue
    let green = SyntectColor {
        r: 0x98,
        g: 0xc3,
        b: 0x79,
        a: 0xff,
    }; // #98c379 — green
    let ivory = SyntectColor {
        r: 0xab,
        g: 0xb2,
        b: 0xbf,
        a: 0xff,
    }; // #abb2bf — text
    let stone = SyntectColor {
        r: 0x7f,
        g: 0x84,
        b: 0x8e,
        a: 0xff,
    }; // #7f848e — comment
    let violet = SyntectColor {
        r: 0xc6,
        g: 0x78,
        b: 0xdd,
        a: 0xff,
    }; // #c678dd — purple
    let whiskey = SyntectColor {
        r: 0xd1,
        g: 0x9a,
        b: 0x66,
        a: 0xff,
    }; // #d19a66 — orange

    let bg = SyntectColor {
        r: 0x28,
        g: 0x2c,
        b: 0x34,
        a: 0xff,
    };

    fn rule(scope: &str, fg: SyntectColor) -> ThemeItem {
        ThemeItem {
            scope: scope
                .parse::<ScopeSelectors>()
                .unwrap_or_else(|_| ScopeSelectors { selectors: vec![] }),
            style: StyleModifier {
                foreground: Some(fg),
                background: None,
                font_style: None,
            },
        }
    }

    fn rule_italic(scope: &str, fg: SyntectColor) -> ThemeItem {
        ThemeItem {
            scope: scope
                .parse::<ScopeSelectors>()
                .unwrap_or_else(|_| ScopeSelectors { selectors: vec![] }),
            style: StyleModifier {
                foreground: Some(fg),
                background: None,
                font_style: Some(syntect::highlighting::FontStyle::ITALIC),
            },
        }
    }

    fn rule_bold(scope: &str, fg: SyntectColor) -> ThemeItem {
        ThemeItem {
            scope: scope
                .parse::<ScopeSelectors>()
                .unwrap_or_else(|_| ScopeSelectors { selectors: vec![] }),
            style: StyleModifier {
                foreground: Some(fg),
                background: None,
                font_style: Some(syntect::highlighting::FontStyle::BOLD),
            },
        }
    }

    SyntectTheme {
        name: Some("One Dark Pro".to_string()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(ivory),
            background: Some(bg),
            ..ThemeSettings::default()
        },
        scopes: vec![
            // ── Comments ────────────────────────────────────────
            rule_italic("comment, punctuation.definition.comment", stone),
            // ── Strings ─────────────────────────────────────────
            rule(
                "string, string.quoted, string.template, string.interpolated",
                green,
            ),
            rule("string.regexp", coral),
            // ── Constants / Numbers ─────────────────────────────
            rule(
                "constant.numeric, constant.language, constant.character",
                whiskey,
            ),
            rule("constant.other, constant.other.color", whiskey),
            // ── Variables ───────────────────────────────────────
            rule("variable, variable.other, variable.parameter", coral),
            rule("variable.language", coral),
            rule(
                "variable.other.constant, variable.other.enummember",
                whiskey,
            ),
            // ── Keywords ────────────────────────────────────────
            rule(
                "keyword, keyword.control, keyword.operator.new, keyword.operator.expression, keyword.operator.logical, keyword.operator.delete, keyword.operator.typeof, keyword.operator.void, keyword.operator.instanceof, keyword.operator.in",
                violet,
            ),
            rule(
                "keyword.operator, keyword.operator.assignment, keyword.operator.arithmetic, keyword.operator.comparison, keyword.operator.ternary",
                cyan,
            ),
            // ── Storage / Types ─────────────────────────────────
            rule("storage, storage.type, storage.modifier", violet),
            rule(
                "support.type, support.class, entity.name.type, entity.name.namespace",
                chalky,
            ),
            rule("entity.other.inherited-class", chalky),
            // ── Functions ───────────────────────────────────────
            rule(
                "entity.name.function, support.function, meta.function-call",
                fountain_blue,
            ),
            rule("variable.function", fountain_blue),
            // ── Classes / Entities ──────────────────────────────
            rule("entity.name.class, entity.name.type.class", chalky),
            rule("entity.name.tag", coral),
            rule("entity.other.attribute-name", whiskey),
            // ── Punctuation ─────────────────────────────────────
            rule("punctuation, meta.brace, meta.delimiter", ivory),
            rule(
                "punctuation.definition.string, punctuation.definition.template-expression",
                green,
            ),
            rule("punctuation.definition.tag", coral),
            rule("punctuation.section.embedded", coral),
            // ── Support ─────────────────────────────────────────
            rule("support.constant", whiskey),
            rule("support.variable", coral),
            rule("support.type.property-name", coral),
            // ── Meta ────────────────────────────────────────────
            rule("meta.object-literal.key", coral),
            // ─── Shell / Bash ───────────────────────────────────
            rule("source.shell variable, source.shell variable.other", coral),
            rule(
                "source.shell keyword.operator, source.shell keyword.operator.pipe, source.shell keyword.operator.redirect, source.shell keyword.operator.logical",
                cyan,
            ),
            rule("source.shell support.function", fountain_blue),
            rule("source.shell entity.name.function", fountain_blue),
            rule("source.shell keyword.control", violet),
            rule(
                "source.shell string.quoted, source.shell string.unquoted",
                green,
            ),
            rule("source.shell punctuation.definition.variable", coral),
            rule(
                "source.shell variable.other.special, source.shell variable.other.positional, source.shell variable.other.bracket",
                coral,
            ),
            rule("source.shell meta.function-call.arguments", ivory),
            rule("source.shell constant.other.option", whiskey),
            // ─── Python ─────────────────────────────────────────
            rule("source.python keyword.operator", cyan),
            rule(
                "source.python support.function, source.python meta.function-call",
                fountain_blue,
            ),
            rule("source.python support.type", chalky),
            rule(
                "source.python entity.name.function.decorator, source.python meta.function.decorator",
                chalky,
            ),
            rule("source.python constant.language", whiskey),
            // ─── JavaScript / TypeScript ────────────────────────
            rule(
                "source.js variable.other.constant, source.ts variable.other.constant",
                whiskey,
            ),
            rule(
                "source.js keyword.operator, source.ts keyword.operator",
                cyan,
            ),
            rule("source.js support.class, source.ts support.class", chalky),
            rule(
                "source.js punctuation.accessor, source.ts punctuation.accessor",
                ivory,
            ),
            rule(
                "source.js entity.name.function, source.ts entity.name.function",
                fountain_blue,
            ),
            // ─── Rust ───────────────────────────────────────────
            rule("source.rust keyword.operator", cyan),
            rule(
                "source.rust entity.name.type, source.rust entity.name.struct, source.rust entity.name.enum",
                chalky,
            ),
            rule("source.rust entity.name.function", fountain_blue),
            rule("source.rust entity.name.lifetime", whiskey),
            rule("source.rust storage.type", violet),
            rule(
                "source.rust support.macro, source.rust entity.name.macro",
                cyan,
            ),
            rule("source.rust constant.language", whiskey),
            // ─── Go ─────────────────────────────────────────────
            rule("source.go keyword.operator", cyan),
            rule("source.go entity.name.function", fountain_blue),
            rule("source.go entity.name.type", chalky),
            rule("source.go support.function.builtin", cyan),
            rule("source.go storage.type", violet),
            // ─── C / C++ ───────────────────────────────────────
            rule(
                "source.c keyword.operator, source.c++ keyword.operator",
                cyan,
            ),
            rule(
                "source.c entity.name.function, source.c++ entity.name.function",
                fountain_blue,
            ),
            rule("source.c storage.type, source.c++ storage.type", violet),
            rule(
                "source.c keyword.control.directive, source.c++ keyword.control.directive, meta.preprocessor",
                violet,
            ),
            rule(
                "source.c entity.name.type, source.c++ entity.name.type",
                chalky,
            ),
            // ─── Java ───────────────────────────────────────────
            rule("source.java keyword.operator", cyan),
            rule("source.java entity.name.function", fountain_blue),
            rule(
                "source.java entity.name.type, source.java storage.type",
                chalky,
            ),
            rule("source.java keyword.other.import", violet),
            rule("source.java entity.name.class", chalky),
            // ─── Ruby ───────────────────────────────────────────
            rule("source.ruby keyword.operator", cyan),
            rule("source.ruby entity.name.function", fountain_blue),
            rule(
                "source.ruby support.class, source.ruby entity.name.type.class",
                chalky,
            ),
            rule("source.ruby variable.other.readwrite.instance", coral),
            rule("source.ruby constant.language.symbol", fountain_blue),
            rule("source.ruby punctuation.definition.variable", coral),
            // ─── PHP ────────────────────────────────────────────
            rule("source.php keyword.operator", cyan),
            rule("source.php entity.name.function", fountain_blue),
            rule("source.php support.class", chalky),
            rule("source.php variable.other", coral),
            rule("source.php punctuation.definition.variable", coral),
            // ─── CSS / SCSS / LESS ──────────────────────────────
            rule(
                "source.css support.type.property-name, source.scss support.type.property-name",
                coral,
            ),
            rule(
                "source.css entity.other.attribute-name.class, source.scss entity.other.attribute-name.class",
                chalky,
            ),
            rule(
                "source.css entity.other.attribute-name.id, source.scss entity.other.attribute-name.id",
                fountain_blue,
            ),
            rule(
                "source.css constant.numeric, source.scss constant.numeric",
                whiskey,
            ),
            rule(
                "source.css keyword.other.unit, source.scss keyword.other.unit",
                whiskey,
            ),
            rule(
                "source.css support.constant, source.scss support.constant",
                whiskey,
            ),
            rule(
                "source.css entity.name.tag, source.scss entity.name.tag",
                coral,
            ),
            // ─── HTML / XML ─────────────────────────────────────
            rule("text.html entity.name.tag, text.xml entity.name.tag", coral),
            rule(
                "text.html entity.other.attribute-name, text.xml entity.other.attribute-name",
                whiskey,
            ),
            rule("text.html string.quoted, text.xml string.quoted", green),
            rule(
                "text.html punctuation.definition.tag, text.xml punctuation.definition.tag",
                ivory,
            ),
            // ─── JSON ───────────────────────────────────────────
            rule("source.json support.type.property-name", coral),
            rule("source.json string.quoted", green),
            rule("source.json constant", whiskey),
            // ─── YAML ───────────────────────────────────────────
            rule("source.yaml entity.name.tag", coral),
            rule("source.yaml string", green),
            rule("source.yaml constant", whiskey),
            // ─── TOML ───────────────────────────────────────────
            rule(
                "source.toml entity.name.tag, source.toml support.type.property-name",
                coral,
            ),
            rule("source.toml string", green),
            rule("source.toml constant", whiskey),
            // ─── SQL ────────────────────────────────────────────
            rule("source.sql keyword", violet),
            rule("source.sql entity.name.function", fountain_blue),
            rule(
                "source.sql constant.other.database-name, source.sql constant.other.table-name",
                chalky,
            ),
            rule("source.sql string", green),
            // ─── Markdown (nested) ──────────────────────────────
            rule("markup.heading", coral),
            rule_bold("markup.bold", whiskey),
            rule_italic("markup.italic", violet),
            rule("markup.inline.raw", green),
            rule("markup.underline.link", fountain_blue),
            // ─── Diff ───────────────────────────────────────────
            rule("markup.inserted", green),
            rule("markup.deleted", coral),
            rule("markup.changed", whiskey),
            // ─── Dockerfile ─────────────────────────────────────
            rule("source.dockerfile keyword", violet),
            rule(
                "source.dockerfile entity.name.function, source.dockerfile support.function",
                fountain_blue,
            ),
            // ─── Makefile ───────────────────────────────────────
            rule("source.makefile variable", coral),
            rule("source.makefile entity.name.function", fountain_blue),
            rule("source.makefile keyword.control", violet),
            // ─── Catch-all for operators ────────────────────────
            rule("keyword.operator", cyan),
        ],
    }
}

fn syntect_color_to_hsla(color: SyntectColor) -> Hsla {
    rgba_to_hsla(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    )
}

fn rgba_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f32::EPSILON {
        return Hsla {
            h: 0.0,
            s: 0.0,
            l,
            a,
        };
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < f32::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    Hsla {
        h: h / 6.0,
        s,
        l,
        a,
    }
}

pub(crate) fn apply_code_block_highlighting(
    line_content: &str,
    language: &str,
    colors: &mut [Hsla],
    weights: &mut [FontWeight],
    styles: &mut [FontStyle],
) {
    if line_content.is_empty() {
        return;
    }

    let syntax_set = syntax_set();
    let syntax = syntax_set
        .find_syntax_by_token(language)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, syntect_theme());
    if let Ok(ranges) = highlighter.highlight_line(line_content, syntax_set) {
        let mut byte_pos = 0usize;
        for (style, piece) in &ranges {
            let end = (byte_pos + piece.len()).min(colors.len());
            let color = syntect_color_to_hsla(style.foreground);
            for idx in byte_pos..end {
                colors[idx] = color;
                weights[idx] = if style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::BOLD)
                {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                };
                styles[idx] = if style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::ITALIC)
                {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };
            }
            byte_pos = end;
        }
    }
}

pub(crate) fn resolve_asset_image_path(path: &str) -> PathBuf {
    crate::resolve_runtime_path(path).unwrap_or_else(|| PathBuf::from(path))
}

pub(crate) fn is_remote_image_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// Resolve an image path to an absolute filesystem path.
/// Tries in order:
/// 1. Remote URLs (http:// https://) — not resolved to a local path
/// 2. file:// scheme — strip the scheme
/// 3. Home-relative paths (~/...)
/// 4. Relative paths resolved against the current file's parent directory
/// 5. Relative paths resolved against each workspace root
fn resolve_image_to_path(
    raw: &str,
    file_dir: Option<&PathBuf>,
    workspace_roots: &[PathBuf],
) -> Option<PathBuf> {
    // Strip file:// scheme
    let path_str = if let Some(stripped) = raw.strip_prefix("file://") {
        percent_decode_path(stripped)
    } else {
        raw.to_string()
    };

    let path_str = path_str.as_str();

    // Expand ~/
    let expanded = if let Some(rest) = path_str.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(rest)
        } else {
            return None;
        }
    } else {
        PathBuf::from(path_str)
    };

    // If the path happens to be absolute (e.g. from file:// or ~/ expansion),
    // just check existence.
    if expanded.is_absolute() {
        return expanded.exists().then_some(expanded);
    }

    // Relative to current file's directory
    if let Some(dir) = file_dir {
        let candidate = dir.join(&expanded);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Relative to workspace roots
    for root in workspace_roots {
        let candidate = root.join(&expanded);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn percent_decode_path(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(byte as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Load an image from any supported path/URL format.
/// Supports: http(s), file://, ~/paths, relative paths (to file dir or workspace roots).
pub(crate) fn load_image(
    raw_path: &str,
    file_dir: Option<&PathBuf>,
    workspace_roots: &[PathBuf],
) -> anyhow::Result<RenderImage> {
    if is_remote_image_url(raw_path) {
        return load_render_image_from_url(raw_path);
    }

    let resolved = resolve_image_to_path(raw_path, file_dir, workspace_roots)
        .ok_or_else(|| anyhow::anyhow!("could not resolve image path: {}", raw_path))?;
    load_render_image_from_path(&resolved)
}

fn load_render_image_from_bytes(bytes: &[u8]) -> anyhow::Result<RenderImage> {
    let dynamic = image::load_from_memory(bytes)?;
    let mut buffer = dynamic.to_rgba8();

    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let frame = Frame::new(buffer);
    Ok(RenderImage::new([frame]))
}

pub(crate) fn load_render_image_from_path(path: &PathBuf) -> anyhow::Result<RenderImage> {
    let bytes = std::fs::read(path)?;
    load_render_image_from_bytes(&bytes)
}

pub(crate) fn load_render_image_from_url(url: &str) -> anyhow::Result<RenderImage> {
    let response = ureq::get(url).call()?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)?;
    load_render_image_from_bytes(&bytes)
}

/// Show a native macOS file picker (via osascript) and return the chosen path.
pub(crate) fn pick_image_file_blocking() -> Option<PathBuf> {
    let script =
        r#"POSIX path of (choose file of type {"public.image"} with prompt "Choose Image")"#;
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

pub(crate) fn pick_folder_blocking() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("选择工作区目录")
        .pick_folder()
}

pub(crate) fn save_file_blocking(default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("保存文件")
        .set_file_name(default_name)
        .add_filter("Markdown", &["md", "markdown"])
        .save_file()
}

/// Map a buffer byte offset (within line) to display byte offset.
pub(crate) fn buffer_to_display_offset(buffer_to_display: &[usize], buf_offset: usize) -> usize {
    if buf_offset < buffer_to_display.len() {
        buffer_to_display[buf_offset]
    } else if let Some(&last) = buffer_to_display.last() {
        last
    } else {
        0
    }
}

/// Build styled TextRuns for the display text.
pub(crate) fn build_text_runs_for_display(
    display_text: &str,
    line_content: &str,
    display_to_buffer: &[usize],
    decoration: &LineDecoration,
    cursor_col: Option<usize>,
    code_block_syntax_active: bool,
    prefix_collapsed: bool,
    body_font: &Font,
    body_cjk_font: &Font,
    monospace_font: &Font,
    heading_font: &Font,
    heading_cjk_font: &Font,
    collapsed_task_prefix_chars: usize,
    _font_size: f32,
    theme: &Theme,
) -> Vec<TextRun> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FontRole {
        BodyLatin,
        BodyCjk,
        HeadingLatin,
        HeadingCjk,
        Monospace,
    }

    let is_code_line = matches!(
        decoration.block_type,
        BlockType::CodeBlock | BlockType::CodeBlockFence
    );
    let default_font = if matches!(decoration.block_type, BlockType::Heading(_)) {
        heading_font
    } else {
        body_font
    };
    if display_text.is_empty() {
        return vec![TextRun {
            len: 1,
            font: if is_code_line {
                monospace_font.clone()
            } else {
                default_font.clone()
            },
            color: theme.text_primary,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
    }

    let buf_len = line_content.len();
    let disp_len = display_text.len();

    let mut buf_colors: Vec<Hsla> = vec![theme.text_primary; buf_len];
    let mut buf_weights: Vec<FontWeight> = vec![FontWeight::NORMAL; buf_len];
    let mut buf_font_styles: Vec<FontStyle> = vec![FontStyle::Normal; buf_len];
    let mut buf_bg_colors: Vec<Option<Hsla>> = vec![None; buf_len];
    let mut buf_strikethroughs: Vec<bool> = vec![false; buf_len];
    let mut buf_font_roles: Vec<FontRole> = vec![FontRole::BodyLatin; buf_len];

    // Fence lines (``` or ```lang) always render semi-transparent
    if matches!(decoration.block_type, BlockType::CodeBlockFence) {
        let mut fence_color = theme.text_secondary;
        fence_color.a *= 0.5;
        buf_colors.fill(fence_color);
        buf_font_roles.fill(FontRole::Monospace);
    }

    if is_code_line {
        buf_font_roles.fill(FontRole::Monospace);
    } else {
        let is_heading = matches!(decoration.block_type, BlockType::Heading(_));
        for (start, ch) in line_content.char_indices() {
            let role = if prefers_latin_text_font(ch) {
                if is_heading {
                    FontRole::HeadingLatin
                } else {
                    FontRole::BodyLatin
                }
            } else if is_heading {
                FontRole::HeadingCjk
            } else {
                FontRole::BodyCjk
            };
            let end = start + ch.len_utf8();
            for idx in start..end.min(buf_len) {
                buf_font_roles[idx] = role;
            }
        }
    }

    match &decoration.block_type {
        BlockType::Heading(_) => {
            for c in buf_colors.iter_mut() {
                *c = theme.heading;
            }
            for w in buf_weights.iter_mut() {
                *w = FontWeight::BOLD;
            }
        }
        BlockType::Blockquote => {
            for c in buf_colors.iter_mut() {
                *c = theme.syntax_marker;
            }
        }
        BlockType::Table => {
            if decoration
                .table_info
                .as_ref()
                .is_some_and(|ti| ti.is_header)
            {
                for w in buf_weights.iter_mut() {
                    *w = FontWeight::BOLD;
                }
            }
        }
        _ => {}
    }

    if let BlockType::TaskList(true) = decoration.block_type {
        let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        let trimmed = &line_content[leading_ws..];
        let prefix_len = task_prefix_len(trimmed);
        let text_start = (leading_ws + prefix_len).min(buf_len);
        for index in text_start..buf_len {
            buf_colors[index] = theme.strikethrough;
            buf_strikethroughs[index] = true;
        }
    }

    if matches!(decoration.block_type, BlockType::CodeBlock)
        && let Some(language) = decoration.code_language.as_deref()
    {
        apply_code_block_highlighting(
            line_content,
            language,
            &mut buf_colors,
            &mut buf_weights,
            &mut buf_font_styles,
        );
    }

    for span in &decoration.inline_spans {
        let start = span.range.start.min(buf_len);
        let end = span.range.end.min(buf_len);
        for i in start..end {
            match span.kind {
                InlineKind::Bold => {
                    buf_colors[i] = theme.bold;
                    buf_weights[i] = FontWeight::BOLD;
                }
                InlineKind::Italic => {
                    buf_colors[i] = theme.italic;
                    buf_font_styles[i] = FontStyle::Italic;
                }
                InlineKind::Strikethrough => {
                    buf_colors[i] = theme.strikethrough;
                    buf_strikethroughs[i] = true;
                }
                InlineKind::InlineCode => {
                    buf_colors[i] = theme.inline_code;
                    buf_bg_colors[i] = Some(theme.bg_code_inline);
                    buf_font_roles[i] = FontRole::Monospace;
                }
                InlineKind::Link => {
                    buf_colors[i] = theme.link;
                }
                InlineKind::Image => {
                    buf_colors[i] = theme.link;
                }
            }
        }
    }

    if let Some(col) = cursor_col {
        if !matches!(decoration.block_type, BlockType::Table) {
            for sr in &decoration.syntax_ranges {
                let syntax_is_active = if matches!(decoration.block_type, BlockType::CodeBlockFence)
                {
                    // Fence lines always have syntax visible; use active code block check
                    // to determine if markers should get syntax_marker color
                    code_block_syntax_active
                        || (col >= sr.trigger_range.start && col < sr.trigger_range.end)
                } else {
                    col >= sr.trigger_range.start && col < sr.trigger_range.end
                };
                if syntax_is_active {
                    let start = sr.range.start.min(buf_len);
                    let end = sr.range.end.min(buf_len);
                    let syntax_color = match decoration.block_type {
                        BlockType::UnorderedList
                        | BlockType::OrderedList
                        | BlockType::TaskList(_) => theme.list_marker,
                        _ => theme.syntax_marker,
                    };
                    for i in start..end {
                        buf_colors[i] = syntax_color;
                        buf_font_roles[i] = FontRole::Monospace;
                    }
                }
            }
        }
    } else if matches!(decoration.block_type, BlockType::CodeBlockFence) && code_block_syntax_active
    {
        for sr in &decoration.syntax_ranges {
            let start = sr.range.start.min(buf_len);
            let end = sr.range.end.min(buf_len);
            for i in start..end {
                buf_colors[i] = theme.syntax_marker;
                buf_font_roles[i] = FontRole::Monospace;
            }
        }
    }

    let mut colors: Vec<Hsla> = Vec::with_capacity(disp_len);
    let mut weights: Vec<FontWeight> = Vec::with_capacity(disp_len);
    let mut font_styles: Vec<FontStyle> = Vec::with_capacity(disp_len);
    let mut bg_colors: Vec<Option<Hsla>> = Vec::with_capacity(disp_len);
    let mut strikethroughs: Vec<bool> = Vec::with_capacity(disp_len);
    let mut font_roles: Vec<FontRole> = Vec::with_capacity(disp_len);

    for i in 0..disp_len {
        let buf_idx = if i < display_to_buffer.len() {
            display_to_buffer[i]
        } else {
            buf_len.saturating_sub(1)
        };
        let buf_idx = buf_idx.min(buf_len.saturating_sub(1));
        colors.push(buf_colors[buf_idx]);
        weights.push(buf_weights[buf_idx]);
        font_styles.push(buf_font_styles[buf_idx]);
        bg_colors.push(buf_bg_colors[buf_idx]);
        strikethroughs.push(buf_strikethroughs[buf_idx]);
        font_roles.push(buf_font_roles[buf_idx]);
    }

    if prefix_collapsed {
        let leading_ws = display_text.len() - display_text.trim_start_matches([' ', '\t']).len();
        let prefix_len = compute_display_prefix_bytes(
            display_text,
            &decoration.block_type,
            true,
            collapsed_task_prefix_chars,
        )
        .min(disp_len);
        match &decoration.block_type {
            BlockType::UnorderedList => {
                let bullet_end = (leading_ws + "• ".len()).min(prefix_len);
                for i in leading_ws..bullet_end {
                    colors[i] = theme.list_marker;
                    font_roles[i] = FontRole::Monospace;
                }
            }
            BlockType::TaskList(_) => {
                let placeholder_start = leading_ws.saturating_sub(collapsed_task_prefix_chars);
                let placeholder_end = leading_ws.min(disp_len);
                for i in placeholder_start..placeholder_end {
                    colors[i] = gpui::transparent_black();
                    font_roles[i] = FontRole::Monospace;
                }
            }
            BlockType::OrderedList => {
                for i in leading_ws..prefix_len {
                    colors[i] = theme.list_marker;
                    font_roles[i] = FontRole::Monospace;
                }
            }
            _ => {}
        }
    }

    let mut runs = Vec::new();
    let mut run_start = 0;
    while run_start < disp_len {
        let color = colors[run_start];
        let weight = weights[run_start];
        let style = font_styles[run_start];
        let bg = bg_colors[run_start];
        let strike = strikethroughs[run_start];

        let mut run_end = run_start + 1;
        while run_end < disp_len
            && colors[run_end] == color
            && weights[run_end] == weight
            && font_styles[run_end] == style
            && bg_colors[run_end] == bg
            && strikethroughs[run_end] == strike
            && font_roles[run_end] == font_roles[run_start]
        {
            run_end += 1;
        }

        while run_end < disp_len && !display_text.is_char_boundary(run_end) {
            run_end += 1;
        }

        let mut font = match font_roles[run_start] {
            FontRole::BodyLatin => body_font.clone(),
            FontRole::BodyCjk => body_cjk_font.clone(),
            FontRole::HeadingLatin => heading_font.clone(),
            FontRole::HeadingCjk => heading_cjk_font.clone(),
            FontRole::Monospace => monospace_font.clone(),
        };
        font.weight = weight;
        font.style = style;

        runs.push(TextRun {
            len: run_end - run_start,
            font,
            color,
            background_color: bg,
            underline: None,
            strikethrough: if strike {
                Some(StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(theme.strikethrough),
                })
            } else {
                None
            },
        });

        run_start = run_end;
    }

    runs
}

/// Apply letter spacing to a shaped WrappedLine by offsetting glyph positions.
/// Each glyph's x position is shifted by `glyph_index * spacing` pixels.
pub(crate) fn apply_letter_spacing(wrapped: &mut WrappedLine, spacing_px: f32) {
    if spacing_px == 0.0 {
        return;
    }

    let old_layout = &**wrapped;
    let old_line = &old_layout.unwrapped_layout;

    let mut new_runs: Vec<ShapedRun> = Vec::with_capacity(old_line.runs.len());
    let mut glyph_count: usize = 0;

    for run in &old_line.runs {
        let mut new_glyphs: Vec<ShapedGlyph> = Vec::with_capacity(run.glyphs.len());
        for glyph in &run.glyphs {
            new_glyphs.push(ShapedGlyph {
                id: glyph.id,
                position: point(
                    glyph.position.x + px(glyph_count as f32 * spacing_px),
                    glyph.position.y,
                ),
                index: glyph.index,
                is_emoji: glyph.is_emoji,
            });
            glyph_count += 1;
        }
        new_runs.push(ShapedRun {
            font_id: run.font_id,
            glyphs: new_glyphs,
        });
    }

    let extra_width = if glyph_count > 0 {
        px((glyph_count - 1) as f32 * spacing_px)
    } else {
        px(0.0)
    };

    let new_line_layout = Arc::new(LineLayout {
        font_size: old_line.font_size,
        width: old_line.width + extra_width,
        ascent: old_line.ascent,
        descent: old_line.descent,
        runs: new_runs,
        len: old_line.len,
    });

    let new_wrapped_layout = Arc::new(WrappedLineLayout {
        unwrapped_layout: new_line_layout,
        wrap_boundaries: old_layout.wrap_boundaries.clone(),
        wrap_width: old_layout.wrap_width,
    });

    let arc_mut: &mut Arc<WrappedLineLayout> = &mut *wrapped;
    *arc_mut = new_wrapped_layout;
}
