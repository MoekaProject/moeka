use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    App, BorderStyle, Bounds, Corners, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, Font, FontFallbacks, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    IntoElement, LayoutId, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style, TextAlign,
    TextRun, TransformationMatrix, Window, WrappedLine, fill, point, px, quad, relative, size,
};

use crate::markdown::BlockType;

use super::helpers;
use super::{profile, *};

pub(crate) struct EditorElement {
    pub editor: Entity<EditorView>,
}

pub(crate) struct EditorPrepaintState {
    hitbox: Hitbox,
    link_hitboxes: Vec<Hitbox>,
    lines: Vec<PrepaintLine>,
    indent_guide_quads: Vec<PaintQuad>,
    whitespace_dots: Vec<PaintQuad>,
    cursor_quads: Vec<PaintQuad>,
    selection_quads: Vec<PaintQuad>,
    search_match_quads: Vec<PaintQuad>,
    diff_line_bgs: Vec<PaintQuad>,
    diff_inline_quads: Vec<PaintQuad>,
    line_highlight: Option<PaintQuad>,
    code_block_bgs: Vec<PaintQuad>,
    unordered_list_markers: Vec<(WrappedLine, Point<Pixels>, Pixels)>,
    marked_underlines: Vec<PaintQuad>,
    gutter_quads: Vec<PaintQuad>,
    gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    gutter_fold_icons: Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    /// HR dashed lines: (y_position, start_x, width)
    hr_lines: Vec<(f32, f32, f32)>,
    /// Image previews to paint: (bounds, image_data)
    image_previews: Vec<(Bounds<Pixels>, Arc<RenderImage>, Pixels)>,
    image_borders: Vec<PaintQuad>,
    /// Table cell borders and backgrounds
    table_borders: Vec<PaintQuad>,
    table_bgs: Vec<PaintQuad>,
    /// Image URL-edit icons: (bounds, line_idx, alpha)
    image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)>,
    /// Blue wavy underlines for links when cmd is held
    link_underlines: Vec<PaintQuad>,
    inline_overlays: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    /// Per-cell shaped texts for table block rendering.
    table_cell_texts: Vec<(WrappedLine, Point<Pixels>, Pixels)>,
    code_block_copy_buttons: Vec<CodeBlockCopyButtonPaint>,
    code_block_copy_hitboxes: Vec<Hitbox>,
    preview_zoom_buttons: Vec<CodeBlockCopyButtonPaint>,
    preview_zoom_hitboxes: Vec<Hitbox>,
}

struct CodeBlockGroupBounds {
    left: f32,
    top: f32,
    width: f32,
    bottom: f32,
}

struct CodeBlockCopyButtonPaint {
    quad: PaintQuad,
    label: WrappedLine,
    origin: Point<Pixels>,
    line_height: Pixels,
}

fn shape_preview_button(
    window: &Window,
    button_font: &Font,
    button_font_size: f32,
    button_padding_left_x: f32,
    button_padding_right_x: f32,
    button_padding_y: f32,
    button_text_line_height: f32,
    label_text: &str,
    text_color: Hsla,
) -> Option<(WrappedLine, f32, f32)> {
    let label_run = TextRun {
        len: label_text.len(),
        font: button_font.clone(),
        color: text_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let label_lines = window
        .text_system()
        .shape_text(
            label_text.to_string().into(),
            px(button_font_size),
            &[label_run],
            None,
            None,
        )
        .ok()?;
    let label = label_lines.into_iter().next()?;
    let button_width = f32::from(label.width()) + button_padding_left_x + button_padding_right_x;
    let button_height = button_text_line_height + button_padding_y * 2.0;
    Some((label, button_width, button_height))
}

struct PrepaintLine {
    wrapped: WrappedLine,
    origin: Point<Pixels>,
    line_height: Pixels,
    /// Per-visual-line shaped segments for indented painting.
    segments: Option<Vec<(WrappedLine, Point<Pixels>)>>,
    /// True if this line is a table row rendered via per-cell block mode.
    is_table_block: bool,
}

fn diff_line_background(editor: &EditorView, line_idx: usize) -> Option<Hsla> {
    let row = editor.active_git_diff_state()?.rows.get(line_idx)?;
    match row.kind {
        GitDiffRowKind::Added => Some(editor.styles.selection.selection_diff_added_line_bg),
        GitDiffRowKind::Removed => Some(editor.styles.selection.selection_diff_removed_line_bg),
        GitDiffRowKind::Context => None,
    }
}

fn diff_hunk_label_text_color(editor: &EditorView, line_idx: usize) -> Option<Hsla> {
    let row = editor.active_git_diff_state()?.rows.get(line_idx)?;
    row.is_hunk_label.then_some(
        editor
            .styles
            .editor
            .editor_diff_collapsed_line_label_text_color,
    )
}

fn diff_table_syntax_color_ranges(
    line_content: &str,
    table_info: &crate::markdown::TableLineInfo,
) -> Vec<Range<usize>> {
    let line_len = line_content.len();
    if table_info.is_separator {
        return (line_len > 0).then_some(0..line_len).into_iter().collect();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    for cell_range in &table_info.cell_ranges {
        let start = cell_range.start.min(line_len);
        let end = cell_range.end.min(line_len).max(start);
        if cursor < start {
            ranges.push(cursor..start);
        }
        cursor = end;
    }
    if cursor < line_len {
        ranges.push(cursor..line_len);
    }
    ranges
}

fn apply_display_text_color_ranges(
    runs: &mut Vec<TextRun>,
    display_text: &str,
    ranges: &[Range<usize>],
    color: Hsla,
) {
    if runs.is_empty() || display_text.is_empty() || ranges.is_empty() {
        return;
    }

    let mut byte_colors: Vec<Option<Hsla>> = vec![None; display_text.len()];
    for range in ranges {
        let start = range.start.min(display_text.len());
        let end = range.end.min(display_text.len()).max(start);
        for entry in byte_colors.iter_mut().take(end).skip(start) {
            *entry = Some(color);
        }
    }

    let original_runs = std::mem::take(runs);
    let mut styled_runs = Vec::with_capacity(original_runs.len());
    let mut run_start = 0usize;
    for run in original_runs {
        let run_end = run_start + run.len;
        let mut segment_start = run_start;
        let mut segment_color = byte_colors
            .get(run_start)
            .copied()
            .flatten()
            .unwrap_or(run.color);
        let mut cursor = run_start;

        while cursor < run_end {
            let next = display_text[cursor..run_end]
                .chars()
                .next()
                .map(|ch| cursor + ch.len_utf8())
                .unwrap_or(run_end);
            let next_color = if next < run_end {
                byte_colors
                    .get(next)
                    .copied()
                    .flatten()
                    .unwrap_or(run.color)
            } else {
                segment_color
            };
            if next < run_end && next_color != segment_color {
                let mut segment_run = run.clone();
                segment_run.len = next - segment_start;
                segment_run.color = segment_color;
                styled_runs.push(segment_run);
                segment_start = next;
                segment_color = next_color;
            }
            cursor = next;
        }

        let mut segment_run = run;
        segment_run.len = run_end - segment_start;
        segment_run.color = segment_color;
        styled_runs.push(segment_run);
        run_start = run_end;
    }

    *runs = styled_runs;
}

fn push_diff_inline_highlight_quads(
    quads: &mut Vec<PaintQuad>,
    spans: &[Range<usize>],
    display_text: &str,
    line_content_len: usize,
    buffer_to_display: &[usize],
    runs: &[TextRun],
    wrapped: &WrappedLine,
    content_x: f32,
    text_line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    fill_color: Hsla,
    border_color: Hsla,
    border_width_px: f32,
    letter_spacing: f32,
    window: &mut Window,
) {
    if spans.is_empty() {
        return;
    }

    for span in spans {
        let start = span.start.min(line_content_len);
        let end = span.end.min(line_content_len);
        if start >= end {
            continue;
        }

        let disp_start = helpers::buffer_to_display_offset(buffer_to_display, start);
        let disp_end = helpers::buffer_to_display_offset(buffer_to_display, end);
        for bounds in helpers::build_highlight_bounds(
            display_text,
            runs,
            wrapped,
            disp_start,
            disp_end,
            content_x,
            text_line_top,
            line_height,
            font_size,
            indent_px,
            0.0,
            letter_spacing,
            window,
        ) {
            quads.push(fill(bounds, fill_color));
            quads.push(quad(
                bounds,
                Corners::all(px(0.0)),
                gpui::transparent_black(),
                px(border_width_px),
                border_color,
                BorderStyle::default(),
            ));
        }
    }
}

fn build_precise_table_layout(
    window: &mut Window,
    buffer: &Buffer,
    decorations: &[LineDecoration],
    table_start: usize,
    table_end: usize,
    text_width: f32,
    cell_padding_x: f32,
    cell_padding_y: f32,
    base_char_width: f32,
    line_height: f32,
    handle_size: f32,
    body_font: &Font,
    body_cjk_font: &Font,
    monospace_font: &Font,
    heading_font: &Font,
    heading_cjk_font: &Font,
    font_size: f32,
    theme: &Theme,
    cursor_offsets: &[usize],
    marked_range: Option<&Range<usize>>,
) -> helpers::TableLayout {
    let mut layout = helpers::compute_table_layout(
        decorations,
        table_start,
        table_end,
        text_width,
        cell_padding_x,
        cell_padding_y,
        base_char_width,
        line_height,
        handle_size,
    );

    let mut row_heights = vec![line_height + cell_padding_y * 2.0; layout.visual_row_count];
    for vi in 0..layout.visual_row_count {
        let Some(source_line) = layout.visual_row_to_line.get(vi).copied() else {
            continue;
        };
        let line_start_offset = buffer.line_start(source_line);
        let row_ti = decorations
            .get(source_line)
            .and_then(|d| d.table_info.as_ref());
        let mut row_height = line_height + cell_padding_y * 2.0;

        if let Some(row_ti) = row_ti
            && let Some(row_cells) = layout.cells.get_mut(vi)
        {
            for cell_layout in row_cells.iter_mut() {
                let cell_content = row_ti
                    .cells
                    .get(cell_layout.col)
                    .cloned()
                    .unwrap_or_default();
                let cell_active_ranges = helpers::active_table_cell_ranges(
                    line_start_offset,
                    &cell_layout.cell_range,
                    cursor_offsets,
                    marked_range,
                );
                let cell_decoration = crate::markdown::parse_decorations(&cell_content)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                let (folded_raw, d2b_raw, b2d_raw, _) = helpers::build_display_text(
                    &cell_content,
                    &cell_decoration,
                    &cell_active_ranges,
                    false,
                    None,
                    0,
                    0,
                    0,
                    0,
                    0,
                    1.0,
                    1.0,
                    1.0,
                );
                let trim_start = folded_raw.len() - folded_raw.trim_start().len();
                let trim_end = folded_raw.len() - folded_raw.trim_end().len();
                cell_layout.display_text = folded_raw.trim().to_string();
                cell_layout.display_to_buffer =
                    d2b_raw[trim_start..d2b_raw.len() - trim_end].to_vec();
                cell_layout.buffer_to_display = b2d_raw
                    .iter()
                    .map(|&display_index| display_index.saturating_sub(trim_start))
                    .collect();

                let mut cell_runs = helpers::build_text_runs_for_display(
                    &cell_layout.display_text,
                    &cell_content,
                    &cell_layout.display_to_buffer,
                    &cell_decoration,
                    &cell_active_ranges,
                    false,
                    false,
                    body_font,
                    body_cjk_font,
                    monospace_font,
                    heading_font,
                    heading_cjk_font,
                    0,
                    font_size,
                    theme,
                    crate::theme::with_alpha(theme.git_status_added, 0.24),
                    crate::theme::with_alpha(theme.text_danger, 0.28),
                    theme.link,
                    FontWeight::BOLD,
                    None,
                );
                if layout.is_header.get(vi).copied().unwrap_or(false) && !layout.is_front_matter {
                    for run in &mut cell_runs {
                        run.font.weight = gpui::FontWeight::BOLD;
                    }
                }

                let cell_text: SharedString = if cell_layout.display_text.is_empty() {
                    " ".into()
                } else {
                    cell_layout.display_text.clone().into()
                };
                let inner_width = (cell_layout.width - cell_padding_x * 2.0).max(base_char_width);
                if let Ok(shaped_lines) = window.text_system().shape_text(
                    cell_text,
                    px(font_size),
                    &cell_runs,
                    Some(px(inner_width)),
                    None,
                ) {
                    if let Some(shaped) = shaped_lines.into_iter().next() {
                        let visual_lines = helpers::wrap_boundary_byte_indices(&shaped).len() + 1;
                        row_height = row_height
                            .max(visual_lines as f32 * line_height + cell_padding_y * 2.0);
                    }
                }
            }
        }

        row_heights[vi] = row_height;
    }

    let mut current_y = 0.0_f32;
    for (vi, row) in layout.cells.iter_mut().enumerate() {
        let row_height = row_heights
            .get(vi)
            .copied()
            .unwrap_or(line_height + cell_padding_y * 2.0);
        for cell in row.iter_mut() {
            cell.y = current_y;
            cell.height = row_height;
        }
        current_y += row_height;
    }
    layout.row_heights = row_heights;
    layout.total_height = current_y;
    layout
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

fn push_whitespace_dot_quads(
    dots: &mut Vec<PaintQuad>,
    text: &str,
    wrapped: &WrappedLine,
    origin: Point<Pixels>,
    line_height: f32,
    dot_size: f32,
    dot_color: Hsla,
    skip_prefix_bytes: usize,
) {
    let dot_radius = px(dot_size / 2.0);
    for (index, character) in text.char_indices() {
        if index < skip_prefix_bytes {
            continue;
        }
        if character != ' ' {
            continue;
        }
        let Some(start) = wrapped.position_for_index(index, px(line_height)) else {
            continue;
        };
        let next_index = index + character.len_utf8();
        let end = wrapped
            .position_for_index(next_index.min(text.len()), px(line_height))
            .unwrap_or(start);
        let start_x = f32::from(start.x);
        let start_y = f32::from(start.y);
        let end_x = if f32::from(end.y) == start_y {
            f32::from(end.x)
        } else {
            start_x + line_height * 0.45
        };
        let width = (end_x - start_x).abs().max(dot_size);
        let dot_x = f32::from(origin.x) + start_x + (width - dot_size) * 0.5;
        let dot_y = f32::from(origin.y) + start_y + (line_height - dot_size) * 0.5;
        dots.push(quad(
            Bounds::new(
                point(px(dot_x), px(dot_y)),
                size(px(dot_size), px(dot_size)),
            ),
            Corners::all(dot_radius),
            dot_color,
            px(0.0),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
    }
}

fn compute_wrap_boundary_upstream_xs(
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

    for boundary in helpers::wrap_boundary_byte_indices(wrapped) {
        let segment_text = &display_text[segment_start..boundary];
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

fn upstream_wrap_boundary_position(
    wrapped: &WrappedLine,
    display_text: &str,
    disp_col: usize,
    upstream_xs: &HashMap<usize, f32>,
    line_height: f32,
) -> Option<Point<Pixels>> {
    let upstream_x = *upstream_xs.get(&disp_col)?;
    let previous_boundary = display_text[..disp_col]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0);
    let previous_pos = wrapped.position_for_index(previous_boundary, px(line_height))?;
    Some(point(px(upstream_x), previous_pos.y))
}

fn downstream_wrap_boundary_position(
    wrapped: &WrappedLine,
    disp_col: usize,
    line_height: f32,
) -> Option<Point<Pixels>> {
    let downstream_visual_idx = helpers::wrap_boundary_byte_indices(wrapped)
        .iter()
        .position(|boundary| *boundary == disp_col)?
        + 1;
    Some(point(
        px(0.0),
        px(downstream_visual_idx as f32 * line_height),
    ))
}

fn push_gutter_line_number_label(
    gutter_labels: &mut Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    window: &mut Window,
    line_index: usize,
    is_active: bool,
    font: &Font,
    font_size: f32,
    gutter_right_x: f32,
    text_line_top: f32,
    line_height: f32,
    vertical_offset: f32,
    right_gap: f32,
    text_color: Hsla,
    active_text_color: Hsla,
) {
    let line_number = (line_index + 1).to_string();
    let line_number_run = TextRun {
        len: line_number.len(),
        font: font.clone(),
        color: if is_active {
            active_text_color
        } else {
            text_color
        },
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(
        line_number.into(),
        px(font_size),
        &[line_number_run],
        None,
    );
    let label_x = gutter_right_x - right_gap - f32::from(shaped.width);
    let label_y = (text_line_top - vertical_offset).round();
    gutter_labels.push((shaped, point(px(label_x), px(label_y)), px(line_height)));
}

fn leading_indent_columns(line: &str) -> usize {
    let mut columns = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            _ => break,
        }
    }
    columns
}

pub(super) fn detect_indent_guide_step(buffer: &crate::buffer::Buffer, line_count: usize) -> usize {
    detect_indent_guide_step_in_range(buffer, 0, line_count)
}

fn detect_indent_guide_step_in_range(
    buffer: &crate::buffer::Buffer,
    start_line: usize,
    end_line: usize,
) -> usize {
    let mut previous_indent = None;
    let mut detected_step = usize::MAX;

    for line_idx in start_line..end_line.min(buffer.line_count()) {
        let line = buffer.line_content(line_idx);
        if line.trim().is_empty() {
            continue;
        }

        let indent = leading_indent_columns(line);
        if let Some(previous) = previous_indent
            && indent > previous
        {
            detected_step = detected_step.min(indent - previous);
        }
        previous_indent = Some(indent);
    }

    if detected_step == usize::MAX {
        4
    } else {
        detected_step.clamp(1, 8)
    }
}

fn push_indent_guide_quads(
    quads: &mut Vec<PaintQuad>,
    line_content: &str,
    content_x: f32,
    line_top: f32,
    total_line_height: f32,
    char_width: f32,
    indent_step_columns: usize,
    guide_width: f32,
    dash_len: f32,
    gap_len: f32,
    guide_color: Hsla,
) {
    if indent_step_columns == 0 || line_content.trim().is_empty() {
        return;
    }

    let indent_columns = leading_indent_columns(line_content);
    let guide_count = indent_columns / indent_step_columns;
    if guide_count == 0 {
        return;
    }

    let level_width = indent_step_columns as f32 * char_width;
    if gap_len <= 0.0 {
        for level in 0..guide_count {
            let guide_x = content_x + level as f32 * level_width;
            quads.push(fill(
                Bounds::new(
                    point(px(guide_x), px(line_top)),
                    size(px(guide_width), px(total_line_height.max(0.0))),
                ),
                guide_color,
            ));
        }
        return;
    }

    let segment_step = (dash_len + gap_len).max(1.0);
    for level in 0..guide_count {
        let guide_x = content_x + level as f32 * level_width;
        let mut y = line_top;
        let line_bottom = line_top + total_line_height;
        while y < line_bottom {
            let height = dash_len.min(line_bottom - y);
            quads.push(fill(
                Bounds::new(point(px(guide_x), px(y)), size(px(guide_width), px(height))),
                guide_color,
            ));
            y += segment_step;
        }
    }
}

fn inline_overlay_prefers_latin_font(ch: char) -> bool {
    matches!(
        ch,
        '\u{0000}'..='\u{024F}'
            | '\u{1E00}'..='\u{1EFF}'
            | '\u{2000}'..='\u{206F}'
            | '\u{20A0}'..='\u{20CF}'
    )
}

fn inline_overlay_runs(
    text: &str,
    latin_font: &Font,
    cjk_font: &Font,
    color: Hsla,
) -> Vec<TextRun> {
    text.chars()
        .map(|ch| TextRun {
            len: ch.len_utf8(),
            font: if inline_overlay_prefers_latin_font(ch) {
                latin_font.clone()
            } else {
                cjk_font.clone()
            },
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect()
}

enum InlineOverlayVerticalAlign {
    Top(f32),
    Bottom(f32),
    Offset(f32),
}

fn push_inline_text_overlay(
    overlays: &mut Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    window: &mut Window,
    wrapped: &WrappedLine,
    overlay_text: &str,
    visible_range: Range<usize>,
    latin_font: &Font,
    cjk_font: &Font,
    color: Hsla,
    font_size: f32,
    scale: f32,
    vertical_align: InlineOverlayVerticalAlign,
    anchor_right: bool,
    content_x: f32,
    text_line_top: f32,
    indent_px: f32,
    line_height: f32,
) {
    if visible_range.start >= visible_range.end || overlay_text.is_empty() {
        return;
    }

    let Some(pos_start) = wrapped.position_for_index(visible_range.start, px(line_height)) else {
        return;
    };
    let Some(pos_end) = wrapped.position_for_index(visible_range.end, px(line_height)) else {
        return;
    };
    if pos_start.y != pos_end.y {
        return;
    }

    let overlay_runs = inline_overlay_runs(overlay_text, latin_font, cjk_font, color);
    let overlay_font_size = (font_size * scale).max(8.0);
    let shaped = window.text_system().shape_line(
        SharedString::from(overlay_text.to_string()),
        px(overlay_font_size),
        &overlay_runs,
        None,
    );
    let indent_start = helpers::indent_for_y(indent_px, pos_start.y, line_height);
    let reserved_width = f32::from(pos_end.x - pos_start.x).max(0.0);
    let overlay_width = f32::from(shaped.width);
    let horizontal_offset = if anchor_right {
        (reserved_width - overlay_width).max(0.0)
    } else {
        0.0
    };
    let overlay_line_height = px((overlay_font_size * 1.15).max(overlay_font_size + 1.0));
    let line_top = text_line_top + f32::from(pos_start.y);
    let line_bottom = line_top + line_height;
    let overlay_origin_y = match vertical_align {
        InlineOverlayVerticalAlign::Top(inset) => line_top + inset,
        InlineOverlayVerticalAlign::Bottom(inset) => {
            line_bottom - f32::from(overlay_line_height) - inset
        }
        InlineOverlayVerticalAlign::Offset(offset) => line_top + offset,
    };
    let overlay_origin = point(
        px(content_x + indent_start + horizontal_offset) + pos_start.x,
        px(overlay_origin_y),
    );
    overlays.push((shaped, overlay_origin, overlay_line_height));
}

fn non_markdown_cached_line_can_reuse(
    cached: &LineDisplayInfo,
    current_wrap_width: Option<f32>,
) -> bool {
    const WIDTH_TOLERANCE_PX: f32 = 0.5;

    match current_wrap_width {
        Some(current_wrap_width) => {
            cached.wrap_width_px.is_some_and(|cached_wrap_width| {
                (cached_wrap_width - current_wrap_width).abs() <= WIDTH_TOLERANCE_PX
            }) || (cached.visual_line_count <= 1
                && cached.unwrapped_width_px <= current_wrap_width + WIDTH_TOLERANCE_PX)
        }
        None => cached.wrap_width_px.is_none() || cached.visual_line_count <= 1,
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some("editor-element".into())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let _profile_scope = profile::ProfileScope::new("editor.prepaint");
        let editor = self.editor.read(cx);
        let theme = &editor.theme;
        let buffer = &editor.buffer;
        let cursor = &editor.cursor;
        let decorations = &editor.decorations;
        let scroll_offset = editor.scroll_offset;
        let cursor_line = editor.cursor_line();

        let viewport_height: f32 = bounds.size.height.into();
        let viewport_width: f32 = bounds.size.width.into();
        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let expanded_layout = editor.editor_uses_expanded_layout();
        let editor_width = if expanded_layout {
            viewport_width
        } else {
            viewport_width.min(editor.max_editor_width())
        };
        let centered_left = origin_x + ((viewport_width - editor_width) / 2.0).max(0.0);
        let editor_left = if expanded_layout {
            origin_x
        } else {
            centered_left
        };
        let text_width = (editor_width - editor.padding_left() - editor.padding_right())
            .max(editor.scaled(120.0));
        let wrap_width = (text_width
            - editor.scaled(editor.styles.editor.editor_wrap_right_padding_px))
        .max(editor.scaled(120.0));
        let gutter_right_x = editor.gutter_line_number_right_x(editor_left);
        let gutter_icon_column_left = editor.gutter_icon_column_left(editor_left);
        let gutter_icon_column_width = editor.gutter_icon_column_width();

        let mut lines = Vec::new();
        let mut indent_guide_quads = Vec::new();
        let mut selection_quads = Vec::new();
        let mut search_match_quads: Vec<PaintQuad> = Vec::new();
        let mut line_highlight = None;
        let mut code_block_bgs = Vec::new();
        let mut whitespace_dots: Vec<PaintQuad> = Vec::new();
        let mut cursor_quads: Vec<PaintQuad> = Vec::new();
        let mut marked_underlines = Vec::new();
        let mut link_underlines: Vec<PaintQuad> = Vec::new();
        let mut link_hitboxes: Vec<Hitbox> = Vec::new();

        let line_count = buffer.line_count();
        let has_valid_cache = editor.line_display_info.len() == line_count
            && (editor.last_text_width - text_width).abs() < 0.5;
        let wrap_width_changed = (editor.last_text_width - text_width).abs() >= 0.5;
        const OFFSCREEN_WRAP_REFLOW_BUDGET_PER_FRAME: usize = 48;
        const WINDOWED_PREPAINT_OVERSCAN_FACTOR: f32 = 0.75;
        const WINDOWED_PREPAINT_CURSOR_CONTEXT_LINES: usize = 8;
        let active_file_kind = editor.active_file_kind();
        let active_tab_is_markdown = active_file_kind.is_some_and(FileKind::is_markdown);
        let use_windowed_prepaint = !wrap_width_changed
            && active_file_kind.is_some_and(|file_kind| {
                if file_kind.is_markdown() {
                    has_valid_cache
                } else {
                    file_kind.supports_text_buffer()
                }
            });
        let (render_start_line, render_end_line) = if use_windowed_prepaint && line_count > 0 {
            let overscan_height = viewport_height * WINDOWED_PREPAINT_OVERSCAN_FACTOR;
            let mut start = editor
                .line_at_y((scroll_offset - overscan_height).max(0.0))
                .saturating_sub(WINDOWED_PREPAINT_CURSOR_CONTEXT_LINES);
            let mut end = editor
                .line_at_y(scroll_offset + viewport_height + overscan_height)
                .saturating_add(1);

            if editor.editor_wraps_to_available_width() {
                end = end.saturating_add(OFFSCREEN_WRAP_REFLOW_BUDGET_PER_FRAME);
            }

            start = start.min(cursor_line.saturating_sub(WINDOWED_PREPAINT_CURSOR_CONTEXT_LINES));
            end = end.max(cursor_line.saturating_add(WINDOWED_PREPAINT_CURSOR_CONTEXT_LINES + 1));

            if active_tab_is_markdown {
                let mut expanded = true;
                while expanded {
                    expanded = false;
                    let scan_start = start.min(line_count);
                    let scan_end = end.min(line_count);
                    for line_idx in scan_start..scan_end {
                        let Some(decoration) = decorations.get(line_idx) else {
                            continue;
                        };

                        if matches!(
                            decoration.block_type,
                            BlockType::CodeBlock | BlockType::CodeBlockFence
                        ) {
                            let mut group_start = line_idx;
                            while group_start > 0
                                && matches!(
                                    decorations
                                        .get(group_start - 1)
                                        .map(|decoration| &decoration.block_type),
                                    Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                                )
                            {
                                group_start -= 1;
                            }

                            let mut group_end = line_idx.saturating_add(1);
                            while group_end < line_count
                                && matches!(
                                    decorations
                                        .get(group_end)
                                        .map(|decoration| &decoration.block_type),
                                    Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                                )
                            {
                                group_end += 1;
                            }

                            if group_start < start {
                                start = group_start;
                                expanded = true;
                            }
                            if group_end > end {
                                end = group_end;
                                expanded = true;
                            }
                        }

                        if let Some(table_info) = decoration.table_info.as_ref()
                            && !table_info.is_front_matter
                        {
                            if table_info.table_start_line < start {
                                start = table_info.table_start_line;
                                expanded = true;
                            }
                            if table_info.table_end_line > end {
                                end = table_info.table_end_line;
                                expanded = true;
                            }
                        }

                        if let Some(preview) = decoration.block_preview.as_ref() {
                            if preview.start_line < start {
                                start = preview.start_line;
                                expanded = true;
                            }
                            let preview_end = preview.end_line.saturating_add(1);
                            if preview_end > end {
                                end = preview_end;
                                expanded = true;
                            }
                        }
                    }
                }
            }

            (start.min(line_count), end.min(line_count))
        } else {
            (0, line_count)
        };
        let mut y = origin_y
            + if render_start_line == 0 {
                editor.padding_top()
            } else {
                editor.line_y_offset(render_start_line)
            };
        let mut _last_line_height = 0.0_f32;
        let mut scroll_anchor_delta = 0.0_f32;
        let suppress_scroll_anchor_compensation = editor.pending_preview_reveal.is_some()
            || editor.pending_center_cursor_in_view
            || editor.pending_scroll_to_cursor;

        // Collect all cursor selection ranges (primary + extra)
        let mut all_sel_ranges: Vec<std::ops::Range<usize>> =
            Vec::with_capacity(1 + editor.extra_cursors.len());
        let sel_range = cursor.selection_range();
        let _has_selection = cursor.has_selection();
        all_sel_ranges.push(sel_range.clone());
        for ec in &editor.extra_cursors {
            all_sel_ranges.push(ec.selection_range());
        }
        // Collect all cursor offsets for painting cursor bars
        let mut all_cursor_offsets: Vec<usize> = Vec::with_capacity(1 + editor.extra_cursors.len());
        all_cursor_offsets.push(cursor.offset);
        for ec in &editor.extra_cursors {
            all_cursor_offsets.push(ec.offset);
        }
        let any_selection = all_sel_ranges.iter().any(|r| r.start != r.end);
        let mut active_line_numbers: HashSet<usize> = HashSet::new();
        for &offset in &all_cursor_offsets {
            active_line_numbers.insert(buffer.offset_to_line_col(offset).0);
        }
        for range in all_sel_ranges
            .iter()
            .filter(|range| range.start != range.end)
        {
            let start_line = buffer.offset_to_line_col(range.start).0;
            let end_offset = range.end.saturating_sub(1).min(buffer.len());
            let end_line = buffer.offset_to_line_col(end_offset).0;
            for line in start_line..=end_line {
                active_line_numbers.insert(line);
            }
        }

        let mut gutter_quads = Vec::new();
        let mut gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla)> = Vec::new();
        let mut gutter_fold_icons: Vec<(Bounds<Pixels>, &'static str, Hsla)> = Vec::new();
        let mut checkbox_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut image_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut generated_preview_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut generated_preview_image_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut generated_preview_zoom_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut image_preview_zoom_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut display_info_updates: Vec<(usize, Option<LineDisplayInfo>)> =
            Vec::with_capacity(render_end_line.saturating_sub(render_start_line));
        let mut hr_lines: Vec<(f32, f32, f32)> = Vec::new();
        let mut image_previews: Vec<(Bounds<Pixels>, Arc<RenderImage>, Pixels)> = Vec::new();
        let mut image_borders: Vec<PaintQuad> = Vec::new();
        let mut table_borders: Vec<PaintQuad> = Vec::new();
        let mut table_bgs: Vec<PaintQuad> = Vec::new();
        let mut diff_line_bgs: Vec<PaintQuad> = Vec::new();
        let mut diff_inline_quads: Vec<PaintQuad> = Vec::new();
        let mut table_cell_texts: Vec<(WrappedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut table_click_areas_local: Vec<(Bounds<Pixels>, usize, usize, usize)> = Vec::new();
        let mut table_col_handle_areas_local: Vec<(Bounds<Pixels>, usize, usize)> = Vec::new();
        let mut table_row_handle_areas_local: Vec<(Bounds<Pixels>, usize, usize)> = Vec::new();
        let mut table_layout_cache: HashMap<usize, helpers::TableLayout> = HashMap::new();
        let mut image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)> = Vec::new();
        let mut image_edit_hit_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut gutter_fold_hit_areas_local: Vec<(Bounds<Pixels>, usize, bool)> = Vec::new();
        let mut code_block_group_bounds: HashMap<usize, CodeBlockGroupBounds> = HashMap::new();
        let mut code_block_hover_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut code_block_copy_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut code_block_copy_buttons: Vec<CodeBlockCopyButtonPaint> = Vec::new();
        let mut image_zoom_button_targets: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut preview_zoom_buttons: Vec<CodeBlockCopyButtonPaint> = Vec::new();
        let mut preview_zoom_hitboxes = Vec::new();
        let preview_button_gap = 5.0;
        let mut unordered_list_markers: Vec<(WrappedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut inline_overlays: Vec<(ShapedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut preview_overlay_bounds_local: Option<Bounds<Pixels>> = None;
        let mut preview_overlay_image_bounds_local: Option<Bounds<Pixels>> = None;

        let hidden_lines: HashSet<usize> = {
            let mut set = HashSet::new();
            for &fold_line in &editor.folded_sections {
                if let Some(range) = editor.fold_range_for_line(fold_line) {
                    for l in range {
                        set.insert(l);
                    }
                }
            }
            set
        };

        let text_style = window.text_style();
        let editor_styles = &editor.styles.editor;
        let configured_font_family = |configured: &'static str| {
            if configured == ".SystemUIFont" {
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
                px(editor.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                f32::from(test_lines[0].width())
            } else {
                editor.base_font_size() * 0.6
            }
        };

        if buffer.text().is_empty() && editor.active_tab_is_markdown() {
            let placeholder = "键入 / 命令或右键插入格式...";
            let placeholder_runs = inline_overlay_runs(
                placeholder,
                &body_font,
                &body_cjk_font,
                crate::theme::with_alpha(
                    theme.text_secondary,
                    editor.styles.editor.editor_empty_placeholder_opacity,
                ),
            );
            let shaped = window.text_system().shape_line(
                SharedString::from(placeholder),
                px(editor.base_font_size()),
                &placeholder_runs,
                None,
            );
            inline_overlays.push((
                shaped,
                point(px(editor_left + editor.padding_left()), px(y)),
                px(editor.visual_height_for_line(0)),
            ));
        }
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
                px(editor.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                f32::from(test_lines[0].width()).max(1.0)
            } else {
                base_char_width.max(1.0)
            }
        };
        let show_non_markdown_indent_guides = editor.active_tab_is_non_markdown_text();
        let indent_guide_step_columns = if show_non_markdown_indent_guides {
            editor.cached_non_markdown_indent_guide_step()
        } else {
            0
        };
        let indent_guide_color = editor.styles.editor.editor_indent_guide_color;
        let indent_guide_width = editor.scaled(editor.styles.editor.editor_indent_guide_width_px);
        let indent_guide_dash_len =
            editor.scaled(editor.styles.editor.editor_indent_guide_dash_length_px);
        let indent_guide_gap_len =
            editor.scaled(editor.styles.editor.editor_indent_guide_gap_length_px);
        let non_markdown_cached_wrap_width = if editor.active_tab_is_non_markdown_text()
            && editor.editor_wraps_to_available_width()
        {
            let letter_spacing = editor_styles.editor_letter_spacing_px;
            Some(if letter_spacing > 0.0 {
                wrap_width * editor.base_font_size() / (editor.base_font_size() + letter_spacing)
            } else {
                wrap_width
            })
        } else {
            None
        };
        let defer_offscreen_wrap_reflow = editor.active_tab_is_non_markdown_text()
            && editor.editor_wraps_to_available_width()
            && wrap_width_changed;
        let mut deferred_offscreen_wrap_lines = false;
        let mut remaining_offscreen_wrap_reflows = if editor.active_tab_is_non_markdown_text()
            && editor.editor_wraps_to_available_width()
            && !wrap_width_changed
        {
            OFFSCREEN_WRAP_REFLOW_BUDGET_PER_FRAME
        } else {
            0
        };

        let image_cache = &editor.image_cache;
        let window_scale_factor = window.scale_factor().max(1.0);
        let table_cell_padding_px = editor.scaled(editor.styles.table.table_cell_padding_px);
        let table_cell_padding_chars = if table_cell_padding_px <= 0.0 {
            0
        } else {
            (table_cell_padding_px / base_char_width).ceil().max(1.0) as usize
        };
        let checkbox_size_px =
            editor.base_font_size() * editor.styles.checkbox.checkbox_size_factor;
        let checkbox_text_gap_px = editor.scaled(editor.styles.checkbox.checkbox_text_gap_px);
        let task_source_indent_step_chars =
            editor.styles.checkbox.checkbox_parent_prefix_chars as usize;
        let collapsed_task_prefix_display_columns = helpers::task_prefix_display_columns(
            body_space_width,
            checkbox_size_px,
            checkbox_text_gap_px,
        );
        let checkbox_indent_step_display_columns = collapsed_task_prefix_display_columns;

        let is_code_block_type = |i: usize| {
            matches!(
                decorations.get(i).map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            )
        };

        let code_block_group_range = |line_idx: usize| {
            if !is_code_block_type(line_idx) {
                return None;
            }

            let mut start = line_idx;
            while start > 0 && is_code_block_type(start - 1) {
                start -= 1;
            }

            let mut end = line_idx;
            while end + 1 < line_count && is_code_block_type(end + 1) {
                end += 1;
            }

            Some((start, end))
        };

        let active_code_fence_group = if cursor_line < line_count
            && matches!(
                decorations.get(cursor_line).map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            ) {
            code_block_group_range(cursor_line)
        } else {
            None
        };
        let active_front_matter_group = decorations
            .get(cursor_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .filter(|table_info| table_info.is_front_matter)
            .map(|table_info| {
                (
                    table_info.table_start_line,
                    table_info.table_end_line.saturating_sub(1),
                )
            });
        let active_markdown_indent_guide_step_columns = active_code_fence_group
            .map(|(start, end)| detect_indent_guide_step_in_range(buffer, start, end + 1))
            .or_else(|| {
                active_front_matter_group
                    .map(|(start, end)| detect_indent_guide_step_in_range(buffer, start, end + 1))
            })
            .unwrap_or(0);

        let selection_match_ranges = editor.selection_match_ranges();
        let bracket_match_ranges = editor.bracket_match_ranges();

        for line_idx in render_start_line..render_end_line {
            let is_cursor_line_early = line_idx == cursor_line;
            let line_has_block_preview = decorations
                .get(line_idx)
                .and_then(|decoration| decoration.block_preview.as_ref())
                .is_some();
            let cached_line = editor
                .line_display_info
                .get(line_idx)
                .and_then(|info| info.as_ref());
            let mut cached_height_before_reflow = None;
            let mut cached_line_above_viewport = false;
            let can_reuse_non_markdown_cached_line = editor.active_tab_is_non_markdown_text()
                && !is_cursor_line_early
                && !line_has_block_preview
                && cached_line.is_some_and(|cached| {
                    non_markdown_cached_line_can_reuse(cached, non_markdown_cached_wrap_width)
                });
            if !is_cursor_line_early && !line_has_block_preview {
                if let Some(cached) = cached_line {
                    let cached_h =
                        cached.line_height * cached.visual_line_count as f32 + cached.extra_height;
                    let effective_scroll_offset = scroll_offset + scroll_anchor_delta;
                    let est_top = y - effective_scroll_offset;
                    let est_bottom = est_top + cached_h;
                    cached_height_before_reflow = Some(cached_h);
                    cached_line_above_viewport = est_bottom < origin_y;
                    let is_offscreen =
                        est_bottom < origin_y || est_top > origin_y + viewport_height;
                    let is_stale_offscreen_non_markdown_line = editor
                        .active_tab_is_non_markdown_text()
                        && editor.editor_wraps_to_available_width()
                        && is_offscreen
                        && !can_reuse_non_markdown_cached_line;
                    let should_defer_stale_offscreen_non_markdown_line =
                        is_stale_offscreen_non_markdown_line
                            && (defer_offscreen_wrap_reflow
                                || remaining_offscreen_wrap_reflows == 0);
                    let can_skip_offscreen = if editor.active_tab_is_non_markdown_text() {
                        can_reuse_non_markdown_cached_line
                            || should_defer_stale_offscreen_non_markdown_line
                    } else {
                        has_valid_cache
                    };

                    if is_offscreen && can_skip_offscreen {
                        if should_defer_stale_offscreen_non_markdown_line {
                            deferred_offscreen_wrap_lines = true;
                        }
                        display_info_updates.push((line_idx, Some(cached.clone())));
                        y += cached_h;
                        continue;
                    }

                    if is_stale_offscreen_non_markdown_line
                        && !defer_offscreen_wrap_reflow
                        && remaining_offscreen_wrap_reflows > 0
                    {
                        remaining_offscreen_wrap_reflows -= 1;
                    }
                }
            }

            let decoration = decorations.get(line_idx).cloned().unwrap_or_default();
            let block_type = &decoration.block_type;
            let font_size = editor.font_size_for_block(block_type);
            let line_height = editor.line_height_for_block(block_type);
            let (block_padding_top, block_padding_bottom) = editor.block_padding_for_line(line_idx);

            if hidden_lines.contains(&line_idx) {
                let line_content = buffer.line_content(line_idx);
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|ls| ls.into_iter().next());
                display_info_updates.push((
                    line_idx,
                    wrapped_line.map(|wl| {
                        let unwrapped_width_px = f32::from(wl.unwrapped_layout.width);
                        LineDisplayInfo {
                            display_text: String::new(),
                            display_to_buffer: Vec::new(),
                            buffer_to_display: vec![0; line_content.len()],
                            base_runs: Vec::new(),
                            font_size,
                            line_height: 0.0,
                            visual_line_count: 0,
                            wrapped_line: wl,
                            wrap_width_px: None,
                            unwrapped_width_px,
                            wrap_boundary_upstream_xs: HashMap::new(),
                            extra_height: 0.0,
                            indent_px: 0.0,
                            content_x: editor_left + editor.padding_left(),
                            selection_padding_left_px: 0.0,
                        }
                    }),
                ));
                continue;
            }
            let is_cursor_line = line_idx == cursor_line;
            let estimated_total_line_height =
                line_height + block_padding_top + block_padding_bottom;
            let effective_scroll_offset = scroll_offset + scroll_anchor_delta;
            let estimated_line_top = y - effective_scroll_offset;
            let should_defer_uncached_offscreen_non_markdown_line = editor
                .active_tab_is_non_markdown_text()
                && !is_cursor_line
                && !line_has_block_preview
                && cached_line.is_none()
                && estimated_line_top > origin_y + viewport_height
                && (!editor.editor_wraps_to_available_width()
                    || defer_offscreen_wrap_reflow
                    || remaining_offscreen_wrap_reflows == 0);

            if should_defer_uncached_offscreen_non_markdown_line {
                if editor.editor_wraps_to_available_width() {
                    deferred_offscreen_wrap_lines = true;
                }
                display_info_updates.push((line_idx, None));
                y += estimated_total_line_height;
                continue;
            }

            let line_content = buffer.line_content(line_idx);
            let line_start_offset = buffer.line_start(line_idx);
            let line_end_offset = buffer.line_end(line_idx);
            let cursor_in_table = if let Some(ref ti) = decoration.table_info {
                cursor_line >= ti.table_start_line && cursor_line < ti.table_end_line
            } else {
                false
            };
            let heading_content_start = matches!(decoration.block_type, BlockType::Heading(_))
                .then(|| {
                    editor
                        .line_content_start_col(line_idx)
                        .min(line_content.len())
                });
            let ime_active_on_line = editor.marked_range.as_ref().is_some_and(|marked| {
                marked.start < line_end_offset && marked.end > line_start_offset
            });
            let cursor_col = if editor.cursor.has_selection()
                && matches!(decoration.block_type, BlockType::Heading(_))
            {
                // Only expand heading syntax if the cursor itself is on this heading line,
                // not when selection merely passes through.
                if is_cursor_line {
                    Some(cursor.offset.saturating_sub(line_start_offset))
                } else {
                    None
                }
            } else if editor.is_selecting && editor.cursor.has_selection() {
                let sel = editor.cursor.selection_range();
                if sel.start >= line_start_offset && sel.end <= line_end_offset {
                    let sel_start_col = sel.start - line_start_offset;
                    let sel_end_col = sel.end - line_start_offset;
                    let within_syntax = decoration.inline_spans.iter().any(|span| {
                        let prefix_len = match span.kind {
                            crate::markdown::InlineKind::Bold => 2,
                            crate::markdown::InlineKind::Italic => 1,
                            crate::markdown::InlineKind::Strikethrough => 2,
                            crate::markdown::InlineKind::InlineCode => 1,
                            crate::markdown::InlineKind::Link
                            | crate::markdown::InlineKind::Image => {
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
                            .filter(|sr| sr.range.end > span.range.end)
                            .map(|sr| sr.range.end)
                            .max()
                            .unwrap_or(span.range.end);
                        sel_start_col >= full_start && sel_end_col <= full_end
                    });
                    if within_syntax {
                        Some(cursor.offset.saturating_sub(line_start_offset))
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
                            cursor
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
                Some(cursor.offset.saturating_sub(line_start_offset))
            } else if cursor_in_table {
                Some(line_content.len())
            } else {
                None
            };
            let mut active_ranges: Vec<std::ops::Range<usize>> = all_sel_ranges
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
                .collect();
            for marked in editor.marked_ranges() {
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
            if active_ranges.is_empty()
                && let Some(col) = cursor_col
            {
                let clamped = col.min(line_content.len());
                active_ranges.push(clamped..clamped);
            }

            // Hide all non-first-row table lines (rendered as block on the first row)
            let hide_table_line = matches!(block_type, BlockType::Table)
                && decoration.table_info.as_ref().is_some_and(|ti| {
                    // Hide separator rows and all data rows.
                    // Only the first row (header) is kept as anchor for real tables.
                    !ti.is_front_matter && (ti.is_separator || !ti.is_header)
                });
            if hide_table_line {
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next());
                display_info_updates.push((
                    line_idx,
                    wrapped_line.map(|wrapped_line| {
                        let unwrapped_width_px = f32::from(wrapped_line.unwrapped_layout.width);
                        LineDisplayInfo {
                            display_text: String::new(),
                            display_to_buffer: Vec::new(),
                            buffer_to_display: vec![0; line_content.len()],
                            base_runs: Vec::new(),
                            font_size,
                            line_height: 0.0,
                            visual_line_count: 0,
                            wrapped_line,
                            wrap_width_px: None,
                            unwrapped_width_px,
                            wrap_boundary_upstream_xs: HashMap::new(),
                            extra_height: 0.0,
                            indent_px: 0.0,
                            content_x: editor_left + editor.padding_left(),
                            selection_padding_left_px: 0.0,
                        }
                    }),
                ));
                continue;
            }

            let is_front_matter_line = decoration
                .table_info
                .as_ref()
                .is_some_and(|ti| ti.is_front_matter);

            let table_width_chars = if *block_type == BlockType::Table && !is_front_matter_line {
                Some((text_width / base_char_width) as usize)
            } else {
                None
            };

            if editor.active_tab_is_non_markdown_text()
                && !is_cursor_line_early
                && can_reuse_non_markdown_cached_line
                && let Some(cached) = cached_line
            {
                let font_size = cached.font_size;
                let line_height = cached.line_height;
                let visual_line_count = cached.visual_line_count;
                let text_line_height = line_height * visual_line_count as f32;
                let total_line_height = text_line_height + cached.extra_height;
                let effective_scroll_offset = scroll_offset + scroll_anchor_delta;
                let line_top = y - effective_scroll_offset;
                let line_bottom = line_top + total_line_height;
                let text_line_top = line_top;
                let content_x = editor_left + editor.padding_left();

                display_info_updates.push((
                    line_idx,
                    Some(LineDisplayInfo {
                        display_text: cached.display_text.clone(),
                        display_to_buffer: cached.display_to_buffer.clone(),
                        buffer_to_display: cached.buffer_to_display.clone(),
                        base_runs: cached.base_runs.clone(),
                        font_size,
                        line_height,
                        visual_line_count,
                        wrapped_line: cached.wrapped_line.clone(),
                        wrap_width_px: cached.wrap_width_px,
                        unwrapped_width_px: cached.unwrapped_width_px,
                        wrap_boundary_upstream_xs: cached.wrap_boundary_upstream_xs.clone(),
                        extra_height: cached.extra_height,
                        indent_px: cached.indent_px,
                        content_x,
                        selection_padding_left_px: cached.selection_padding_left_px,
                    }),
                ));

                if line_bottom < origin_y || line_top > origin_y + viewport_height {
                    y += total_line_height;
                    _last_line_height = total_line_height;
                    continue;
                }

                let mut runs = cached.base_runs.clone();
                helpers::apply_search_match_text_styles(
                    &mut runs,
                    &cached.display_text,
                    &cached.display_to_buffer,
                    line_start_offset,
                    &editor.search_matches,
                    editor.search_current_index,
                    editor.styles.search.search_match_text_color,
                    editor.styles.search.search_match_active_text_color,
                );
                if editor.current_file_path == editor.workspace_search_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &cached.display_text,
                        &cached.display_to_buffer,
                        line_start_offset,
                        &editor.workspace_search_highlight_ranges,
                        editor.workspace_search_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                if editor.current_file_path == editor.outline_flash_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &cached.display_text,
                        &cached.display_to_buffer,
                        line_start_offset,
                        &editor.outline_flash_highlight_ranges,
                        editor.outline_flash_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                let origin = point(px(content_x), px(text_line_top));
                if show_non_markdown_indent_guides {
                    push_indent_guide_quads(
                        &mut indent_guide_quads,
                        line_content,
                        content_x,
                        line_top,
                        total_line_height,
                        base_char_width,
                        indent_guide_step_columns,
                        indent_guide_width,
                        indent_guide_dash_len,
                        indent_guide_gap_len,
                        indent_guide_color,
                    );
                }
                let is_active_line_number = active_line_numbers.contains(&line_idx);
                let skip_block_line_highlight = decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|ti| !ti.is_front_matter && line_idx == ti.table_start_line);
                push_whitespace_dot_quads(
                    &mut whitespace_dots,
                    &cached.display_text,
                    &cached.wrapped_line,
                    origin,
                    line_height,
                    editor.scaled(editor.styles.editor.editor_whitespace_dot_size_px),
                    editor.styles.editor.editor_whitespace_dot_color,
                    0,
                );

                let gutter_styles = &editor.styles.gutter;
                push_gutter_line_number_label(
                    &mut gutter_labels,
                    window,
                    line_idx,
                    is_active_line_number,
                    &monospace_font,
                    editor.scaled(gutter_styles.gutter_line_number_font_size_px),
                    gutter_right_x,
                    text_line_top,
                    line_height,
                    editor.scaled(gutter_styles.gutter_line_number_vertical_offset_px),
                    editor.scaled(gutter_styles.gutter_line_number_right_gap_px),
                    gutter_styles.gutter_line_number_text_color,
                    gutter_styles.gutter_line_number_active_text_color,
                );

                let fold_range = if hidden_lines.contains(&line_idx) {
                    None
                } else {
                    editor.fold_range_for_line(line_idx)
                };
                if fold_range.is_some() {
                    let fi_size = editor.scaled(gutter_styles.gutter_fold_icon_size_px);
                    let fi_hit_padding = editor.scaled(gutter_styles.gutter_fold_hit_padding_px);
                    let fi_x = editor.gutter_icon_x(editor_left, fi_size);
                    let fi_y = text_line_top + (line_height - fi_size) / 2.0;
                    let reveal = editor.gutter_fold_reveal.max(0.0);
                    let is_folded = editor.folded_sections.contains(&line_idx);
                    let is_hovered = editor.hovering_gutter_fold_area
                        || editor.hovering_fold_line == Some(line_idx);
                    let fi_alpha = if is_folded {
                        gutter_styles.gutter_fold_folded_opacity
                    } else if is_hovered {
                        gutter_styles.gutter_fold_hover_opacity * reveal
                    } else {
                        gutter_styles.gutter_fold_default_opacity * reveal
                    };
                    if fi_alpha > 0.0 {
                        let mut fi_color = if is_folded {
                            gutter_styles.gutter_fold_folded_color
                        } else {
                            gutter_styles.gutter_fold_hover_color
                        };
                        fi_color.a = fi_alpha;
                        gutter_fold_icons.push((
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size))),
                            if is_folded {
                                FOLD_SVG_PATH
                            } else {
                                FOLD_EXPANDED_SVG_PATH
                            },
                            fi_color,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        Bounds::new(
                            point(px(gutter_icon_column_left - fi_hit_padding), px(line_top)),
                            size(
                                px(gutter_icon_column_width + fi_hit_padding * 2.0),
                                px(total_line_height),
                            ),
                        ),
                        line_idx,
                        false,
                    ));
                }

                if let Some(diff_bg) = diff_line_background(editor, line_idx) {
                    diff_line_bgs.push(fill(
                        Bounds::new(
                            point(px(origin_x), px(line_top)),
                            size(px(viewport_width), px(total_line_height)),
                        ),
                        diff_bg,
                    ));
                }

                let selection_styles = &editor.styles.selection;
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.diff_removed_spans,
                    &cached.display_text,
                    line_content.len(),
                    &cached.buffer_to_display,
                    &runs,
                    &cached.wrapped_line,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    cached.indent_px,
                    selection_styles.selection_diff_removed_inline_bg,
                    selection_styles.selection_diff_removed_inline_border_color,
                    editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                    editor_styles.editor_letter_spacing_px,
                    window,
                );
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.diff_added_spans,
                    &cached.display_text,
                    line_content.len(),
                    &cached.buffer_to_display,
                    &runs,
                    &cached.wrapped_line,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    cached.indent_px,
                    selection_styles.selection_diff_added_inline_bg,
                    selection_styles.selection_diff_added_inline_border_color,
                    editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                    editor_styles.editor_letter_spacing_px,
                    window,
                );

                if editor.selected_handle.is_none()
                    && is_cursor_line
                    && !any_selection
                    && editor.extra_cursors.is_empty()
                    && !skip_block_line_highlight
                {
                    line_highlight = Some(fill(
                        Bounds::new(
                            point(px(origin_x), px(line_top)),
                            size(px(viewport_width), px(total_line_height)),
                        ),
                        theme.line_highlight,
                    ));
                }

                for sr in &all_sel_ranges {
                    if sr.start == sr.end {
                        continue;
                    }
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);

                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start = helpers::buffer_to_display_offset(
                            &cached.buffer_to_display,
                            buf_start_col,
                        );
                        let disp_end = helpers::buffer_to_display_offset(
                            &cached.buffer_to_display,
                            buf_end_col,
                        );
                        selection_quads.extend(helpers::build_selection_quads(
                            &cached.display_text,
                            &runs,
                            &cached.wrapped_line,
                            disp_start,
                            disp_end,
                            content_x,
                            text_line_top,
                            line_height,
                            font_size,
                            cached.indent_px,
                            cached.selection_padding_left_px,
                            theme.selection,
                            editor_styles.editor_letter_spacing_px,
                            window,
                        ));
                    }
                }

                if !editor.search_matches.is_empty() {
                    let match_bg = editor.styles.search.search_match_bg;
                    let active_bg = editor.styles.search.search_match_active_bg;
                    for (match_idx, sr) in editor.search_matches.iter().enumerate() {
                        let is_active = editor.search_current_index == Some(match_idx);
                        let sel_start_on_line = sr.start.max(line_start_offset);
                        let sel_end_on_line = sr.end.min(line_end_offset);
                        if sel_start_on_line < sel_end_on_line {
                            let buf_start_col = sel_start_on_line - line_start_offset;
                            let buf_end_col = sel_end_on_line - line_start_offset;
                            let disp_start = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_start_col,
                            );
                            let disp_end = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_end_col,
                            );
                            search_match_quads.extend(helpers::build_selection_quads(
                                &cached.display_text,
                                &runs,
                                &cached.wrapped_line,
                                disp_start,
                                disp_end,
                                content_x,
                                line_top,
                                line_height,
                                font_size,
                                cached.indent_px,
                                0.0,
                                if is_active { active_bg } else { match_bg },
                                editor_styles.editor_letter_spacing_px,
                                window,
                            ));
                        }
                    }
                }

                if editor.current_file_path == editor.workspace_search_highlight_path {
                    let match_bg = editor.styles.search.search_match_bg;
                    let active_bg = editor.styles.search.search_match_active_bg;
                    for (match_idx, sr) in
                        editor.workspace_search_highlight_ranges.iter().enumerate()
                    {
                        let is_active =
                            editor.workspace_search_highlight_current_index == Some(match_idx);
                        let sel_start_on_line = sr.start.max(line_start_offset);
                        let sel_end_on_line = sr.end.min(line_end_offset);
                        if sel_start_on_line < sel_end_on_line {
                            let buf_start_col = sel_start_on_line - line_start_offset;
                            let buf_end_col = sel_end_on_line - line_start_offset;
                            let disp_start = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_start_col,
                            );
                            let disp_end = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_end_col,
                            );
                            search_match_quads.extend(helpers::build_selection_quads(
                                &cached.display_text,
                                &runs,
                                &cached.wrapped_line,
                                disp_start,
                                disp_end,
                                content_x,
                                line_top,
                                line_height,
                                font_size,
                                cached.indent_px,
                                0.0,
                                if is_active { active_bg } else { match_bg },
                                editor_styles.editor_letter_spacing_px,
                                window,
                            ));
                        }
                    }
                }
                if editor.current_file_path == editor.outline_flash_highlight_path {
                    let match_bg = editor.styles.search.search_match_bg;
                    let active_bg = editor.styles.search.search_match_active_bg;
                    for (match_idx, sr) in editor.outline_flash_highlight_ranges.iter().enumerate()
                    {
                        let is_active =
                            editor.outline_flash_highlight_current_index == Some(match_idx);
                        let sel_start_on_line = sr.start.max(line_start_offset);
                        let sel_end_on_line = sr.end.min(line_end_offset);
                        if sel_start_on_line < sel_end_on_line {
                            let buf_start_col = sel_start_on_line - line_start_offset;
                            let buf_end_col = sel_end_on_line - line_start_offset;
                            let disp_start = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_start_col,
                            );
                            let disp_end = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_end_col,
                            );
                            search_match_quads.extend(helpers::build_selection_quads(
                                &cached.display_text,
                                &runs,
                                &cached.wrapped_line,
                                disp_start,
                                disp_end,
                                content_x,
                                line_top,
                                line_height,
                                font_size,
                                cached.indent_px,
                                0.0,
                                if is_active { active_bg } else { match_bg },
                                editor_styles.editor_letter_spacing_px,
                                window,
                            ));
                        }
                    }
                }

                if editor.selected_handle.is_none()
                    && window.is_window_active()
                    && editor.focus_handle.is_focused(window)
                    && editor.focused_sidebar_path.is_none()
                    && (editor.cursor_opacity > 0.0 || editor.is_selecting)
                {
                    for &cur_offset in &all_cursor_offsets {
                        if cur_offset < line_start_offset || cur_offset > line_end_offset {
                            continue;
                        }
                        let buf_cursor_col = cur_offset.saturating_sub(line_start_offset);
                        let buf_cursor_col = buf_cursor_col.min(line_content.len());
                        let disp_cursor_col = if buf_cursor_col >= line_content.len() {
                            cached.display_text.len()
                        } else {
                            helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_cursor_col,
                            )
                        };

                        let cursor_pos = cached
                            .wrapped_line
                            .position_for_index(disp_cursor_col, px(line_height))
                            .unwrap_or_default();
                        let cursor_pos = match editor.wrap_boundary_affinity {
                            Some(WrapBoundaryAffinity {
                                offset: affinity_offset,
                                kind: WrapBoundaryAffinityKind::Upstream,
                            }) if affinity_offset == cur_offset => upstream_wrap_boundary_position(
                                &cached.wrapped_line,
                                &cached.display_text,
                                disp_cursor_col,
                                &cached.wrap_boundary_upstream_xs,
                                line_height,
                            )
                            .unwrap_or(cursor_pos),
                            Some(WrapBoundaryAffinity {
                                offset: affinity_offset,
                                kind: WrapBoundaryAffinityKind::Downstream,
                            }) if affinity_offset == cur_offset => {
                                downstream_wrap_boundary_position(
                                    &cached.wrapped_line,
                                    disp_cursor_col,
                                    line_height,
                                )
                                .unwrap_or(cursor_pos)
                            }
                            _ => cursor_pos,
                        };
                        let cursor_height =
                            line_height * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
                        let cursor_top = text_line_top
                            + f32::from(cursor_pos.y)
                            + (line_height - cursor_height) / 2.0;
                        let mut cursor_color = theme.cursor;
                        cursor_color.a *= if editor.is_selecting {
                            editor.styles.cursor.cursor_opacity_max
                        } else {
                            editor.cursor_opacity
                        };
                        cursor_quads.push(fill(
                            Bounds::new(
                                point(px(content_x) + cursor_pos.x, px(cursor_top)),
                                size(px(editor.styles.cursor.cursor_width_px), px(cursor_height)),
                            ),
                            cursor_color,
                        ));
                    }
                }

                if let Some(ref marked) = editor.marked_range {
                    let mark_start_on_line = marked.start.max(line_start_offset);
                    let mark_end_on_line = marked.end.min(line_end_offset);
                    if mark_start_on_line < mark_end_on_line {
                        let buf_start = mark_start_on_line - line_start_offset;
                        let buf_end = mark_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&cached.buffer_to_display, buf_start);
                        let disp_end =
                            helpers::buffer_to_display_offset(&cached.buffer_to_display, buf_end);
                        let pos_start = cached
                            .wrapped_line
                            .position_for_index(disp_start, px(line_height))
                            .unwrap_or_default();
                        let pos_end = cached
                            .wrapped_line
                            .position_for_index(disp_end, px(line_height))
                            .unwrap_or_default();
                        let underline_y =
                            text_line_top + f32::from(pos_start.y) + line_height - 2.0;
                        marked_underlines.push(fill(
                            Bounds::from_corners(
                                point(px(content_x) + pos_start.x, px(underline_y)),
                                point(px(content_x) + pos_end.x, px(underline_y + 1.0)),
                            ),
                            theme.text_primary,
                        ));
                    }
                }

                lines.push(PrepaintLine {
                    wrapped: cached.wrapped_line.clone(),
                    origin,
                    line_height: px(line_height),
                    segments: None,
                    is_table_block: false,
                });

                y += total_line_height;
                _last_line_height = total_line_height;
                continue;
            }
            let active_tab_is_diff = editor.active_tab_is_diff();
            let code_block_syntax_active = active_code_fence_group
                .is_some_and(|(start, end)| line_idx >= start && line_idx <= end)
                || active_front_matter_group
                    .is_some_and(|(start, end)| line_idx >= start && line_idx <= end);
            let diff_table_syntax_line = active_tab_is_diff
                && decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|ti| !ti.is_front_matter);
            let block_preview_active = if active_tab_is_diff {
                decoration.block_preview.is_some()
            } else {
                decoration.block_preview.as_ref().is_some_and(|preview| {
                    helpers::block_preview_is_active(preview, cursor_line, &all_sel_ranges)
                })
            };
            let collapsed_block_preview = decoration
                .block_preview
                .as_ref()
                .filter(|preview| !editor.folded_sections.contains(&preview.start_line))
                .filter(|_| !block_preview_active);
            if let Some(preview) = collapsed_block_preview
                && !preview.is_anchor
            {
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next());
                display_info_updates.push((
                    line_idx,
                    wrapped_line.map(|wrapped_line| {
                        let unwrapped_width_px = f32::from(wrapped_line.unwrapped_layout.width);
                        LineDisplayInfo {
                            display_text: String::new(),
                            display_to_buffer: Vec::new(),
                            buffer_to_display: vec![0; line_content.len()],
                            base_runs: Vec::new(),
                            font_size,
                            line_height: 0.0,
                            visual_line_count: 0,
                            wrapped_line,
                            wrap_width_px: None,
                            unwrapped_width_px,
                            wrap_boundary_upstream_xs: HashMap::new(),
                            extra_height: 0.0,
                            indent_px: 0.0,
                            content_x: editor_left + editor.padding_left(),
                            selection_padding_left_px: 0.0,
                        }
                    }),
                ));
                continue;
            }
            let (mut display_text, display_to_buffer, buffer_to_display, prefix_collapsed) =
                if collapsed_block_preview.is_some() {
                    (
                        String::new(),
                        Vec::new(),
                        vec![0; line_content.len()],
                        false,
                    )
                } else {
                    let render_decoration =
                        diff_table_syntax_line.then(|| crate::markdown::LineDecoration {
                            block_type: BlockType::CodeBlock,
                            diff_added_spans: decoration.diff_added_spans.clone(),
                            diff_removed_spans: decoration.diff_removed_spans.clone(),
                            ..crate::markdown::LineDecoration::default()
                        });
                    let render_decoration = render_decoration.as_ref().unwrap_or(&decoration);
                    helpers::build_display_text(
                        line_content,
                        render_decoration,
                        &active_ranges,
                        code_block_syntax_active,
                        table_width_chars,
                        table_cell_padding_chars,
                        collapsed_task_prefix_display_columns,
                        checkbox_indent_step_display_columns,
                        editor.styles.checkbox.checkbox_parent_prefix_chars as usize,
                        0,
                        editor.styles.editor.editor_superscript_scale_factor,
                        editor.styles.editor.editor_subscript_scale_factor,
                        editor.styles.editor.editor_superscript_scale_factor,
                    )
                };
            let render_decoration =
                diff_table_syntax_line.then(|| crate::markdown::LineDecoration {
                    block_type: BlockType::CodeBlock,
                    diff_added_spans: decoration.diff_added_spans.clone(),
                    diff_removed_spans: decoration.diff_removed_spans.clone(),
                    ..crate::markdown::LineDecoration::default()
                });
            let render_decoration = render_decoration.as_ref().unwrap_or(&decoration);
            editor.apply_list_display_styles(
                line_content,
                render_decoration,
                &mut display_text,
                prefix_collapsed,
            );

            let runs = if collapsed_block_preview.is_some() {
                vec![TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }]
            } else {
                let mut runs = helpers::build_text_runs_for_display(
                    &display_text,
                    line_content,
                    &display_to_buffer,
                    render_decoration,
                    &active_ranges,
                    code_block_syntax_active,
                    prefix_collapsed,
                    &body_font,
                    &body_cjk_font,
                    &monospace_font,
                    &heading_font,
                    &heading_cjk_font,
                    collapsed_task_prefix_display_columns,
                    font_size,
                    theme,
                    editor.styles.selection.selection_diff_added_inline_bg,
                    editor.styles.selection.selection_diff_removed_inline_bg,
                    editor.styles.wiki_link.wiki_link_text_color,
                    editor.styles.wiki_link.wiki_link_text_weight,
                    Some(editor.styles.table.front_matter_key_text_color),
                );
                helpers::apply_search_match_text_styles(
                    &mut runs,
                    &display_text,
                    &display_to_buffer,
                    line_start_offset,
                    &editor.search_matches,
                    editor.search_current_index,
                    editor.styles.search.search_match_text_color,
                    editor.styles.search.search_match_active_text_color,
                );
                if editor.current_file_path == editor.workspace_search_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &display_text,
                        &display_to_buffer,
                        line_start_offset,
                        &editor.workspace_search_highlight_ranges,
                        editor.workspace_search_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                if editor.current_file_path == editor.outline_flash_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &display_text,
                        &display_to_buffer,
                        line_start_offset,
                        &editor.outline_flash_highlight_ranges,
                        editor.outline_flash_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                if diff_table_syntax_line && let Some(table_info) = decoration.table_info.as_ref() {
                    let syntax_ranges = diff_table_syntax_color_ranges(line_content, table_info);
                    apply_display_text_color_ranges(
                        &mut runs,
                        &display_text,
                        &syntax_ranges,
                        theme.syntax_marker,
                    );
                }
                if let Some(text_color) = diff_hunk_label_text_color(editor, line_idx) {
                    for run in &mut runs {
                        run.color = text_color;
                    }
                }
                runs
            };

            let display_shared: SharedString = if display_text.is_empty() {
                " ".into()
            } else {
                display_text.clone().into()
            };

            let is_list_type = matches!(
                block_type,
                BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
            );
            let prefix_bytes = if is_list_type {
                helpers::compute_display_prefix_bytes(
                    &display_text,
                    block_type,
                    prefix_collapsed,
                    collapsed_task_prefix_display_columns,
                )
            } else {
                0
            };
            let wrap_indent_px =
                if is_list_type && prefix_bytes > 0 && prefix_bytes <= display_text.len() {
                    let prefix_text = &display_text[..prefix_bytes];
                    let prefix_runs = helpers::extract_runs_for_range(&runs, 0, prefix_bytes);
                    let prefix_shared: SharedString = if prefix_text.is_empty() {
                        " ".into()
                    } else {
                        prefix_text.to_string().into()
                    };
                    window
                        .text_system()
                        .shape_text(prefix_shared, px(font_size), &prefix_runs, None, None)
                        .ok()
                        .and_then(|lines| lines.into_iter().next())
                        .map(|line| f32::from(line.width()))
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
            let letter_spacing = editor_styles.editor_letter_spacing_px;

            let base_wrap_width =
                if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence)
                    || is_front_matter_line
                {
                    (text_width
                        - editor.scaled(editor.styles.code_block.code_block_text_padding_left_px)
                        - editor.scaled(editor.styles.code_block.code_block_wrap_right_padding_px))
                    .max(editor.scaled(80.0))
                } else {
                    wrap_width
                };

            let effective_wrap_width = if wrap_indent_px > 0.0 {
                (base_wrap_width - wrap_indent_px).max(base_wrap_width * 0.3)
            } else {
                base_wrap_width
            };

            let effective_wrap_width = if letter_spacing > 0.0 {
                effective_wrap_width * font_size / (font_size + letter_spacing)
            } else {
                effective_wrap_width
            };

            let wrap_width_param = if *block_type == BlockType::Table && !is_front_matter_line {
                None
            } else if !editor.editor_wraps_to_available_width() {
                None
            } else {
                Some(px(effective_wrap_width))
            };

            let Ok(mut wrapped_lines) = window.text_system().shape_text(
                display_shared,
                px(font_size),
                &runs,
                wrap_width_param,
                None,
            ) else {
                display_info_updates.push((line_idx, None));
                y += estimated_total_line_height;
                continue;
            };

            if letter_spacing != 0.0 {
                for w in wrapped_lines.iter_mut() {
                    helpers::apply_letter_spacing(w, letter_spacing);
                }
            }

            let wrapped = &wrapped_lines[0];
            let indent_px =
                if is_list_type && prefix_bytes > 0 && prefix_bytes <= display_text.len() {
                    wrapped
                        .position_for_index(prefix_bytes, px(line_height))
                        .map(|pos| f32::from(pos.x))
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
            let visual_line_count = wrapped.wrap_boundaries().len() + 1;
            let text_line_height = line_height * visual_line_count as f32;
            let image_gap_top = editor.scaled(editor.styles.image_preview.image_gap_top_px);
            let image_gap_bottom = editor.scaled(editor.styles.image_preview.image_gap_bottom_px);
            let generated_preview_gap_bottom = text_line_height + image_gap_top;
            let image_styles = &editor.styles.image_preview;
            let image_corner_radius = px(editor.scaled(image_styles.image_border_radius_px));
            let image_border_width = px(editor.scaled(image_styles.image_active_border_width_px));

            let image_active = decoration.image_path.is_some()
                && helpers::is_image_syntax_active(
                    &decoration,
                    line_content,
                    line_start_offset,
                    cursor.offset,
                    &sel_range,
                );

            let image_extra_height = if let Some(ref path) = decoration.image_path {
                if editor.folded_image_previews.contains(&line_idx) {
                    0.0
                } else if let Some(Some(img_data)) = image_cache.get(path) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0 && img_h > 0.0 {
                        let (_, display_h) = helpers::compute_image_preview_size(
                            img_w,
                            img_h,
                            text_width,
                            window_scale_factor,
                        );
                        display_h + image_gap_top + image_gap_bottom
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };
            let generated_preview_extra_height = if let Some(preview) = collapsed_block_preview {
                if let Some(Some(img_data)) = image_cache.get(&preview.cache_key) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0 && img_h > 0.0 {
                        let (_, display_h) = helpers::compute_generated_preview_size_for_kind(
                            img_w,
                            img_h,
                            text_width,
                            preview.kind,
                        );
                        display_h + image_gap_top + generated_preview_gap_bottom
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Compute table block extra height for header line (table rendered as block below)
            let table_extra_height = if *block_type == BlockType::Table && !active_tab_is_diff {
                if let Some(ref ti) = decoration.table_info {
                    if ti.is_header && !ti.is_front_matter {
                        let layout = table_layout_cache
                            .entry(ti.table_start_line)
                            .or_insert_with(|| {
                                let table_styles = &editor.styles.table;
                                build_precise_table_layout(
                                    window,
                                    buffer,
                                    &decorations,
                                    ti.table_start_line,
                                    ti.table_end_line,
                                    text_width,
                                    editor.scaled(table_styles.table_cell_padding_px),
                                    editor.scaled(table_styles.table_cell_padding_px) * 0.6,
                                    base_char_width,
                                    line_height,
                                    editor.scaled(table_styles.table_handle_size_px),
                                    &body_font,
                                    &body_cjk_font,
                                    &monospace_font,
                                    &heading_font,
                                    &heading_cjk_font,
                                    font_size,
                                    theme,
                                    &all_cursor_offsets,
                                    editor.marked_range.as_ref(),
                                )
                            })
                            .clone();
                        // Subtract line_height since the header line's text area
                        // (which is hidden via is_table_block) already occupies one line_height.
                        (layout.total_height - line_height).max(0.0)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let extra_height = block_padding_top
                + block_padding_bottom
                + image_extra_height
                + generated_preview_extra_height
                + table_extra_height;
            let total_line_height = text_line_height + extra_height;

            if !suppress_scroll_anchor_compensation
                && editor.active_tab_is_non_markdown_text()
                && editor.editor_wraps_to_available_width()
                && !can_reuse_non_markdown_cached_line
                && cached_line_above_viewport
                && let Some(cached_h) = cached_height_before_reflow
            {
                scroll_anchor_delta += total_line_height - cached_h;
            }

            let effective_scroll_offset = scroll_offset + scroll_anchor_delta;
            let line_top = y - effective_scroll_offset;
            let line_bottom = line_top + total_line_height;
            let text_line_top = line_top + block_padding_top;
            let base_content_x =
                if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence)
                    || is_front_matter_line
                {
                    editor_left
                        + editor.padding_left()
                        + editor.scaled(editor.styles.code_block.code_block_text_padding_left_px)
                } else {
                    editor_left + editor.padding_left()
                };
            let leading_ws =
                line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
            let content_x = if matches!(block_type, BlockType::TaskList(_)) && prefix_collapsed {
                let target_prefix_width = helpers::task_prefix_target_width_px(
                    line_content,
                    font_size * editor.styles.checkbox.checkbox_size_factor,
                    checkbox_text_gap_px,
                    task_source_indent_step_chars,
                )
                .unwrap_or(indent_px);
                base_content_x + (target_prefix_width - indent_px)
            } else {
                base_content_x
            };
            let selection_padding_left_px =
                (content_x - (editor_left + editor.padding_left())).max(0.0);
            let wrap_boundary_upstream_xs = compute_wrap_boundary_upstream_xs(
                window,
                wrapped,
                &display_text,
                &runs,
                font_size,
                line_height,
                letter_spacing,
            );

            display_info_updates.push((
                line_idx,
                Some(LineDisplayInfo {
                    display_text: display_text.clone(),
                    display_to_buffer: display_to_buffer.clone(),
                    buffer_to_display: buffer_to_display.clone(),
                    base_runs: runs.clone(),
                    font_size,
                    line_height,
                    visual_line_count,
                    wrapped_line: wrapped.clone(),
                    wrap_width_px: wrapped.wrap_width.map(f32::from),
                    unwrapped_width_px: f32::from(wrapped.unwrapped_layout.width),
                    wrap_boundary_upstream_xs: wrap_boundary_upstream_xs.clone(),
                    extra_height,
                    indent_px,
                    content_x,
                    selection_padding_left_px,
                }),
            ));

            if line_bottom < origin_y || line_top > origin_y + viewport_height {
                if editor.active_tab_is_non_markdown_text()
                    && editor.editor_wraps_to_available_width()
                    && cached_line.is_none()
                    && line_top > origin_y + viewport_height
                    && remaining_offscreen_wrap_reflows > 0
                {
                    remaining_offscreen_wrap_reflows -= 1;
                }
                y += total_line_height;
                _last_line_height = total_line_height;
                continue;
            }

            let segments = if indent_px > 0.0 && wrapped.wrap_boundaries().len() > 0 {
                let byte_indices = helpers::wrap_boundary_byte_indices(wrapped);
                let mut shaped_segments: Vec<(WrappedLine, Point<Pixels>)> = Vec::new();
                let mut seg_start = 0;
                let seg_count = byte_indices.len() + 1;
                for seg_idx in 0..seg_count {
                    let seg_end = if seg_idx < byte_indices.len() {
                        byte_indices[seg_idx]
                    } else {
                        display_text.len()
                    };
                    let seg_text = &display_text[seg_start..seg_end];
                    let seg_runs = helpers::extract_runs_for_range(&runs, seg_start, seg_end);
                    let seg_shared: SharedString = if seg_text.is_empty() {
                        " ".into()
                    } else {
                        seg_text.to_string().into()
                    };
                    if let Ok(seg_wrapped_lines) = window.text_system().shape_text(
                        seg_shared,
                        px(font_size),
                        &seg_runs,
                        None,
                        None,
                    ) {
                        if let Some(mut seg_wrapped) = seg_wrapped_lines.into_iter().next() {
                            if letter_spacing != 0.0 {
                                helpers::apply_letter_spacing(&mut seg_wrapped, letter_spacing);
                            }
                            let seg_x = if seg_idx == 0 {
                                content_x
                            } else {
                                content_x + indent_px
                            };
                            let seg_y = text_line_top + seg_idx as f32 * line_height;
                            shaped_segments.push((seg_wrapped, point(px(seg_x), px(seg_y))));
                        }
                    }
                    seg_start = seg_end;
                }
                Some(shaped_segments)
            } else {
                None
            };

            let origin = point(px(content_x), px(text_line_top));
            let show_line_indent_guides =
                show_non_markdown_indent_guides || code_block_syntax_active;
            let line_indent_guide_step_columns = if show_non_markdown_indent_guides {
                indent_guide_step_columns
            } else if code_block_syntax_active {
                active_markdown_indent_guide_step_columns
            } else {
                0
            };
            if show_line_indent_guides {
                push_indent_guide_quads(
                    &mut indent_guide_quads,
                    line_content,
                    content_x,
                    line_top,
                    total_line_height,
                    base_char_width,
                    line_indent_guide_step_columns,
                    indent_guide_width,
                    indent_guide_dash_len,
                    indent_guide_gap_len,
                    indent_guide_color,
                );
            }
            let is_active_line_number = active_line_numbers.contains(&line_idx);
            let is_table_source_row = *block_type == BlockType::Table
                && !active_tab_is_diff
                && decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|ti| !ti.is_front_matter && !ti.is_separator);
            if !is_table_source_row {
                let whitespace_skip_prefix_bytes = if matches!(block_type, BlockType::TaskList(_)) {
                    prefix_bytes
                } else {
                    0
                };
                push_whitespace_dot_quads(
                    &mut whitespace_dots,
                    &display_text,
                    &wrapped,
                    origin,
                    line_height,
                    editor.scaled(editor.styles.editor.editor_whitespace_dot_size_px),
                    editor.styles.editor.editor_whitespace_dot_color,
                    whitespace_skip_prefix_bytes,
                );
            }

            // Code block-like background
            if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence)
                || is_front_matter_line
                || diff_table_syntax_line
            {
                let corner_r = px(editor.scaled(editor.styles.code_block.code_block_radius_px));
                let side_inset = if editor.theme_kind.is_dark() {
                    0.0
                } else {
                    editor.scaled(editor.styles.code_block.code_block_border_width_px)
                };
                let (group_start, group_end) = if is_front_matter_line || diff_table_syntax_line {
                    let table_info = decoration.table_info.as_ref();
                    let start = table_info.map(|ti| ti.table_start_line).unwrap_or(line_idx);
                    let end = table_info
                        .map(|ti| ti.table_end_line.saturating_sub(1))
                        .unwrap_or(line_idx);
                    (start, end)
                } else {
                    code_block_group_range(line_idx).unwrap_or((line_idx, line_idx))
                };
                let is_group_start = line_idx == group_start;
                let is_group_end = line_idx == group_end;
                let has_top_corner = is_group_start;
                let has_bottom_corner = is_group_end || collapsed_block_preview.is_some();
                let background_left = editor_left + editor.padding_left();
                let background_bottom = line_top + total_line_height;
                let fill_left = background_left + side_inset;
                let fill_width = (text_width - side_inset * 2.0).max(0.0);
                code_block_group_bounds
                    .entry(group_start)
                    .and_modify(|group| {
                        group.top = group.top.min(line_top);
                        group.bottom = group.bottom.max(background_bottom);
                    })
                    .or_insert(CodeBlockGroupBounds {
                        left: background_left,
                        top: line_top,
                        width: text_width,
                        bottom: background_bottom,
                    });
                if fill_width > 0.0 {
                    code_block_bgs.push(quad(
                        Bounds::new(
                            point(px(fill_left), px(line_top)),
                            size(px(fill_width), px(total_line_height)),
                        ),
                        Corners {
                            top_left: if has_top_corner { corner_r } else { px(0.0) },
                            top_right: if has_top_corner { corner_r } else { px(0.0) },
                            bottom_right: if has_bottom_corner { corner_r } else { px(0.0) },
                            bottom_left: if has_bottom_corner { corner_r } else { px(0.0) },
                        },
                        editor.styles.code_block.code_block_bg,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }

            // Table rendering (independent visual block on header line)
            if *block_type == BlockType::Table && !active_tab_is_diff {
                if let Some(ref ti) = decoration.table_info {
                    if ti.is_header && !ti.is_front_matter {
                        let table_styles = &editor.styles.table;
                        let border_color = table_styles.table_border_color;
                        let border_w = table_styles.table_border_width_px;
                        let table_left = editor_left + editor.padding_left();

                        // Get or compute the cached table layout
                        let layout = table_layout_cache
                            .entry(ti.table_start_line)
                            .or_insert_with(|| {
                                build_precise_table_layout(
                                    window,
                                    buffer,
                                    &decorations,
                                    ti.table_start_line,
                                    ti.table_end_line,
                                    text_width,
                                    table_cell_padding_px,
                                    table_cell_padding_px * 0.6,
                                    base_char_width,
                                    line_height,
                                    editor.scaled(table_styles.table_handle_size_px),
                                    &body_font,
                                    &body_cjk_font,
                                    &monospace_font,
                                    &heading_font,
                                    &heading_cjk_font,
                                    font_size,
                                    theme,
                                    &all_cursor_offsets,
                                    editor.marked_range.as_ref(),
                                )
                            })
                            .clone();

                        let table_width = layout.total_width.min(text_width);
                        // Table block starts at text_line_top (header text is hidden)
                        let table_top = text_line_top;
                        let outer_radius = px(table_styles.table_outer_radius_px);

                        // Selection highlight: blue border when selection covers table
                        let table_block_start_offset = buffer.line_start(ti.table_start_line);
                        let table_block_end_offset =
                            buffer.line_end(ti.table_end_line.saturating_sub(1));
                        let selection_overlaps_table = all_sel_ranges.iter().any(|sr| {
                            sr.start != sr.end
                                && sr.start < table_block_end_offset
                                && sr.end > table_block_start_offset
                        });
                        if selection_overlaps_table {
                            let sel_border_w = table_styles.table_selection_border_width_px;
                            table_borders.push(quad(
                                Bounds::new(
                                    point(
                                        px(table_left - sel_border_w),
                                        px(table_top - sel_border_w),
                                    ),
                                    size(
                                        px(table_width + sel_border_w * 2.0),
                                        px(layout.total_height + sel_border_w * 2.0),
                                    ),
                                ),
                                Corners::all(outer_radius),
                                gpui::transparent_black(),
                                px(sel_border_w),
                                theme.cursor,
                                BorderStyle::default(),
                            ));
                        }

                        // Draw outer table border once.
                        table_borders.push(quad(
                            Bounds::new(
                                point(px(table_left), px(table_top)),
                                size(px(table_width), px(layout.total_height)),
                            ),
                            Corners::all(outer_radius),
                            gpui::transparent_black(),
                            px(border_w),
                            border_color,
                            BorderStyle::default(),
                        ));

                        // Draw internal horizontal separators once.
                        let mut row_boundary_y = table_top;
                        for row_h in layout
                            .row_heights
                            .iter()
                            .take(layout.visual_row_count.saturating_sub(1))
                        {
                            row_boundary_y += *row_h;
                            table_borders.push(fill(
                                Bounds::new(
                                    point(px(table_left), px(row_boundary_y)),
                                    size(px(table_width), px(border_w)),
                                ),
                                border_color,
                            ));
                        }

                        // Draw internal vertical separators once.
                        if let Some(first_row) = layout.cells.first() {
                            for cell in first_row.iter().take(layout.col_count.saturating_sub(1)) {
                                let separator_x = table_left + cell.x + cell.width;
                                table_borders.push(fill(
                                    Bounds::new(
                                        point(px(separator_x), px(table_top)),
                                        size(px(border_w), px(layout.total_height)),
                                    ),
                                    border_color,
                                ));
                            }
                        }

                        // Render ALL visual rows of the table block
                        for vi in 0..layout.visual_row_count {
                            let row_y = table_top + layout.row_heights.iter().take(vi).sum::<f32>();
                            let row_h = layout.row_heights.get(vi).copied().unwrap_or(line_height);

                            // Header background
                            if layout.is_header.get(vi).copied().unwrap_or(false) {
                                table_bgs.push(fill(
                                    Bounds::new(
                                        point(px(table_left), px(row_y)),
                                        size(px(table_width), px(row_h)),
                                    ),
                                    table_styles.table_header_bg,
                                ));
                            }

                            // Get the source line info for this visual row
                            let source_line = layout
                                .visual_row_to_line
                                .get(vi)
                                .copied()
                                .unwrap_or(line_idx);
                            let row_ti = decorations
                                .get(source_line)
                                .and_then(|d| d.table_info.as_ref());
                            // Per-cell text shaping
                            if let Some(row_cells) = layout.cells.get(vi) {
                                for cell_layout in row_cells {
                                    let cell_content = row_ti
                                        .and_then(|t| t.cells.get(cell_layout.col))
                                        .cloned()
                                        .unwrap_or_default();

                                    let cell_text: SharedString =
                                        if cell_layout.display_text.is_empty() {
                                            " ".into()
                                        } else {
                                            cell_layout.display_text.clone().into()
                                        };

                                    let cell_decoration =
                                        crate::markdown::parse_decorations(&cell_content)
                                            .into_iter()
                                            .next()
                                            .unwrap_or_default();

                                    let cell_runs = helpers::build_text_runs_for_display(
                                        &cell_layout.display_text,
                                        &cell_content,
                                        &cell_layout.display_to_buffer,
                                        &cell_decoration,
                                        &helpers::active_table_cell_ranges(
                                            buffer.line_start(source_line),
                                            &cell_layout.cell_range,
                                            &all_cursor_offsets,
                                            editor.marked_range.as_ref(),
                                        ),
                                        false,
                                        false,
                                        &body_font,
                                        &body_cjk_font,
                                        &monospace_font,
                                        &heading_font,
                                        &heading_cjk_font,
                                        0,
                                        font_size,
                                        theme,
                                        editor.styles.selection.selection_diff_added_inline_bg,
                                        editor.styles.selection.selection_diff_removed_inline_bg,
                                        editor.styles.wiki_link.wiki_link_text_color,
                                        editor.styles.wiki_link.wiki_link_text_weight,
                                        None,
                                    );

                                    // Apply table/front-matter text styling
                                    let cell_runs =
                                        if layout.is_header.get(vi).copied().unwrap_or(false) {
                                            cell_runs
                                                .into_iter()
                                                .map(|mut r| {
                                                    r.font.weight = gpui::FontWeight::BOLD;
                                                    r
                                                })
                                                .collect()
                                        } else {
                                            cell_runs
                                        };

                                    let cell_x = table_left + cell_layout.x + layout.cell_padding_x;
                                    let cell_text_y = row_y + layout.cell_padding_y;

                                    let wrap_width = (cell_layout.width
                                        - layout.cell_padding_x * 2.0)
                                        .max(base_char_width);

                                    if let Ok(shaped_lines) = window.text_system().shape_text(
                                        cell_text,
                                        px(font_size),
                                        &cell_runs,
                                        Some(px(wrap_width)),
                                        None,
                                    ) {
                                        if let Some(shaped) = shaped_lines.into_iter().next() {
                                            if let Some(cell_range) = row_ti
                                                .and_then(|t| t.cell_ranges.get(cell_layout.col))
                                            {
                                                let line_start_off = buffer.line_start(source_line);
                                                let cell_buf_start =
                                                    line_start_off + cell_range.start;
                                                let cell_buf_end = line_start_off + cell_range.end;

                                                for sr in &all_sel_ranges {
                                                    if sr.start == sr.end {
                                                        continue;
                                                    }
                                                    let sel_start = sr.start.max(cell_buf_start);
                                                    let sel_end = sr.end.min(cell_buf_end);
                                                    if sel_start < sel_end {
                                                        let buf_start = sel_start - cell_buf_start;
                                                        let buf_end = sel_end - cell_buf_start;
                                                        let disp_start = if buf_start
                                                            >= cell_layout.buffer_to_display.len()
                                                        {
                                                            cell_layout.display_text.len()
                                                        } else {
                                                            helpers::buffer_to_display_offset(
                                                                &cell_layout.buffer_to_display,
                                                                buf_start,
                                                            )
                                                        };
                                                        let disp_end = if buf_end
                                                            >= cell_layout.buffer_to_display.len()
                                                        {
                                                            cell_layout.display_text.len()
                                                        } else {
                                                            helpers::buffer_to_display_offset(
                                                                &cell_layout.buffer_to_display,
                                                                buf_end,
                                                            )
                                                        };
                                                        selection_quads.extend(
                                                            helpers::build_selection_quads(
                                                                &cell_layout.display_text,
                                                                &cell_runs,
                                                                &shaped,
                                                                disp_start.min(
                                                                    cell_layout.display_text.len(),
                                                                ),
                                                                disp_end.min(
                                                                    cell_layout.display_text.len(),
                                                                ),
                                                                cell_x,
                                                                cell_text_y,
                                                                line_height,
                                                                font_size,
                                                                0.0,
                                                                0.0,
                                                                theme.selection,
                                                                letter_spacing,
                                                                window,
                                                            ),
                                                        );
                                                    }
                                                }

                                                // Paint cursors within this cell
                                                if editor.selected_handle.is_none()
                                                    && window.is_window_active()
                                                    && editor.focus_handle.is_focused(window)
                                                    && editor.focused_sidebar_path.is_none()
                                                    && (editor.cursor_opacity > 0.0
                                                        || editor.is_selecting)
                                                {
                                                    {
                                                        for &cur_off in &all_cursor_offsets {
                                                            if cur_off >= cell_buf_start
                                                                && cur_off <= cell_buf_end
                                                            {
                                                                let in_cell =
                                                                    cur_off - cell_buf_start;
                                                                let disp_col = if in_cell
                                                                    >= cell_layout
                                                                        .buffer_to_display
                                                                        .len()
                                                                {
                                                                    cell_layout.display_text.len()
                                                                } else {
                                                                    helpers::buffer_to_display_offset(
                                                                    &cell_layout.buffer_to_display,
                                                                    in_cell,
                                                                )
                                                                };
                                                                let disp_col = disp_col.min(
                                                                    cell_layout.display_text.len(),
                                                                );
                                                                if let Some(pos) = shaped
                                                                    .position_for_index(
                                                                        disp_col,
                                                                        px(line_height),
                                                                    )
                                                                {
                                                                    let cursor_height = line_height
                                                                        * editor
                                                                            .styles
                                                                            .cursor
                                                                            .cursor_height_factor
                                                                            .clamp(0.1, 1.0);
                                                                    let cursor_top = cell_text_y
                                                                        + f32::from(pos.y)
                                                                        + (line_height
                                                                            - cursor_height)
                                                                            / 2.0;
                                                                    let mut cursor_color =
                                                                        theme.cursor;
                                                                    cursor_color.a *=
                                                                        if editor.is_selecting {
                                                                            editor
                                                                                .styles
                                                                                .cursor
                                                                                .cursor_opacity_max
                                                                        } else {
                                                                            editor.cursor_opacity
                                                                        };
                                                                    cursor_quads.push(fill(
                                                                    Bounds::new(
                                                                        point(
                                                                            px(cell_x) + pos.x,
                                                                            px(cursor_top),
                                                                        ),
                                                                        size(
                                                                            px(editor
                                                                                .styles
                                                                                .cursor
                                                                                .cursor_width_px),
                                                                            px(cursor_height),
                                                                        ),
                                                                    ),
                                                                    cursor_color,
                                                                ));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            push_whitespace_dot_quads(
                                                &mut whitespace_dots,
                                                &cell_layout.display_text,
                                                &shaped,
                                                point(px(cell_x), px(cell_text_y)),
                                                line_height,
                                                editor.scaled(
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_whitespace_dot_size_px,
                                                ),
                                                editor.styles.editor.editor_whitespace_dot_color,
                                                0,
                                            );

                                            table_cell_texts.push((
                                                shaped,
                                                point(px(cell_x), px(cell_text_y)),
                                                px(line_height),
                                            ));
                                        }
                                    }

                                    // Register cell click area
                                    let click_bounds = Bounds::new(
                                        point(px(table_left + cell_layout.x), px(row_y)),
                                        size(px(cell_layout.width), px(row_h)),
                                    );
                                    table_click_areas_local.push((
                                        click_bounds,
                                        ti.table_start_line,
                                        vi,
                                        cell_layout.col,
                                    ));
                                }
                            }

                            // Row handle areas (left of each row, hover-only)
                            let handle_w = layout.handle_size;
                            let handle_bounds = Bounds::new(
                                point(px(table_left - handle_w), px(row_y)),
                                size(px(handle_w), px(row_h)),
                            );
                            table_row_handle_areas_local.push((
                                handle_bounds,
                                ti.table_start_line,
                                vi,
                            ));
                        }

                        // Column handle areas (above header, hover-only)
                        {
                            let handle_h = layout.handle_size;
                            let mut hx = 0.0_f32;
                            for ci in 0..layout.col_count {
                                let cell_w = layout
                                    .cells
                                    .first()
                                    .and_then(|row| row.get(ci))
                                    .map(|c| c.width)
                                    .unwrap_or(0.0);
                                let handle_bounds = Bounds::new(
                                    point(px(table_left + hx), px(table_top - handle_h)),
                                    size(px(cell_w), px(handle_h)),
                                );
                                table_col_handle_areas_local.push((
                                    handle_bounds,
                                    ti.table_start_line,
                                    ci,
                                ));
                                hx += cell_w;
                            }
                        }

                        let selected_handle = editor.selected_handle;
                        let highlight_width = table_styles.table_handle_highlight_width_px;
                        let dot_size = table_styles.table_handle_dot_size_px;
                        let dot_gap = table_styles.table_handle_gap_px;

                        // Draw handle grips only for the specific handle being hovered
                        let hovering_handle = editor.hovering_handle;
                        // Column handle grips
                        {
                            let mut hx = 0.0_f32;
                            let handle_h = layout.handle_size;
                            for ci in 0..layout.col_count {
                                let cell_w = layout
                                    .cells
                                    .first()
                                    .and_then(|row| row.get(ci))
                                    .map(|c| c.width)
                                    .unwrap_or(0.0);
                                let is_hovered =
                                    hovering_handle == Some((ti.table_start_line, true, ci));
                                let is_selected =
                                    selected_handle == Some((ti.table_start_line, true, ci));
                                let is_drag_source = matches!(
                                    &editor.table_drag,
                                    Some(TableDragState::DraggingColumn {
                                        table_start_line,
                                        source_col,
                                        ..
                                    }) if *table_start_line == ti.table_start_line && *source_col == ci
                                );
                                if is_hovered || is_selected || is_drag_source {
                                    table_borders.push(quad(
                                        Bounds::new(
                                            point(px(table_left + hx), px(table_top)),
                                            size(px(cell_w), px(layout.total_height)),
                                        ),
                                        Corners::all(px(4.0)),
                                        gpui::transparent_black(),
                                        px(highlight_width),
                                        theme.cursor,
                                        BorderStyle::default(),
                                    ));
                                }
                                if is_hovered {
                                    let grip_w = dot_size * 3.0 + dot_gap * 2.0;
                                    let grip_h = dot_size * 2.0 + dot_gap;
                                    let grip_x = table_left + hx + cell_w / 2.0 - grip_w / 2.0;
                                    let grip_y = table_top - handle_h / 2.0 - grip_h / 2.0;
                                    let handle_color = table_styles.table_handle_color;
                                    for dr in 0..2 {
                                        for dc in 0..3 {
                                            let dx = grip_x + dc as f32 * (dot_size + dot_gap);
                                            let dy = grip_y + dr as f32 * (dot_size + dot_gap);
                                            table_bgs.push(fill(
                                                Bounds::new(
                                                    point(px(dx), px(dy)),
                                                    size(px(dot_size), px(dot_size)),
                                                ),
                                                handle_color,
                                            ));
                                        }
                                    }
                                }
                                hx += cell_w;
                            }
                        }

                        // Row handle grips
                        for vi in 0..layout.visual_row_count {
                            let row_y = table_top + layout.row_heights.iter().take(vi).sum::<f32>();
                            let row_h = layout.row_heights.get(vi).copied().unwrap_or(line_height);
                            let is_hovered =
                                hovering_handle == Some((ti.table_start_line, false, vi));
                            let is_selected =
                                selected_handle == Some((ti.table_start_line, false, vi));
                            let is_drag_source = matches!(
                                &editor.table_drag,
                                Some(TableDragState::DraggingRow {
                                    table_start_line,
                                    source_row,
                                    ..
                                }) if *table_start_line == ti.table_start_line && *source_row == vi
                            );
                            if is_hovered || is_selected || is_drag_source {
                                table_borders.push(quad(
                                    Bounds::new(
                                        point(px(table_left), px(row_y)),
                                        size(px(table_width), px(row_h)),
                                    ),
                                    Corners::all(px(4.0)),
                                    gpui::transparent_black(),
                                    px(highlight_width),
                                    theme.cursor,
                                    BorderStyle::default(),
                                ));
                            }
                            if is_hovered {
                                let handle_w = layout.handle_size;
                                let grip_w = dot_size * 2.0 + dot_gap;
                                let grip_h = dot_size * 3.0 + dot_gap * 2.0;
                                let grip_x = table_left - handle_w / 2.0 - grip_w / 2.0;
                                let grip_y = row_y + row_h / 2.0 - grip_h / 2.0;
                                let handle_color = table_styles.table_handle_color;
                                for dr in 0..3 {
                                    for dc in 0..2 {
                                        let dx = grip_x + dc as f32 * (dot_size + dot_gap);
                                        let dy = grip_y + dr as f32 * (dot_size + dot_gap);
                                        table_bgs.push(fill(
                                            Bounds::new(
                                                point(px(dx), px(dy)),
                                                size(px(dot_size), px(dot_size)),
                                            ),
                                            handle_color,
                                        ));
                                    }
                                }
                            }
                        }

                        if let Some(drag) = &editor.table_drag {
                            let indicator_color = theme.cursor;
                            let indicator_width = table_styles.table_drag_indicator_width_px;
                            match drag {
                                TableDragState::DraggingColumn {
                                    source_col,
                                    table_start_line,
                                    current_x,
                                    ..
                                } if *table_start_line == ti.table_start_line => {
                                    let mut boundaries = Vec::with_capacity(layout.col_count + 1);
                                    boundaries.push(table_left);
                                    if let Some(first_row) = layout.cells.first() {
                                        for cell in first_row {
                                            boundaries.push(table_left + cell.x + cell.width);
                                        }
                                    }
                                    if let Some(&indicator_x) = boundaries.iter().min_by(|a, b| {
                                        (*a - *current_x)
                                            .abs()
                                            .partial_cmp(&(*b - *current_x).abs())
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    }) {
                                        let insert_at = boundaries
                                            .iter()
                                            .position(|boundary| {
                                                (*boundary - indicator_x).abs() < 0.5
                                            })
                                            .unwrap_or(0);
                                        if insert_at != *source_col && insert_at != *source_col + 1
                                        {
                                            table_borders.push(fill(
                                                Bounds::new(
                                                    point(
                                                        px(indicator_x - indicator_width / 2.0),
                                                        px(table_top),
                                                    ),
                                                    size(
                                                        px(indicator_width),
                                                        px(layout.total_height),
                                                    ),
                                                ),
                                                indicator_color,
                                            ));
                                        }
                                    }
                                }
                                TableDragState::DraggingRow {
                                    source_row,
                                    table_start_line,
                                    current_y,
                                    ..
                                } if *table_start_line == ti.table_start_line => {
                                    let mut boundaries =
                                        Vec::with_capacity(layout.visual_row_count + 1);
                                    boundaries.push(table_top);
                                    let mut y_cursor = table_top;
                                    for row_height in &layout.row_heights {
                                        y_cursor += *row_height;
                                        boundaries.push(y_cursor);
                                    }
                                    if let Some(&indicator_y) = boundaries.iter().min_by(|a, b| {
                                        (*a - *current_y)
                                            .abs()
                                            .partial_cmp(&(*b - *current_y).abs())
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    }) {
                                        let insert_at = boundaries
                                            .iter()
                                            .position(|boundary| {
                                                (*boundary - indicator_y).abs() < 0.5
                                            })
                                            .unwrap_or(0);
                                        if insert_at != *source_row && insert_at != *source_row + 1
                                        {
                                            table_borders.push(fill(
                                                Bounds::new(
                                                    point(
                                                        px(table_left),
                                                        px(indicator_y - indicator_width / 2.0),
                                                    ),
                                                    size(px(table_width), px(indicator_width)),
                                                ),
                                                indicator_color,
                                            ));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // HR dashed line
            if *block_type == BlockType::HorizontalRule && !is_cursor_line {
                let hr_y = text_line_top + text_line_height / 2.0;
                hr_lines.push((hr_y, editor_left + editor.padding_left(), text_width));
            }

            // Image preview
            if !editor.folded_image_previews.contains(&line_idx) {
                if let Some(ref path) = decoration.image_path {
                    if let Some(Some(img_data)) = image_cache.get(path) {
                        let img_size = img_data.size(0);
                        let img_w = img_size.width.0 as f32;
                        let img_h = img_size.height.0 as f32;
                        if img_w > 0.0 && img_h > 0.0 {
                            let (display_w, display_h) = helpers::compute_image_preview_size(
                                img_w,
                                img_h,
                                text_width,
                                window_scale_factor,
                            );
                            let img_top = text_line_top + text_line_height + image_gap_top;
                            let img_left = editor_left
                                + editor.padding_left()
                                + ((text_width - display_w) / 2.0).max(0.0);
                            let img_bounds = Bounds::new(
                                point(px(img_left), px(img_top)),
                                size(px(display_w), px(display_h)),
                            );
                            image_previews.push((
                                img_bounds,
                                img_data.clone(),
                                image_corner_radius,
                            ));
                            image_hit_areas.push((img_bounds, line_idx));
                            let image_zoom_visible = image_active
                                || editor.hovering_image_preview_line == Some(line_idx);
                            if image_zoom_visible {
                                image_zoom_button_targets.push((img_bounds, line_idx));
                            }
                            if image_zoom_visible {
                                image_borders.push(quad(
                                    img_bounds,
                                    Corners::all(image_corner_radius),
                                    gpui::transparent_black(),
                                    image_border_width,
                                    image_styles.image_active_border_color,
                                    BorderStyle::default(),
                                ));
                            }
                        }
                    }
                }
            }

            if let Some(preview) = collapsed_block_preview {
                if let Some(Some(img_data)) = image_cache.get(&preview.cache_key) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0 && img_h > 0.0 {
                        let overlay_open = editor
                            .preview_overlay_state()
                            .is_some_and(|overlay| overlay.line_idx == line_idx);
                        let (display_w, display_h) =
                            helpers::compute_generated_preview_size_for_kind(
                                img_w,
                                img_h,
                                text_width,
                                preview.kind,
                            );
                        let block_left = editor_left + editor.padding_left();
                        let block_top = line_top;
                        let block_height = text_line_height
                            + display_h
                            + image_gap_top
                            + generated_preview_gap_bottom;
                        let block_bounds = Bounds::new(
                            point(px(block_left), px(block_top)),
                            size(px(text_width.max(1.0)), px(block_height)),
                        );
                        let img_top = line_top + text_line_height + image_gap_top;
                        let img_left = block_left + ((text_width - display_w) / 2.0).max(0.0);
                        let img_bounds = Bounds::new(
                            point(px(img_left), px(img_top)),
                            size(px(display_w), px(display_h)),
                        );
                        image_previews.push((img_bounds, img_data.clone(), px(10.0)));
                        generated_preview_hit_areas.push((block_bounds, line_idx));
                        generated_preview_image_areas.push((img_bounds, line_idx));
                        if editor.hovering_generated_preview_line == Some(line_idx)
                            || editor.hovering_generated_preview_zoom_line == Some(line_idx)
                            || overlay_open
                        {
                            image_borders.push(quad(
                                block_bounds,
                                Corners::all(px(0.0)),
                                gpui::transparent_black(),
                                px(2.0),
                                theme.cursor,
                                BorderStyle::default(),
                            ));
                        }
                    }
                }
            }

            // Gutter decorations
            let gutter_center_y = text_line_top + line_height / 2.0;
            let fold_range = if hidden_lines.contains(&line_idx) {
                None
            } else {
                editor.fold_range_for_line(line_idx)
            };
            let fold_icon_visible = if fold_range.is_some() {
                editor.folded_sections.contains(&line_idx)
                    || editor.hovering_gutter_fold_area
                    || editor.hovering_fold_line == Some(line_idx)
            } else {
                false
            };
            if editor.active_tab_is_non_markdown_text() {
                let gutter_styles = &editor.styles.gutter;
                push_gutter_line_number_label(
                    &mut gutter_labels,
                    window,
                    line_idx,
                    is_active_line_number,
                    &monospace_font,
                    editor.scaled(gutter_styles.gutter_line_number_font_size_px),
                    gutter_right_x,
                    text_line_top,
                    line_height,
                    editor.scaled(gutter_styles.gutter_line_number_vertical_offset_px),
                    editor.scaled(gutter_styles.gutter_line_number_right_gap_px),
                    gutter_styles.gutter_line_number_text_color,
                    gutter_styles.gutter_line_number_active_text_color,
                );
            }

            match block_type {
                BlockType::Heading(level) if is_cursor_line && !fold_icon_visible => {
                    let gutter_styles = &editor.styles.gutter;
                    let heading_offset = editor.scaled(gutter_styles.gutter_heading_icon_offset_px);
                    let icon_size = match *level {
                        1 => editor.scaled(gutter_styles.gutter_heading_h1_icon_size_px),
                        2 => editor.scaled(gutter_styles.gutter_heading_h2_icon_size_px),
                        3 => editor.scaled(gutter_styles.gutter_heading_h3_icon_size_px),
                        _ => editor.scaled(gutter_styles.gutter_heading_h4_icon_size_px),
                    };
                    let icon_path = match *level {
                        1 => HEADING_1_SVG_PATH,
                        2 => HEADING_2_SVG_PATH,
                        3 => HEADING_3_SVG_PATH,
                        4 => HEADING_4_SVG_PATH,
                        5 => HEADING_5_SVG_PATH,
                        _ => HEADING_6_SVG_PATH,
                    };
                    let icon_x = if editor.active_tab_is_non_markdown_text() {
                        editor.gutter_icon_x(editor_left, icon_size)
                    } else {
                        editor_left + editor.padding_left() - icon_size - heading_offset
                    };
                    let icon_y = gutter_center_y + 0.0 - icon_size / 2.0;
                    gutter_svgs.push((
                        Bounds::new(
                            point(px(icon_x), px(icon_y)),
                            size(px(icon_size), px(icon_size)),
                        ),
                        icon_path,
                        gutter_styles.gutter_heading_color,
                    ));
                }
                BlockType::Blockquote => {
                    let gutter_styles = &editor.styles.gutter;
                    let icon_size = editor.scaled(gutter_styles.gutter_blockquote_icon_size_px);
                    let icon_x = if editor.active_tab_is_non_markdown_text() {
                        editor.gutter_icon_x(editor_left, icon_size)
                    } else {
                        editor_left + editor.padding_left()
                            - icon_size
                            - editor.scaled(gutter_styles.gutter_blockquote_icon_offset_px)
                    };
                    let icon_y = gutter_center_y - 1.0 - icon_size / 2.0;
                    gutter_svgs.push((
                        Bounds::new(
                            point(px(icon_x), px(icon_y)),
                            size(px(icon_size), px(icon_size)),
                        ),
                        COMMENT_SVG_PATH,
                        gutter_styles.gutter_blockquote_icon_color,
                    ));
                }
                _ => {}
            }

            // Checkbox painting
            if prefix_collapsed {
                if let BlockType::TaskList(checked) = block_type {
                    let checkbox_styles = &editor.styles.checkbox;
                    let cb_size = font_size * editor.styles.checkbox.checkbox_size_factor;
                    let prefix_bytes = helpers::compute_display_prefix_bytes(
                        &display_text,
                        block_type,
                        prefix_collapsed,
                        collapsed_task_prefix_display_columns,
                    )
                    .min(display_text.len());
                    let text_start_x = wrapped
                        .position_for_index(prefix_bytes, px(line_height))
                        .map(|pos| f32::from(pos.x))
                        .unwrap_or(0.0);
                    let cb_x = content_x + text_start_x - checkbox_text_gap_px - cb_size;
                    let cb_y = text_line_top + (line_height - cb_size) / 2.0;
                    let cb_bounds =
                        Bounds::new(point(px(cb_x), px(cb_y)), size(px(cb_size), px(cb_size)));
                    let corner_r = px(editor.scaled(editor.styles.checkbox.checkbox_radius_px));
                    let checkbox_opacity = if editor.hovering_checkbox_line == Some(line_idx) {
                        checkbox_styles.checkbox_hover_opacity
                    } else {
                        checkbox_styles.checkbox_default_opacity
                    };
                    if *checked {
                        let mut checkbox_bg = theme.checkbox_checked;
                        checkbox_bg.a *= checkbox_opacity;
                        gutter_quads.push(quad(
                            cb_bounds,
                            Corners::all(corner_r),
                            checkbox_bg,
                            px(0.0),
                            gpui::transparent_black(),
                            BorderStyle::default(),
                        ));
                        let icon_size = cb_size * editor.styles.checkbox.checkbox_check_icon_scale;
                        let icon_offset = (cb_size - icon_size) / 2.0;
                        let mut icon_color = gpui::white();
                        icon_color.a *= checkbox_opacity;
                        gutter_svgs.push((
                            Bounds::new(
                                point(px(cb_x + icon_offset), px(cb_y + icon_offset)),
                                size(px(icon_size), px(icon_size)),
                            ),
                            CHECK_SVG_PATH,
                            icon_color,
                        ));
                    } else {
                        let mut border_color = theme.border_tertiary;
                        border_color.a *= checkbox_opacity;
                        gutter_quads.push(quad(
                            cb_bounds,
                            Corners::all(corner_r),
                            gpui::transparent_black(),
                            px(1.5),
                            border_color,
                            BorderStyle::default(),
                        ));
                    }
                    let hit_padding = editor.scaled(2.0);
                    let hit_bounds = Bounds::new(
                        point(px(cb_x - hit_padding), px(line_top)),
                        size(px(cb_size + hit_padding * 2.0), px(total_line_height)),
                    );
                    checkbox_hit_areas.push((hit_bounds, line_idx));
                }
            }

            // Gutter fold icon
            if !hidden_lines.contains(&line_idx) {
                let gs = &editor.styles.gutter;
                let fi_size = editor.scaled(gs.gutter_fold_icon_size_px);
                let fi_offset = editor.scaled(gs.gutter_fold_icon_offset_px);
                let fi_hit_padding = editor.scaled(gs.gutter_fold_hit_padding_px);
                let fi_x = if editor.active_tab_is_non_markdown_text() {
                    editor.gutter_icon_x(editor_left, fi_size)
                } else {
                    editor_left + editor.padding_left() - fi_size - fi_offset
                };

                if fold_range.is_some() {
                    let is_folded = decoration
                        .block_preview
                        .as_ref()
                        .filter(|preview| preview.is_anchor)
                        .is_some_and(|preview| {
                            editor.folded_sections.contains(&preview.start_line)
                        })
                        || editor.folded_sections.contains(&line_idx);
                    let is_hovered = editor.hovering_gutter_fold_area
                        || editor.hovering_fold_line == Some(line_idx);
                    let fi_y = text_line_top + (line_height - fi_size) / 2.0;
                    let reveal = editor.gutter_fold_reveal.max(0.0);
                    let fi_alpha = if is_folded {
                        gs.gutter_fold_folded_opacity
                    } else if is_hovered {
                        gs.gutter_fold_hover_opacity * reveal
                    } else {
                        gs.gutter_fold_default_opacity * reveal
                    };
                    if fi_alpha > 0.0 {
                        let mut fi_color = if is_folded {
                            gs.gutter_fold_folded_color
                        } else {
                            gs.gutter_fold_hover_color
                        };
                        fi_color.a = fi_alpha;
                        gutter_fold_icons.push((
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size))),
                            if is_folded {
                                FOLD_SVG_PATH
                            } else {
                                FOLD_EXPANDED_SVG_PATH
                            },
                            fi_color,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        if editor.active_tab_is_non_markdown_text() {
                            Bounds::new(
                                point(px(gutter_icon_column_left - fi_hit_padding), px(line_top)),
                                size(
                                    px(gutter_icon_column_width + fi_hit_padding * 2.0),
                                    px(total_line_height),
                                ),
                            )
                        } else {
                            Bounds::new(
                                point(px(fi_x - fi_hit_padding), px(line_top)),
                                size(
                                    px(fi_size + fi_hit_padding * 2.0 + fi_offset),
                                    px(total_line_height),
                                ),
                            )
                        },
                        line_idx,
                        false,
                    ));
                }

                // Image preview fold icon
                if decoration
                    .image_path
                    .as_ref()
                    .is_some_and(|p| image_cache.get(p).is_some_and(|v| v.is_some()))
                {
                    let is_img_folded = editor.folded_image_previews.contains(&line_idx);
                    let is_hovered = editor.hovering_gutter_fold_area
                        || editor.hovering_fold_line == Some(line_idx);
                    let fi_y = text_line_top + (line_height - fi_size) / 2.0;
                    let gs = &editor.styles.gutter;
                    let reveal = editor.gutter_fold_reveal.max(0.0);
                    let fi_alpha = if is_img_folded {
                        gs.gutter_fold_folded_opacity
                    } else if is_hovered {
                        gs.gutter_fold_hover_opacity * reveal
                    } else {
                        gs.gutter_fold_default_opacity * reveal
                    };
                    if fi_alpha > 0.0 {
                        let mut fi_color = if is_img_folded {
                            gs.gutter_fold_folded_color
                        } else {
                            gs.gutter_fold_hover_color
                        };
                        fi_color.a = fi_alpha;
                        gutter_fold_icons.push((
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size))),
                            if is_img_folded {
                                FOLD_SVG_PATH
                            } else {
                                FOLD_EXPANDED_SVG_PATH
                            },
                            fi_color,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        if editor.active_tab_is_non_markdown_text() {
                            Bounds::new(
                                point(px(gutter_icon_column_left - fi_hit_padding), px(line_top)),
                                size(
                                    px(gutter_icon_column_width + fi_hit_padding * 2.0),
                                    px(total_line_height),
                                ),
                            )
                        } else {
                            Bounds::new(
                                point(px(fi_x - fi_hit_padding), px(line_top)),
                                size(
                                    px(fi_size + fi_hit_padding * 2.0 + fi_offset),
                                    px(total_line_height),
                                ),
                            )
                        },
                        line_idx,
                        true,
                    ));
                }
            }

            // Image URL-edit icon
            if image_active {
                let ie_size = editor.scaled(13.0);
                let syntax_end_x = wrapped
                    .position_for_index(display_text.len(), px(line_height))
                    .map(|pos| f32::from(pos.x))
                    .unwrap_or(0.0);
                let ie_x = content_x + syntax_end_x + editor.scaled(10.0);
                let ie_y = text_line_top + (line_height - ie_size) / 2.0;
                let ie_bounds =
                    Bounds::new(point(px(ie_x), px(ie_y)), size(px(ie_size), px(ie_size)));
                let is_hovered = editor.hovering_image_edit_line == Some(line_idx);
                let alpha = if is_hovered { 0.55 } else { 0.7 };
                image_edit_icons.push((ie_bounds, line_idx, alpha));
                image_edit_hit_areas_local.push((ie_bounds, line_idx));
            }

            // Line highlight
            let skip_block_line_highlight = decoration
                .table_info
                .as_ref()
                .is_some_and(|ti| !ti.is_front_matter && line_idx == ti.table_start_line);
            if let Some(diff_bg) = diff_line_background(editor, line_idx) {
                diff_line_bgs.push(fill(
                    Bounds::new(
                        point(px(origin_x), px(line_top)),
                        size(px(viewport_width), px(total_line_height)),
                    ),
                    diff_bg,
                ));
            }
            let selection_styles = &editor.styles.selection;
            push_diff_inline_highlight_quads(
                &mut diff_inline_quads,
                &decoration.diff_removed_spans,
                &display_text,
                line_content.len(),
                &buffer_to_display,
                &runs,
                wrapped,
                content_x,
                text_line_top,
                line_height,
                font_size,
                indent_px,
                selection_styles.selection_diff_removed_inline_bg,
                selection_styles.selection_diff_removed_inline_border_color,
                editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                letter_spacing,
                window,
            );
            push_diff_inline_highlight_quads(
                &mut diff_inline_quads,
                &decoration.diff_added_spans,
                &display_text,
                line_content.len(),
                &buffer_to_display,
                &runs,
                wrapped,
                content_x,
                text_line_top,
                line_height,
                font_size,
                indent_px,
                selection_styles.selection_diff_added_inline_bg,
                selection_styles.selection_diff_added_inline_border_color,
                editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                letter_spacing,
                window,
            );
            if !decoration.highlight_spans.is_empty() {
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.highlight_spans,
                    &display_text,
                    line_content.len(),
                    &buffer_to_display,
                    &runs,
                    wrapped,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    indent_px,
                    editor.styles.editor.editor_inline_highlight_bg,
                    gpui::transparent_black(),
                    0.0,
                    letter_spacing,
                    window,
                );
            }
            if !selection_match_ranges.is_empty() {
                let mut line_spans: Vec<std::ops::Range<usize>> = Vec::new();
                for range in &selection_match_ranges {
                    if range.end > line_start_offset && range.start < line_end_offset {
                        let start = range.start.max(line_start_offset) - line_start_offset;
                        let end = range.end.min(line_end_offset) - line_start_offset;
                        if start < end {
                            line_spans.push(start..end);
                        }
                    }
                }
                if !line_spans.is_empty() {
                    push_diff_inline_highlight_quads(
                        &mut diff_inline_quads,
                        &line_spans,
                        &display_text,
                        line_content.len(),
                        &buffer_to_display,
                        &runs,
                        wrapped,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        gpui::transparent_black(),
                        editor.styles.editor.editor_selection_match_border,
                        editor.scaled(editor.styles.editor.editor_match_border_width_px),
                        letter_spacing,
                        window,
                    );
                }
            }
            if !bracket_match_ranges.is_empty() {
                let mut line_spans: Vec<std::ops::Range<usize>> = Vec::new();
                for range in &bracket_match_ranges {
                    if range.end > line_start_offset && range.start < line_end_offset {
                        let start = range.start.max(line_start_offset) - line_start_offset;
                        let end = range.end.min(line_end_offset) - line_start_offset;
                        if start < end {
                            line_spans.push(start..end);
                        }
                    }
                }
                if !line_spans.is_empty() {
                    push_diff_inline_highlight_quads(
                        &mut diff_inline_quads,
                        &line_spans,
                        &display_text,
                        line_content.len(),
                        &buffer_to_display,
                        &runs,
                        wrapped,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        gpui::transparent_black(),
                        editor.styles.editor.editor_bracket_match_border,
                        editor.scaled(editor.styles.editor.editor_match_border_width_px),
                        letter_spacing,
                        window,
                    );
                }
            }
            if editor.selected_handle.is_none()
                && is_cursor_line
                && !any_selection
                && editor.extra_cursors.is_empty()
                && !skip_block_line_highlight
            {
                line_highlight = Some(fill(
                    Bounds::new(
                        point(px(origin_x), px(line_top)),
                        size(px(viewport_width), px(total_line_height)),
                    ),
                    theme.line_highlight,
                ));
            }

            // Selection quads — iterate over all cursors (primary + extra)
            for sr in &all_sel_ranges {
                if sr.start == sr.end {
                    continue;
                }
                let sel_start_on_line = sr.start.max(line_start_offset);
                let sel_end_on_line = sr.end.min(line_end_offset);

                if sel_start_on_line < sel_end_on_line {
                    let buf_start_col = sel_start_on_line - line_start_offset;
                    let buf_end_col = sel_end_on_line - line_start_offset;
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                    let disp_end =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                    selection_quads.extend(helpers::build_selection_quads(
                        &display_text,
                        &runs,
                        wrapped,
                        disp_start,
                        disp_end,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        selection_padding_left_px,
                        theme.selection,
                        letter_spacing,
                        window,
                    ));
                }
                if sr.end > line_end_offset && sr.start <= line_end_offset {
                    let disp_len = display_text.len();
                    let pos_end = wrapped
                        .position_for_index(disp_len, px(line_height))
                        .unwrap_or_default();
                    let indent_end = helpers::indent_for_y(indent_px, pos_end.y, line_height);
                    selection_quads.push(fill(
                        Bounds::from_corners(
                            point(
                                px(content_x + indent_end) + pos_end.x,
                                px(text_line_top) + pos_end.y,
                            ),
                            point(
                                px(content_x + indent_end)
                                    + pos_end.x
                                    + px(editor.base_font_size() * 0.4),
                                px(text_line_top) + pos_end.y + px(line_height),
                            ),
                        ),
                        theme.selection,
                    ));
                }
            }

            // Search match quads
            if !editor.search_matches.is_empty() {
                let match_bg = editor.styles.search.search_match_bg;
                let active_bg = editor.styles.search.search_match_active_bg;
                for (match_idx, sr) in editor.search_matches.iter().enumerate() {
                    let is_active = editor.search_current_index == Some(match_idx);
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);
                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                        let disp_end =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                        search_match_quads.extend(helpers::build_selection_quads(
                            &display_text,
                            &runs,
                            wrapped,
                            disp_start,
                            disp_end,
                            content_x,
                            line_top,
                            line_height,
                            font_size,
                            indent_px,
                            0.0,
                            if is_active { active_bg } else { match_bg },
                            letter_spacing,
                            window,
                        ));
                    }
                }
            }

            if editor.current_file_path == editor.workspace_search_highlight_path {
                let match_bg = editor.styles.search.search_match_bg;
                let active_bg = editor.styles.search.search_match_active_bg;
                for (match_idx, sr) in editor.workspace_search_highlight_ranges.iter().enumerate() {
                    let is_active =
                        editor.workspace_search_highlight_current_index == Some(match_idx);
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);
                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                        let disp_end =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                        search_match_quads.extend(helpers::build_selection_quads(
                            &display_text,
                            &runs,
                            wrapped,
                            disp_start,
                            disp_end,
                            content_x,
                            line_top,
                            line_height,
                            font_size,
                            indent_px,
                            0.0,
                            if is_active { active_bg } else { match_bg },
                            letter_spacing,
                            window,
                        ));
                    }
                }
            }
            if editor.current_file_path == editor.outline_flash_highlight_path {
                let match_bg = editor.styles.search.search_match_bg;
                let active_bg = editor.styles.search.search_match_active_bg;
                for (match_idx, sr) in editor.outline_flash_highlight_ranges.iter().enumerate() {
                    let is_active = editor.outline_flash_highlight_current_index == Some(match_idx);
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);
                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                        let disp_end =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                        search_match_quads.extend(helpers::build_selection_quads(
                            &display_text,
                            &runs,
                            wrapped,
                            disp_start,
                            disp_end,
                            content_x,
                            line_top,
                            line_height,
                            font_size,
                            indent_px,
                            0.0,
                            if is_active { active_bg } else { match_bg },
                            letter_spacing,
                            window,
                        ));
                    }
                }
            }

            // Cursor bars — paint for all cursors (primary + extra)
            // Skip for table lines — table cursor is rendered within the table block
            if editor.selected_handle.is_none()
                && (*block_type != BlockType::Table || is_front_matter_line)
                && window.is_window_active()
                && editor.focus_handle.is_focused(window)
                && editor.focused_sidebar_path.is_none()
                && (editor.cursor_opacity > 0.0 || editor.is_selecting)
            {
                for &cur_offset in &all_cursor_offsets {
                    if cur_offset < line_start_offset || cur_offset > line_end_offset {
                        continue;
                    }
                    // For line_end_offset, only match if this is the actual last offset
                    // (handles cursor at end of line)
                    if cur_offset == line_end_offset && cur_offset != line_start_offset {
                        // cursor is at line end — show it here
                    }
                    let buf_cursor_col = cur_offset.saturating_sub(line_start_offset);
                    let buf_cursor_col = buf_cursor_col.min(line_content.len());
                    let disp_cursor_col = if buf_cursor_col >= line_content.len() {
                        display_text.len()
                    } else {
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_cursor_col)
                    };

                    let cursor_pos = wrapped
                        .position_for_index(disp_cursor_col, px(line_height))
                        .unwrap_or_default();
                    let cursor_pos = match editor.wrap_boundary_affinity {
                        Some(WrapBoundaryAffinity {
                            offset: affinity_offset,
                            kind: WrapBoundaryAffinityKind::Upstream,
                        }) if affinity_offset == cur_offset => upstream_wrap_boundary_position(
                            wrapped,
                            &display_text,
                            disp_cursor_col,
                            &wrap_boundary_upstream_xs,
                            line_height,
                        )
                        .unwrap_or(cursor_pos),
                        Some(WrapBoundaryAffinity {
                            offset: affinity_offset,
                            kind: WrapBoundaryAffinityKind::Downstream,
                        }) if affinity_offset == cur_offset => {
                            downstream_wrap_boundary_position(wrapped, disp_cursor_col, line_height)
                                .unwrap_or(cursor_pos)
                        }
                        _ => cursor_pos,
                    };

                    let cursor_indent = helpers::indent_for_y(indent_px, cursor_pos.y, line_height);
                    let cursor_height =
                        line_height * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
                    let cursor_top = text_line_top
                        + f32::from(cursor_pos.y)
                        + (line_height - cursor_height) / 2.0;
                    let mut cursor_color = theme.cursor;
                    cursor_color.a *= if editor.is_selecting {
                        editor.styles.cursor.cursor_opacity_max
                    } else {
                        editor.cursor_opacity
                    };
                    cursor_quads.push(fill(
                        Bounds::new(
                            point(px(content_x + cursor_indent) + cursor_pos.x, px(cursor_top)),
                            size(px(editor.styles.cursor.cursor_width_px), px(cursor_height)),
                        ),
                        cursor_color,
                    ));
                }
            }

            // IME marked text underline
            if let Some(ref marked) = editor.marked_range {
                let mark_start_on_line = marked.start.max(line_start_offset);
                let mark_end_on_line = marked.end.min(line_end_offset);
                if mark_start_on_line < mark_end_on_line {
                    let buf_start = mark_start_on_line - line_start_offset;
                    let buf_end = mark_end_on_line - line_start_offset;
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_start);
                    let disp_end = helpers::buffer_to_display_offset(&buffer_to_display, buf_end);
                    let pos_start = wrapped
                        .position_for_index(disp_start, px(line_height))
                        .unwrap_or_default();
                    let pos_end = wrapped
                        .position_for_index(disp_end, px(line_height))
                        .unwrap_or_default();
                    let indent_start = helpers::indent_for_y(indent_px, pos_start.y, line_height);
                    let underline_y = text_line_top + f32::from(pos_start.y) + line_height - 2.0;
                    marked_underlines.push(fill(
                        Bounds::from_corners(
                            point(px(content_x + indent_start) + pos_start.x, px(underline_y)),
                            point(
                                px(content_x + indent_start) + pos_end.x,
                                px(underline_y + 1.0),
                            ),
                        ),
                        theme.text_primary,
                    ));
                }
            }

            // Link underlines when cmd is held
            if editor.command_held {
                let hovered_position = editor.hovered_editor_position;
                for span in &decoration.inline_spans {
                    if span.kind != crate::markdown::InlineKind::Link {
                        continue;
                    }
                    let buf_start = span.range.start.min(line_content.len());
                    let buf_end = span.range.end.min(line_content.len());
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_start);
                    let disp_end = helpers::buffer_to_display_offset(&buffer_to_display, buf_end);
                    if let (Some(pos_start), Some(pos_end)) = (
                        wrapped.position_for_index(disp_start, px(line_height)),
                        wrapped.position_for_index(disp_end, px(line_height)),
                    ) {
                        let indent_start =
                            helpers::indent_for_y(indent_px, pos_start.y, line_height);
                        let link_bounds = Bounds::from_corners(
                            point(
                                px(content_x + indent_start) + pos_start.x,
                                px(text_line_top + f32::from(pos_start.y)),
                            ),
                            point(
                                px(content_x + indent_start) + pos_end.x,
                                px(text_line_top + f32::from(pos_start.y) + line_height),
                            ),
                        );
                        if hovered_position.is_some_and(|position| link_bounds.contains(&position))
                        {
                            let ul_y = text_line_top + f32::from(pos_start.y) + line_height - 2.0;
                            link_underlines.push(fill(
                                Bounds::from_corners(
                                    point(px(content_x + indent_start) + pos_start.x, px(ul_y)),
                                    point(px(content_x + indent_start) + pos_end.x, px(ul_y + 1.5)),
                                ),
                                theme.link,
                            ));
                        }
                        link_hitboxes
                            .push(window.insert_hitbox(link_bounds, HitboxBehavior::Normal));
                    }
                }

                for wiki_link in &decoration.wiki_links {
                    if !editor.markdown_target_is_navigable(&wiki_link.target_path) {
                        continue;
                    }
                    let is_collapsed =
                        !helpers::active_ranges_intersect(&active_ranges, &wiki_link.full_range);
                    let visible_range = if is_collapsed {
                        wiki_link.collapsed_range.clone()
                    } else {
                        wiki_link.full_range.clone()
                    };
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.start);
                    let disp_end =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.end);
                    if let (Some(pos_start), Some(pos_end)) = (
                        wrapped.position_for_index(disp_start, px(line_height)),
                        wrapped.position_for_index(disp_end, px(line_height)),
                    ) {
                        let indent_start =
                            helpers::indent_for_y(indent_px, pos_start.y, line_height);
                        let link_bounds = Bounds::from_corners(
                            point(
                                px(content_x + indent_start) + pos_start.x,
                                px(text_line_top + f32::from(pos_start.y)),
                            ),
                            point(
                                px(content_x + indent_start) + pos_end.x,
                                px(text_line_top + f32::from(pos_start.y) + line_height),
                            ),
                        );
                        if hovered_position.is_some_and(|position| link_bounds.contains(&position))
                        {
                            let ul_y = text_line_top + f32::from(pos_start.y) + line_height - 2.0;
                            link_underlines.push(fill(
                                Bounds::from_corners(
                                    point(px(content_x + indent_start) + pos_start.x, px(ul_y)),
                                    point(px(content_x + indent_start) + pos_end.x, px(ul_y + 1.5)),
                                ),
                                theme.link,
                            ));
                        }
                        link_hitboxes
                            .push(window.insert_hitbox(link_bounds, HitboxBehavior::Normal));
                    }
                }

                for footnote_reference in &decoration.footnote_references {
                    if editor
                        .find_footnote_definition_line(&footnote_reference.label)
                        .is_none()
                    {
                        continue;
                    }
                    let is_collapsed = !helpers::active_ranges_intersect(
                        &active_ranges,
                        &footnote_reference.full_range,
                    );
                    let visible_range = if is_collapsed {
                        footnote_reference.collapsed_range.clone()
                    } else {
                        footnote_reference.full_range.clone()
                    };
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.start);
                    let disp_end =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.end);
                    if let (Some(pos_start), Some(pos_end)) = (
                        wrapped.position_for_index(disp_start, px(line_height)),
                        wrapped.position_for_index(disp_end, px(line_height)),
                    ) {
                        let indent_start =
                            helpers::indent_for_y(indent_px, pos_start.y, line_height);
                        let link_bounds = Bounds::from_corners(
                            point(
                                px(content_x + indent_start) + pos_start.x,
                                px(text_line_top + f32::from(pos_start.y)),
                            ),
                            point(
                                px(content_x + indent_start) + pos_end.x,
                                px(text_line_top + f32::from(pos_start.y) + line_height),
                            ),
                        );
                        if hovered_position.is_some_and(|position| link_bounds.contains(&position))
                        {
                            let ul_y = text_line_top + f32::from(pos_start.y) + line_height - 2.0;
                            link_underlines.push(fill(
                                Bounds::from_corners(
                                    point(px(content_x + indent_start) + pos_start.x, px(ul_y)),
                                    point(px(content_x + indent_start) + pos_end.x, px(ul_y + 1.5)),
                                ),
                                theme.link,
                            ));
                        }
                        link_hitboxes
                            .push(window.insert_hitbox(link_bounds, HitboxBehavior::Normal));
                    }
                }
            }

            for superscript_range in &decoration.superscript_spans {
                if !helpers::inline_range_is_collapsed(
                    &decoration,
                    superscript_range,
                    &active_ranges,
                ) {
                    continue;
                }
                let disp_start =
                    helpers::buffer_to_display_offset(&buffer_to_display, superscript_range.start);
                let disp_end =
                    helpers::buffer_to_display_offset(&buffer_to_display, superscript_range.end);
                push_inline_text_overlay(
                    &mut inline_overlays,
                    window,
                    wrapped,
                    line_content
                        .get(superscript_range.clone())
                        .unwrap_or_default(),
                    disp_start..disp_end,
                    &body_font,
                    &body_cjk_font,
                    theme.syntax_marker,
                    font_size,
                    editor.styles.editor.editor_superscript_scale_factor,
                    InlineOverlayVerticalAlign::Top(
                        editor.scaled(editor.styles.editor.editor_superscript_offset_px),
                    ),
                    false,
                    content_x,
                    text_line_top,
                    indent_px,
                    line_height,
                );
            }

            for subscript_range in &decoration.subscript_spans {
                if !helpers::inline_range_is_collapsed(&decoration, subscript_range, &active_ranges)
                {
                    continue;
                }
                let disp_start =
                    helpers::buffer_to_display_offset(&buffer_to_display, subscript_range.start);
                let disp_end =
                    helpers::buffer_to_display_offset(&buffer_to_display, subscript_range.end);
                push_inline_text_overlay(
                    &mut inline_overlays,
                    window,
                    wrapped,
                    line_content
                        .get(subscript_range.clone())
                        .unwrap_or_default(),
                    disp_start..disp_end,
                    &body_font,
                    &body_cjk_font,
                    theme.syntax_marker,
                    font_size,
                    editor.styles.editor.editor_subscript_scale_factor,
                    InlineOverlayVerticalAlign::Bottom(
                        editor.scaled(editor.styles.editor.editor_subscript_offset_px),
                    ),
                    false,
                    content_x,
                    text_line_top,
                    indent_px,
                    line_height,
                );
            }

            for footnote_reference in &decoration.footnote_references {
                if !helpers::inline_range_is_collapsed(
                    &decoration,
                    &footnote_reference.collapsed_range,
                    &active_ranges,
                ) {
                    continue;
                }
                let disp_start = helpers::buffer_to_display_offset(
                    &buffer_to_display,
                    footnote_reference.collapsed_range.start,
                );
                let disp_end = helpers::buffer_to_display_offset(
                    &buffer_to_display,
                    footnote_reference.collapsed_range.end,
                );
                push_inline_text_overlay(
                    &mut inline_overlays,
                    window,
                    wrapped,
                    line_content
                        .get(footnote_reference.collapsed_range.clone())
                        .unwrap_or_default(),
                    disp_start..disp_end,
                    &body_font,
                    &body_cjk_font,
                    theme.link,
                    font_size,
                    editor.styles.editor.editor_superscript_scale_factor,
                    InlineOverlayVerticalAlign::Offset(-line_height * 0.14),
                    false,
                    content_x,
                    text_line_top,
                    indent_px,
                    line_height,
                );
            }

            // Determine if this is a table row with per-cell rendering
            let is_table_block_line = *block_type == BlockType::Table
                && !active_tab_is_diff
                && decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|ti| !ti.is_separator && !ti.is_front_matter);

            lines.push(PrepaintLine {
                wrapped: wrapped.clone(),
                origin,
                line_height: px(line_height),
                segments,
                is_table_block: is_table_block_line,
            });

            if *block_type == BlockType::UnorderedList && prefix_collapsed {
                let marker_text = editor
                    .unordered_list_display_marker_for_line(line_content)
                    .trim_end()
                    .to_string();
                if !marker_text.is_empty() {
                    let marker_font_size = font_size
                        + editor.scaled(
                            editor
                                .styles
                                .editor
                                .editor_unordered_list_marker_font_size_offset_px,
                        );
                    let marker_run = TextRun {
                        len: marker_text.len(),
                        font: monospace_font.clone(),
                        color: theme.list_marker,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    if let Ok(mut marker_wrapped_lines) = window.text_system().shape_text(
                        marker_text.clone().into(),
                        px(marker_font_size),
                        &[marker_run],
                        None,
                        None,
                    ) && let Some(marker_wrapped) = marker_wrapped_lines.pop()
                    {
                        let marker_x = if leading_ws == 0 {
                            base_content_x
                        } else {
                            content_x
                                + f32::from(
                                    wrapped
                                        .position_for_index(leading_ws, px(line_height))
                                        .unwrap_or_default()
                                        .x,
                                )
                        };
                        let marker_offset = editor.scaled(
                            editor
                                .styles
                                .editor
                                .editor_unordered_list_marker_font_size_offset_px,
                        );
                        let marker_line_height = line_height
                            + editor.scaled(
                                editor
                                    .styles
                                    .editor
                                    .editor_unordered_list_marker_line_height_offset_px,
                            );
                        unordered_list_markers.push((
                            marker_wrapped,
                            point(px(marker_x), px(text_line_top - marker_offset * 0.35)),
                            px(marker_line_height),
                        ));
                    }
                }
            }

            y += total_line_height;
            _last_line_height = total_line_height;
        }
        if editor.preview_overlay_state().is_none() {
            preview_overlay_bounds_local = None;
            preview_overlay_image_bounds_local = None;
        }

        let mut code_block_copy_hitboxes = Vec::new();
        let code_block_styles = &editor.styles.code_block;
        let button_font_size = editor.scaled(code_block_styles.code_block_copy_button_font_size_px);
        let button_padding_x = editor.scaled(code_block_styles.code_block_copy_button_padding_x_px);
        let button_padding_y = editor.scaled(code_block_styles.code_block_copy_button_padding_y_px);
        let button_text_line_height = button_font_size * 1.2;
        let button_right = editor.scaled(code_block_styles.code_block_copy_button_right_px);
        let button_top = editor.scaled(code_block_styles.code_block_copy_button_top_px);
        let button_radius = px(editor.scaled(code_block_styles.code_block_copy_button_radius_px));
        let button_border_width =
            px(editor.scaled(code_block_styles.code_block_copy_button_border_width_px));
        let button_min_left = editor.scaled(code_block_styles.code_block_text_padding_left_px);
        let mut button_font = text_style.font();
        button_font.family = configured_font_family(editor.styles.ui_font_family);
        let code_block_border_width =
            px(editor.scaled(code_block_styles.code_block_border_width_px));
        let code_block_radius = px(editor.scaled(code_block_styles.code_block_radius_px));
        let overlay_active = editor.preview_overlay_state().is_some();
        let preview_outer_button_padding_x = button_padding_x;
        let preview_inner_button_padding_x = editor.scaled(3.0);
        for (start_line, group) in &code_block_group_bounds {
            code_block_bgs.push(quad(
                Bounds::new(
                    point(px(group.left), px(group.top)),
                    size(px(group.width), px(group.bottom - group.top)),
                ),
                Corners::all(code_block_radius),
                gpui::transparent_black(),
                code_block_border_width,
                code_block_styles.code_block_border_color,
                BorderStyle::default(),
            ));

            let group_bounds = Bounds::new(
                point(px(group.left), px(group.top)),
                size(px(group.width), px(group.bottom - group.top)),
            );
            let is_front_matter_group = editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.table_info.as_ref())
                .is_some_and(|table_info| table_info.is_front_matter);

            if is_front_matter_group {
                continue;
            }

            if overlay_active {
                continue;
            }

            code_block_hover_areas_local.push((group_bounds, *start_line));

            let language_label = editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.code_language.as_deref())
                .filter(|language| !language.trim().is_empty())
                .map(|language| language.to_uppercase())
                .unwrap_or_else(|| "CODE".to_string());

            let preview_anchor = editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.block_preview.as_ref())
                .filter(|preview| preview.is_anchor);
            let preview_is_active = preview_anchor.is_some_and(|preview| {
                helpers::block_preview_is_active(preview, cursor_line, &all_sel_ranges)
            });
            let group_syntax_active =
                code_block_group_range(*start_line).is_some_and(|(group_start, group_end)| {
                    active_code_fence_group.is_some_and(|(active_start, active_end)| {
                        group_start == active_start && group_end == active_end
                    })
                });

            let is_copied = editor.copied_code_block_start == Some(*start_line);
            let preview_actions_visible = preview_anchor.is_some()
                && (preview_is_active
                    || editor.hovering_code_block_start == Some(*start_line)
                    || editor.hovering_generated_preview_line == Some(*start_line)
                    || editor.hovering_code_block_copy_start == Some(*start_line)
                    || editor.hovering_generated_preview_zoom_line == Some(*start_line)
                    || is_copied);
            let is_visible = if preview_anchor.is_some() {
                preview_actions_visible
            } else {
                group_syntax_active
                    || editor.hovering_code_block_start == Some(*start_line)
                    || editor.hovering_code_block_copy_start == Some(*start_line)
                    || is_copied
            };
            if !is_visible {
                continue;
            }

            let is_button_hovered = editor.hovering_code_block_copy_start == Some(*start_line);
            let (label_text, button_bg, button_text_color, button_border_color) = if is_copied {
                (
                    editor.t(crate::i18n::TextKey::Copied).to_string(),
                    code_block_styles.code_block_copy_button_copied_bg,
                    code_block_styles.code_block_copy_button_copied_text_color,
                    code_block_styles.code_block_copy_button_copied_border_color,
                )
            } else if preview_anchor.is_some() {
                let preview_label = match preview_anchor.map(|preview| preview.kind) {
                    Some(crate::markdown::BlockPreviewKind::Mermaid) => "MERMAID",
                    Some(crate::markdown::BlockPreviewKind::Math) => "LATEX",
                    None => "COPY",
                };
                (
                    preview_label.to_string(),
                    if is_button_hovered {
                        code_block_styles.code_block_copy_button_hover_bg
                    } else {
                        code_block_styles.code_block_copy_button_bg
                    },
                    if is_button_hovered {
                        code_block_styles.code_block_copy_button_hover_text_color
                    } else {
                        code_block_styles.code_block_copy_button_text_color
                    },
                    if is_button_hovered {
                        code_block_styles.code_block_copy_button_hover_border_color
                    } else {
                        code_block_styles.code_block_copy_button_border_color
                    },
                )
            } else if is_button_hovered {
                (
                    language_label.clone(),
                    code_block_styles.code_block_copy_button_hover_bg,
                    code_block_styles.code_block_copy_button_hover_text_color,
                    code_block_styles.code_block_copy_button_hover_border_color,
                )
            } else {
                (
                    language_label,
                    code_block_styles.code_block_copy_button_bg,
                    code_block_styles.code_block_copy_button_text_color,
                    code_block_styles.code_block_copy_button_border_color,
                )
            };
            let (copy_button_padding_left_x, copy_button_padding_right_x) =
                if preview_anchor.is_some() {
                    (
                        preview_inner_button_padding_x,
                        preview_outer_button_padding_x,
                    )
                } else {
                    (button_padding_x, button_padding_x)
                };
            let Some((label, button_width_raw, _)) = shape_preview_button(
                window,
                &button_font,
                button_font_size,
                copy_button_padding_left_x,
                copy_button_padding_right_x,
                button_padding_y,
                button_text_line_height,
                &label_text,
                button_text_color,
            ) else {
                continue;
            };
            let button_width = button_width_raw.ceil();
            let mut preview_zoom_button = None;
            if editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.block_preview.as_ref())
                .filter(|preview| preview.is_anchor)
                .is_some_and(|preview| {
                    image_cache
                        .get(&preview.cache_key)
                        .is_some_and(|entry| entry.is_some())
                })
            {
                let is_zoom_hovered =
                    editor.hovering_generated_preview_zoom_line == Some(*start_line);
                let (zoom_bg, zoom_text_color, zoom_border_color) = if is_zoom_hovered {
                    (
                        code_block_styles.code_block_copy_button_hover_bg,
                        code_block_styles.code_block_copy_button_hover_text_color,
                        code_block_styles.code_block_copy_button_hover_border_color,
                    )
                } else {
                    (
                        code_block_styles.code_block_copy_button_bg,
                        code_block_styles.code_block_copy_button_text_color,
                        code_block_styles.code_block_copy_button_border_color,
                    )
                };
                if let Some((zoom_label, zoom_button_width_raw, zoom_button_height_raw)) =
                    shape_preview_button(
                        window,
                        &button_font,
                        button_font_size,
                        preview_outer_button_padding_x,
                        preview_inner_button_padding_x,
                        button_padding_y,
                        button_text_line_height,
                        "ZOOM",
                        zoom_text_color,
                    )
                {
                    preview_zoom_button = Some((
                        zoom_label,
                        zoom_button_width_raw.ceil(),
                        zoom_button_height_raw.ceil(),
                        zoom_bg,
                        zoom_border_color,
                    ));
                }
            }

            let button_height = preview_zoom_button
                .as_ref()
                .map(|(_, _, zoom_height, _, _)| {
                    (button_text_line_height + button_padding_y * 2.0)
                        .ceil()
                        .max(*zoom_height)
                })
                .unwrap_or_else(|| (button_text_line_height + button_padding_y * 2.0).ceil());
            let action_anchor_bounds = generated_preview_hit_areas
                .iter()
                .find(|(_, line_idx)| *line_idx == *start_line)
                .map(|(bounds, _)| *bounds)
                .unwrap_or(group_bounds);
            let action_anchor_left = f32::from(action_anchor_bounds.origin.x);
            let action_anchor_top = f32::from(action_anchor_bounds.origin.y);
            let action_anchor_width = f32::from(action_anchor_bounds.size.width);
            let action_group_width = preview_zoom_button
                .as_ref()
                .map(|(_, zoom_width, _, _, _)| zoom_width + preview_button_gap + button_width)
                .unwrap_or(button_width);
            let action_group_x =
                ((action_anchor_left + action_anchor_width - button_right - action_group_width)
                    .max(action_anchor_left + button_min_left))
                .round();
            let button_x = preview_zoom_button
                .as_ref()
                .map(|(_, zoom_width, _, _, _)| action_group_x + zoom_width + preview_button_gap)
                .unwrap_or(action_group_x)
                .round();
            let button_y = (action_anchor_top + button_top).round();
            let button_bounds = Bounds::new(
                point(px(button_x), px(button_y)),
                size(px(button_width), px(button_height)),
            );

            code_block_copy_areas_local.push((button_bounds, *start_line));
            code_block_copy_hitboxes
                .push(window.insert_hitbox(button_bounds, HitboxBehavior::Normal));
            code_block_copy_buttons.push(CodeBlockCopyButtonPaint {
                quad: quad(
                    button_bounds,
                    Corners::all(button_radius),
                    button_bg,
                    button_border_width,
                    button_border_color,
                    BorderStyle::default(),
                ),
                label,
                origin: point(
                    px(button_x + copy_button_padding_left_x),
                    px(button_y + button_padding_y),
                ),
                line_height: px(button_text_line_height),
            });

            if let Some((zoom_label, zoom_button_width, _, zoom_bg, zoom_border_color)) =
                preview_zoom_button
            {
                let zoom_x = action_group_x;
                let zoom_bounds = Bounds::new(
                    point(px(zoom_x), px(button_y)),
                    size(px(zoom_button_width), px(button_height)),
                );

                generated_preview_zoom_areas_local.push((zoom_bounds, *start_line));
                preview_zoom_hitboxes
                    .push(window.insert_hitbox(zoom_bounds, HitboxBehavior::Normal));
                preview_zoom_buttons.push(CodeBlockCopyButtonPaint {
                    quad: quad(
                        zoom_bounds,
                        Corners::all(button_radius),
                        zoom_bg,
                        button_border_width,
                        zoom_border_color,
                        BorderStyle::default(),
                    ),
                    label: zoom_label,
                    origin: point(
                        px(zoom_x + preview_outer_button_padding_x),
                        px(button_y + button_padding_y),
                    ),
                    line_height: px(button_text_line_height),
                });
            }
        }

        if !overlay_active {
            for (target_bounds, line_idx) in &image_zoom_button_targets {
                let Some((zoom_label, zoom_button_width_raw, button_height_raw)) =
                    shape_preview_button(
                        window,
                        &button_font,
                        button_font_size,
                        button_padding_x,
                        button_padding_x,
                        button_padding_y,
                        button_text_line_height,
                        "ZOOM",
                        code_block_styles.code_block_copy_button_text_color,
                    )
                else {
                    continue;
                };
                let zoom_button_width = zoom_button_width_raw.ceil();
                let button_height = button_height_raw.ceil();
                let target_left = f32::from(target_bounds.origin.x);
                let target_top = f32::from(target_bounds.origin.y);
                let target_width = f32::from(target_bounds.size.width);
                let min_button_left = (target_left + editor.scaled(12.0)).round();
                let zoom_x = (target_left + target_width - button_right - zoom_button_width)
                    .max(min_button_left)
                    .round();
                let button_y = (target_top + button_top).round();
                let zoom_bounds = Bounds::new(
                    point(px(zoom_x), px(button_y)),
                    size(px(zoom_button_width), px(button_height)),
                );

                image_preview_zoom_areas_local.push((zoom_bounds, *line_idx));
                preview_zoom_hitboxes
                    .push(window.insert_hitbox(zoom_bounds, HitboxBehavior::Normal));
                preview_zoom_buttons.push(CodeBlockCopyButtonPaint {
                    quad: quad(
                        zoom_bounds,
                        Corners::all(button_radius),
                        code_block_styles.code_block_copy_button_bg,
                        button_border_width,
                        code_block_styles.code_block_copy_button_border_color,
                        BorderStyle::default(),
                    ),
                    label: zoom_label,
                    origin: point(
                        px(zoom_x + button_padding_x),
                        px(button_y + button_padding_y),
                    ),
                    line_height: px(button_text_line_height),
                });
            }
        }

        self.editor.update(cx, |editor, cx| {
            editor.apply_line_display_info_updates(line_count, display_info_updates);
            editor.last_viewport_height = viewport_height;
            editor.last_viewport_width = viewport_width;
            editor.checkbox_areas = checkbox_hit_areas.clone();
            editor.image_click_areas = image_hit_areas.clone();
            editor.image_preview_zoom_areas = image_preview_zoom_areas_local.clone();
            editor.generated_preview_click_areas = generated_preview_hit_areas.clone();
            editor.generated_preview_image_areas = generated_preview_image_areas.clone();
            editor.generated_preview_zoom_areas = generated_preview_zoom_areas_local.clone();
            editor.code_block_hover_areas = code_block_hover_areas_local.clone();
            editor.code_block_copy_areas = code_block_copy_areas_local.clone();
            editor.table_click_areas = table_click_areas_local.clone();
            editor.table_col_handle_areas = table_col_handle_areas_local.clone();
            editor.table_row_handle_areas = table_row_handle_areas_local.clone();
            editor.gutter_fold_areas = gutter_fold_hit_areas_local.clone();
            editor.image_edit_areas = image_edit_hit_areas_local.clone();
            editor.preview_overlay_bounds = preview_overlay_bounds_local;
            editor.preview_overlay_image_bounds = preview_overlay_image_bounds_local;
            editor.last_editor_left = editor_left;
            editor.last_editor_width = editor_width;
            editor.last_content_left = editor_left + editor.padding_left();
            editor.last_text_width = text_width;
            if scroll_anchor_delta.abs() > 0.5
                && editor.active_tab_is_non_markdown_text()
                && editor.editor_wraps_to_available_width()
                && editor.pending_preview_reveal.is_none()
                && !editor.pending_center_cursor_in_view
                && !editor.pending_scroll_to_cursor
            {
                editor.scroll_offset = (editor.scroll_offset + scroll_anchor_delta).max(0.0);
            }
            editor.content_origin = bounds.origin;
            editor.layout_dirty = false;
            if deferred_offscreen_wrap_lines {
                cx.notify();
            }
            let mut did_adjust_preview_reveal = false;
            if let Some(pending) = editor.pending_preview_reveal.take() {
                editor.pending_scroll_to_cursor = false;
                editor.pending_center_cursor_in_view = false;
                editor.scroll_offset = pending.scroll_offset;
                editor.clamp_scroll();
                did_adjust_preview_reveal = true;
            }
            if editor.pending_center_cursor_in_view {
                editor.center_cursor_in_view(cx);
            } else if editor.pending_scroll_to_cursor {
                editor.scroll_to_cursor();
                cx.notify();
            } else if did_adjust_preview_reveal {
                cx.notify();
            }
        });

        // Trigger loading for any image paths not yet in cache
        let decorations_ref = self.editor.read(cx).decorations.clone();
        let mut new_image_paths: Vec<String> = Vec::new();
        for dec in &decorations_ref {
            if let Some(ref path) = dec.image_path {
                let has = self.editor.read(cx).image_cache.contains_key(path);
                if !has {
                    new_image_paths.push(path.clone());
                }
            }
        }
        if !new_image_paths.is_empty() {
            for path in new_image_paths {
                self.editor.update(cx, |editor, cx| {
                    editor.queue_image_load(path, cx);
                });
            }
        }

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        EditorPrepaintState {
            hitbox,
            link_hitboxes,
            lines,
            indent_guide_quads,
            whitespace_dots,
            cursor_quads,
            selection_quads,
            search_match_quads,
            diff_line_bgs,
            diff_inline_quads,
            line_highlight,
            code_block_bgs,
            unordered_list_markers,
            marked_underlines,
            gutter_quads,
            gutter_labels,
            gutter_svgs,
            gutter_fold_icons,
            hr_lines,
            image_previews,
            image_borders,
            table_borders,
            table_bgs,
            table_cell_texts,
            code_block_copy_buttons,
            code_block_copy_hitboxes,
            preview_zoom_buttons,
            preview_zoom_hitboxes,
            image_edit_icons,
            link_underlines,
            inline_overlays,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = self.editor.read(cx);
        let focus_handle = editor.focus_handle.clone();
        let cursor_style = match editor.hovering_handle {
            Some((_, true, _)) => CursorStyle::ResizeColumn,
            Some((_, false, _)) => CursorStyle::ResizeRow,
            None if editor.hovering_checkbox_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_fold_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_image_preview_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_generated_preview_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_generated_preview_zoom_line.is_some() => CursorStyle::Arrow,
            None if editor.editor_scrollbar_hovered => CursorStyle::Arrow,
            None => CursorStyle::IBeam,
        };
        window.set_cursor_style(cursor_style, &prepaint.hitbox);
        for hitbox in &prepaint.code_block_copy_hitboxes {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        for hitbox in &prepaint.preview_zoom_hitboxes {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        for link_hitbox in &prepaint.link_hitboxes {
            window.set_cursor_style(CursorStyle::PointingHand, link_hitbox);
        }
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        {
            let editor = self.editor.clone();
            window.on_modifiers_changed(move |event, _window, cx| {
                editor.update(cx, |this, cx| {
                    let new_val = event.modifiers.platform;
                    if this.command_held != new_val {
                        this.command_held = new_val;
                        cx.notify();
                    }
                });
            });
        }

        for quad in prepaint.code_block_bgs.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.table_bgs.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.diff_line_bgs.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.indent_guide_quads.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.diff_inline_quads.drain(..) {
            window.paint_quad(quad);
        }

        if let Some(highlight) = prepaint.line_highlight.take() {
            window.paint_quad(highlight);
        }

        // Search match highlights (behind selection)
        for quad in prepaint.search_match_quads.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.selection_quads.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.whitespace_dots.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.gutter_quads.drain(..) {
            window.paint_quad(quad);
        }
        for (icon_bounds, icon_path, color) in &prepaint.gutter_svgs {
            window
                .paint_svg(
                    *icon_bounds,
                    (*icon_path).into(),
                    TransformationMatrix::default(),
                    *color,
                    cx,
                )
                .ok();
        }
        for (shaped, origin, line_height) in &prepaint.gutter_labels {
            shaped.paint(*origin, *line_height, window, cx).ok();
        }

        for (shaped, origin, line_height) in &prepaint.inline_overlays {
            shaped.paint(*origin, *line_height, window, cx).ok();
        }

        for quad in prepaint.table_borders.drain(..) {
            window.paint_quad(quad);
        }

        // Table cell texts (per-cell rendering)
        for (shaped, origin, lh) in &prepaint.table_cell_texts {
            shaped
                .paint(*origin, *lh, TextAlign::Left, None, window, cx)
                .ok();
        }

        // HR dashed lines
        for (hr_y, hr_start_x, hr_width) in &prepaint.hr_lines {
            let dash_len = 6.0_f32;
            let gap_len = 4.0_f32;
            let mut x = *hr_start_x;
            let end_x = *hr_start_x + hr_width;
            let theme = &self.editor.read(cx).theme;
            while x < end_x {
                let seg_end = (x + dash_len).min(end_x);
                window.paint_quad(fill(
                    Bounds::new(point(px(x), px(*hr_y)), size(px(seg_end - x), px(1.0))),
                    theme.border_decorative,
                ));
                x = seg_end + gap_len;
            }
        }

        for line in &prepaint.lines {
            // Skip normal text rendering for table rows (rendered via table_cell_texts)
            if line.is_table_block {
                continue;
            }
            if let Some(ref segments) = line.segments {
                for (seg_wrapped, seg_origin) in segments {
                    seg_wrapped
                        .paint(
                            *seg_origin,
                            line.line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        )
                        .ok();
                }
            } else {
                line.wrapped
                    .paint(
                        line.origin,
                        line.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
            }
        }

        for (marker, origin, line_height) in &prepaint.unordered_list_markers {
            marker
                .paint(*origin, *line_height, TextAlign::Left, None, window, cx)
                .ok();
        }

        for button in &prepaint.code_block_copy_buttons {
            window.paint_quad(button.quad.clone());
            button
                .label
                .paint(
                    button.origin,
                    button.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
        }

        // Image previews
        for (img_bounds, img_data, corner_radius) in &prepaint.image_previews {
            window
                .paint_image(
                    *img_bounds,
                    Corners::all(*corner_radius),
                    img_data.clone(),
                    0,
                    false,
                )
                .ok();
        }

        for quad in prepaint.image_borders.drain(..) {
            window.paint_quad(quad);
        }

        for button in &prepaint.preview_zoom_buttons {
            window.paint_quad(button.quad.clone());
            button
                .label
                .paint(
                    button.origin,
                    button.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
        }

        // Image URL-edit icons
        for (icon_bounds, _line, alpha) in &prepaint.image_edit_icons {
            let theme = &self.editor.read(cx).theme;
            let mut color = theme.text_secondary;
            color.a = *alpha;
            window
                .paint_svg(
                    *icon_bounds,
                    OPEN_SVG_PATH.into(),
                    TransformationMatrix::default(),
                    color,
                    cx,
                )
                .ok();
        }

        for (icon_bounds, icon_path, color) in &prepaint.gutter_fold_icons {
            window
                .paint_svg(
                    *icon_bounds,
                    (*icon_path).into(),
                    TransformationMatrix::default(),
                    *color,
                    cx,
                )
                .ok();
        }

        for cursor in prepaint.cursor_quads.drain(..) {
            window.paint_quad(cursor);
        }

        for quad in prepaint.marked_underlines.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.link_underlines.drain(..) {
            window.paint_quad(quad);
        }
    }
}
