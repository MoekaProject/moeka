//! Cell display shaping and geometry, separate from semantic table parsing.
use std::{collections::HashMap, ops::Range, sync::Arc};

use gpui::{
    Bounds, LineLayout, Pixels, Point, RenderImage, TextRun, Window, WrappedLine,
    WrappedLineLayout, point, px, size,
};

use super::{WrapBoundaryAffinityKind, helpers};

use crate::markdown::TableLineInfo;

/// Restore editable whitespace around semantic cells.
/// Immediately after a pipe, one optional ASCII space is formatting padding;
/// any further spaces/tabs belong to the cell. Never
/// borrow row/container indentation or synthesize source bytes.
pub(super) fn content_with_range(
    info: &TableLineInfo,
    source_line: &str,
    col: usize,
) -> (String, Range<usize>) {
    let mut content = info.cells.get(col).cloned().unwrap_or_default();
    let mut range = info.cell_ranges.get(col).cloned().unwrap_or(0..0);
    // Front matter has synthetic cells; stale or invalid source metadata must
    // retain the existing extraction rather than slicing an unrelated range.
    if info.is_front_matter
        || info.cells.get(col).is_none()
        || info.cell_ranges.get(col).is_none()
        || source_line.get(range.clone()) != Some(content.as_str())
    {
        return (content, range);
    }

    let tail = &source_line[range.end..];
    let trailing_len = tail.len() - tail.trim_start_matches([' ', '\t']).len();
    content.push_str(&tail[..trailing_len]);
    range.end += trailing_len;

    let before = &source_line[..range.start];
    let padding_start = before.trim_end_matches([' ', '\t']).len();
    if before[..padding_start].ends_with('|') {
        let formatting_space =
            usize::from(source_line.as_bytes().get(padding_start) == Some(&b' '));
        range.start = padding_start + formatting_space;
        content = source_line[range.clone()].to_owned();
    }
    (content, range)
}

/// Reserve the checkbox and its gap using the caller's measured monospace
/// advance (including tracking), while retaining the ordinary three columns.
pub(super) fn task_prefix_columns(monospace_advance: f32, checkbox_size: f32, gap: f32) -> usize {
    let width = checkbox_size + gap;
    if !monospace_advance.is_finite() || !width.is_finite() {
        return 3;
    }
    ((width / monospace_advance.max(1.0)).ceil() as usize).max(3)
}

/// Ignore only real trailing source padding. Display spaces can reserve visible
/// scripts, footnotes or list markers and must still contribute to column width.
pub(super) fn intrinsic_line_end(
    content: &str,
    fragment: &crate::markdown::TableCellFragment,
    buffer_to_display: &[usize],
    display_range: &Range<usize>,
) -> usize {
    let source = &content[fragment.range.clone()];
    let padding = source.len() - source.trim_end_matches([' ', '\t']).len();
    let source_end = (fragment.range.end - padding).max(fragment.range.start + fragment.prefix_len);
    buffer_to_display
        .get(source_end)
        .copied()
        .unwrap_or(display_range.end)
        .saturating_sub(display_range.start)
        .min(display_range.len())
}

pub(super) type CellDisplay = (String, Vec<usize>, Vec<usize>, bool, Vec<TextRun>);

pub(super) struct PreparedCell {
    pub display: CellDisplay,
    pub lines: Vec<WrappedLine>,
    pub hard_ranges: Vec<Range<usize>>,
    pub wraps: Vec<HangingWrap>,
}

/// GPUI stores row breaks/heights but not hanging indentation. Keep the latter
/// beside that same layout, and use row indices (not y/line-height division).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HangingWrap {
    pub indent: f32,
}

impl HangingWrap {
    pub fn row_indent(self, row: usize) -> f32 {
        if row == 0 { 0.0 } else { self.indent }
    }

    pub fn position(
        self,
        line: &WrappedLine,
        text: &str,
        index: usize,
        line_height: f32,
        affinity: Option<WrapBoundaryAffinityKind>,
    ) -> Point<Pixels> {
        let boundaries = helpers::wrap_boundary_byte_indices(line, text);
        let row = boundaries.partition_point(|&boundary| {
            boundary < index
                || (boundary == index && affinity == Some(WrapBoundaryAffinityKind::Downstream))
        });
        let start = row.checked_sub(1).map_or(0, |row| boundaries[row]);
        point(
            line.unwrapped_layout.x_for_index(index) - line.unwrapped_layout.x_for_index(start)
                + px(self.row_indent(row)),
            line.row_top(row, px(line_height)),
        )
    }

    pub fn hit_x(self, line: &WrappedLine, x: f32, y: Pixels, line_height: f32) -> Pixels {
        px((x - self.row_indent(line.row_for_y(y, px(line_height)))).max(0.0))
    }

    pub fn selection_bounds(
        self,
        line: &WrappedLine,
        text: &str,
        selection: Range<usize>,
        origin: Point<Pixels>,
        line_height: f32,
    ) -> Vec<Bounds<Pixels>> {
        let boundaries = helpers::wrap_boundary_byte_indices(line, text);
        let row_xs = soft_row_xs(line);
        let mut row_top = origin.y;
        helpers::wrap_segment_byte_ranges(text.len(), &boundaries)
            .into_iter()
            .enumerate()
            .filter_map(|(row, range)| {
                let top = row_top;
                let height = line.row_height(row, px(line_height));
                row_top += height;
                let start = range.start.max(selection.start);
                let end = range.end.min(selection.end);
                if start >= end {
                    return None;
                }
                let start_x = if start == range.start {
                    row_xs[row]
                } else {
                    line.unwrapped_layout.x_for_index(start)
                };
                let end_x = if end == range.end {
                    row_xs[row + 1]
                } else {
                    line.unwrapped_layout.x_for_index(end)
                };
                Some(Bounds::new(
                    point(
                        origin.x + px(self.row_indent(row)) + start_x - row_xs[row],
                        top,
                    ),
                    size(end_x - start_x, height),
                ))
            })
            .collect()
    }
}

fn display_prefix_end(
    fragment: &crate::markdown::TableCellFragment,
    buffer_to_display: &[usize],
    range: &Range<usize>,
) -> usize {
    if fragment.prefix_len == 0 {
        return 0;
    }
    buffer_to_display
        .get(fragment.range.start + fragment.prefix_len)
        .copied()
        .unwrap_or(range.start)
        .saturating_sub(range.start)
        .min(range.len())
}

/// Reveal only prefixes whose *shaped* reservation cannot fit. At most one
/// rebuild is needed: revealing one fragment cannot change any other fragment.
fn expand_narrow_prefixes(
    content: &str,
    decoration: &crate::markdown::LineDecoration,
    active_ranges: &mut Vec<Range<usize>>,
    buffer_to_display: &[usize],
    lines: &[WrappedLine],
    ranges: &[Range<usize>],
    inner_width: f32,
) -> bool {
    let mut changed = false;
    for ((fragment, line), range) in decoration
        .table_cell_fragments
        .iter()
        .zip(lines)
        .zip(ranges)
    {
        let end = display_prefix_end(fragment, buffer_to_display, range);
        if end > 0
            && f32::from(line.unwrapped_layout.x_for_index(end)) >= inner_width
            && helpers::table_cell_fragment_prefix_is_folded(content, fragment, active_ranges)
        {
            active_ranges.push(fragment.range.start..fragment.range.start + fragment.prefix_len);
            changed = true;
        }
    }
    changed
}

/// One bounded two-pass path for measurement, paint and input/IME. The builder
/// applies final font weights before measuring, including bold table headers.
pub(super) fn prepare_cell(
    window: &mut Window,
    content: &str,
    decoration: &crate::markdown::LineDecoration,
    active_ranges: &mut Vec<Range<usize>>,
    inner_width: f32,
    font_size: f32,
    tracking: f32,
    line_height: f32,
    image_cache: &HashMap<String, Option<Arc<RenderImage>>>,
    build: impl Fn(&[Range<usize>]) -> CellDisplay,
) -> anyhow::Result<PreparedCell> {
    let inner_width = inner_width.max(1.0);
    for pass in 0..2 {
        let display = build(active_ranges);
        let text = if display.0.is_empty() {
            " ".into()
        } else {
            display.0.clone().into()
        };
        let mut lines = window
            .text_system()
            .shape_text(text, px(font_size), &display.4, None, None)?
            .into_iter()
            .collect::<Vec<_>>();
        let hard_ranges = helpers::hard_line_byte_ranges(&display.0);
        for line in &mut lines {
            helpers::apply_letter_spacing(line, tracking);
        }
        if pass == 0
            && expand_narrow_prefixes(
                content,
                decoration,
                active_ranges,
                &display.2,
                &lines,
                &hard_ranges,
                inner_width,
            )
        {
            continue;
        }
        let mut wraps = Vec::with_capacity(lines.len());
        for (index, (line, range)) in lines.iter_mut().zip(&hard_ranges).enumerate() {
            helpers::keep_inline_math_placeholders_unwrapped(
                line,
                decoration,
                content,
                &display.2,
                active_ranges,
                image_cache,
                range.start,
            );
            let prefix = decoration
                .table_cell_fragments
                .get(index)
                .map_or(0, |fragment| {
                    display_prefix_end(fragment, &display.2, range)
                });
            let wrap = wrap_fragment(line, prefix, inner_width, |body, prefix| {
                helpers::keep_inline_math_placeholders_unwrapped(
                    body,
                    decoration,
                    content,
                    &display.2,
                    active_ranges,
                    image_cache,
                    range.start + prefix,
                );
            });
            helpers::apply_inline_math_row_heights(
                line,
                decoration,
                content,
                &display.2,
                active_ranges,
                image_cache,
                line_height,
                range.start,
            );
            wraps.push(wrap);
        }
        return Ok(PreparedCell {
            display,
            lines,
            hard_ranges,
            wraps,
        });
    }
    unreachable!("cell prefix expansion takes at most one rebuild")
}

/// Wrap the body at the remaining width, then translate its GPUI boundaries
/// back into the original full-fragment layout. This keeps the first marker
/// intact without reserving its width twice on the first row.
fn wrap_fragment(
    line: &mut WrappedLine,
    prefix: usize,
    inner_width: f32,
    mut keep_atoms: impl FnMut(&mut WrappedLine, usize),
) -> HangingWrap {
    let indent = f32::from(line.unwrapped_layout.x_for_index(prefix));
    if prefix == 0 || !indent.is_finite() || indent <= 0.0 || indent >= inner_width {
        line.rewrap(px(inner_width));
        keep_atoms(line, 0);
        return HangingWrap::default();
    }
    let mut body = line.clone();
    let original = line.unwrapped_layout.clone();
    let mut skipped = Vec::with_capacity(original.runs.len());
    let mut runs = original.runs.clone();
    for run in &mut runs {
        let count = run.glyphs.partition_point(|glyph| glyph.index < prefix);
        skipped.push(count);
        run.glyphs.drain(..count);
        for glyph in &mut run.glyphs {
            glyph.index -= prefix;
            glyph.position.x -= px(indent);
        }
    }
    body.text = line.text.get(prefix..).unwrap_or("").to_owned().into();
    let layout: &mut Arc<WrappedLineLayout> = &mut body;
    *layout = Arc::new(WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            runs,
            width: original.width - px(indent),
            len: original.len.saturating_sub(prefix),
            font_size: original.font_size,
            ascent: original.ascent,
            descent: original.descent,
        }),
        ..Default::default()
    });
    body.rewrap(px(inner_width - indent));
    keep_atoms(&mut body, prefix);
    let mut row_start = px(0.0);
    let too_wide = body
        .wrap_boundaries
        .iter()
        .map(|boundary| {
            body.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix]
                .position
                .x
        })
        .chain(std::iter::once(body.unwrapped_layout.width))
        .any(|end| {
            let width = end - row_start;
            row_start = end;
            f32::from(width) > inner_width - indent + 0.01
        });
    if too_wide {
        // A wide CJK glyph or atomic formula may fit the cell but not its
        // indented body lane. Never push it into the neighboring column.
        line.rewrap(px(inner_width));
        keep_atoms(line, 0);
        return HangingWrap::default();
    }
    let layout: &mut Arc<WrappedLineLayout> = &mut *line;
    *layout = Arc::new(WrappedLineLayout {
        unwrapped_layout: original,
        wrap_boundaries: body
            .wrap_boundaries
            .iter()
            .map(|boundary| gpui::WrapBoundary {
                run_ix: boundary.run_ix,
                glyph_ix: boundary.glyph_ix + skipped[boundary.run_ix],
            })
            .collect(),
        wrap_width: Some(px(inner_width - indent)),
        row_heights: Vec::new(),
    });
    HangingWrap { indent }
}

fn soft_row_xs(line: &WrappedLine) -> Vec<Pixels> {
    std::iter::once(px(0.0))
        .chain(line.wrap_boundaries.iter().map(|boundary| {
            line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix]
                .position
                .x
        }))
        .chain(std::iter::once(line.unwrapped_layout.width))
        .collect()
}

/// Split glyph geometry once, rather than cloning/scanning the whole hard line
/// for every soft row. Decoration shaping below does not replace these advances.
fn segment_layouts(original: &WrappedLine) -> Vec<(Range<usize>, Arc<LineLayout>)> {
    let source = &original.unwrapped_layout;
    let boundaries = helpers::wrap_boundary_byte_indices(original, &original.text);
    let ranges = helpers::wrap_segment_byte_ranges(original.text.len(), &boundaries);
    let row_xs = soft_row_xs(original);
    let mut layouts = ranges
        .iter()
        .enumerate()
        .map(|(row, range)| LineLayout {
            runs: Vec::new(),
            width: row_xs[row + 1] - row_xs[row],
            len: range.len(),
            font_size: source.font_size,
            ascent: source.ascent,
            descent: source.descent,
        })
        .collect::<Vec<_>>();
    for run in &source.runs {
        let mut last_row = None;
        for glyph in &run.glyphs {
            let row = boundaries.partition_point(|&boundary| boundary <= glyph.index);
            if last_row != Some(row) {
                layouts[row].runs.push(gpui::ShapedRun {
                    font_id: run.font_id,
                    glyphs: Vec::new(),
                });
                last_row = Some(row);
            }
            let mut glyph = glyph.clone();
            glyph.position.x -= row_xs[row];
            glyph.index -= ranges[row].start;
            layouts[row].runs.last_mut().unwrap().glyphs.push(glyph);
        }
    }
    ranges
        .into_iter()
        .zip(layouts.into_iter().map(Arc::new))
        .collect()
}

pub(super) fn paint_segments(
    window: &mut Window,
    original: &WrappedLine,
    runs: &[TextRun],
    hanging: HangingWrap,
    line_height: f32,
) -> anyhow::Result<Vec<(WrappedLine, Point<Pixels>)>> {
    let mut result = Vec::new();
    let mut run_index = 0;
    let mut run_start = 0;
    let mut top = px(0.0);
    for (row, (range, geometry)) in segment_layouts(original).into_iter().enumerate() {
        let mut segment_runs = Vec::new();
        while let Some(run) = runs.get(run_index) {
            let run_end = run_start + run.len;
            if run_start >= range.end {
                break;
            }
            let start = range.start.max(run_start);
            let end = range.end.min(run_end);
            if start < end {
                let mut run = run.clone();
                run.len = end - start;
                segment_runs.push(run);
            }
            if run_end > range.end {
                break;
            }
            run_start = run_end;
            run_index += 1;
        }
        let text = original.text[range].to_owned();
        let mut segment = window
            .text_system()
            .shape_text(
                text.into(),
                original.unwrapped_layout.font_size,
                &segment_runs,
                None,
                None,
            )?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing cell paint segment"))?;
        let layout: &mut Arc<WrappedLineLayout> = &mut segment;
        *layout = Arc::new(WrappedLineLayout {
            unwrapped_layout: geometry,
            row_heights: vec![original.row_height(row, px(line_height))],
            ..Default::default()
        });
        result.push((segment, point(px(hanging.row_indent(row)), top)));
        top += original.row_height(row, px(line_height));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{Arc, HangingWrap, LineLayout, WrappedLine, WrappedLineLayout, point, px};
    use gpui::{FontId, FontStyle, FontWeight, GlyphId, ShapedGlyph, ShapedRun, TextRun, font};

    use super::super::helpers;
    use super::{content_with_range, task_prefix_columns};
    use crate::{buffer::Buffer, markdown, theme::Theme};

    struct RenderedCell {
        content: String,
        cell_range: Range<usize>,
        decoration: markdown::LineDecoration,
        active_ranges: Vec<Range<usize>>,
        raw_display: String,
        display_range: Range<usize>,
        display: String,
        display_to_buffer: Vec<usize>,
        buffer_to_display: Vec<usize>,
        prefix_collapsed: bool,
        runs: Vec<TextRun>,
    }

    /// Deterministic shaped runs, with distinct body/mono/CJK and bold metrics.
    /// These feed the production GPUI rewrapper and shared geometry, not a
    /// second implementation of wrapping. No native window/fonts are needed.
    fn measured_line(
        text: &str,
        runs: &[TextRun],
        metrics: [f32; 3],
        zoom: f32,
        tracking: f32,
        header: bool,
    ) -> WrappedLine {
        let mut shaped_runs = Vec::new();
        let mut run_index = 0;
        let mut run_end = runs.first().map_or(text.len(), |run| run.len);
        let mut x = 0.0;
        for (index, ch) in text.char_indices() {
            while index >= run_end && run_index + 1 < runs.len() {
                run_index += 1;
                run_end += runs[run_index].len;
            }
            let run = runs.get(run_index);
            let metric = match run.map(|run| run.font.family.as_ref()) {
                Some("Test Mono") => metrics[0],
                Some("Test CJK") => metrics[2],
                _ => metrics[1],
            };
            let bold = header || run.is_some_and(|run| run.font.weight == FontWeight::BOLD);
            let advance = metric * zoom * if bold { 1.17 } else { 1.0 };
            let font_id = FontId(run_index);
            if shaped_runs
                .last()
                .is_none_or(|run: &ShapedRun| run.font_id != font_id)
            {
                shaped_runs.push(ShapedRun {
                    font_id,
                    glyphs: Vec::new(),
                });
            }
            shaped_runs.last_mut().unwrap().glyphs.push(ShapedGlyph {
                // Glyphs are never sent to native painting in numerical tests.
                id: unsafe { std::mem::zeroed::<GlyphId>() },
                position: point(px(x), px(0.0)),
                index,
                is_emoji: ch as u32 > 0xffff,
            });
            x += advance;
        }
        let mut line = WrappedLine::default();
        line.text = text.to_owned().into();
        let layout: &mut Arc<WrappedLineLayout> = &mut line;
        *layout = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: px(16.0 * zoom),
                width: px(x),
                ascent: px(12.0 * zoom),
                descent: px(4.0 * zoom),
                runs: shaped_runs,
                len: text.len(),
            }),
            ..Default::default()
        });
        helpers::apply_letter_spacing(&mut line, tracking);
        line
    }

    /// No implicit space before the closing pipe: completion must come from
    /// the supplied source. A preceding UTF-8 cell exercises line-relative maps.
    fn render_cell(
        source: &str,
        carets: &[usize],
        marked_range: Option<Range<usize>>,
    ) -> RenderedCell {
        render_cell_with_focus(
            source,
            |cell_start| helpers::TableCellSyntaxFocus {
                selections: carets
                    .iter()
                    .map(|offset| cell_start + offset..cell_start + offset)
                    .collect(),
                marked_ranges: marked_range
                    .map(|range| cell_start + range.start..cell_start + range.end)
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
            [1.0; 3],
        )
    }

    fn render_cell_with_focus(
        source: &str,
        focus: impl FnOnce(usize) -> helpers::TableCellSyntaxFocus,
        scales: [f32; 3],
    ) -> RenderedCell {
        let text = format!("| Neighbor | Input |\n| --- | --- |\n| 隣🙂 | {source}|");
        let buffer = Buffer::from_text(&text);
        let decorations = markdown::parse_decorations(&text);
        let row = buffer.line_content(2);
        let info = decorations[2].table_info.as_ref().unwrap();
        let (content, cell_range) = content_with_range(info, row, 1);
        assert_eq!(&row[cell_range.clone()], content);
        let line_start = buffer.line_start(2);
        let cell_start = line_start + cell_range.start;
        let decoration = helpers::table_cell_decoration(&decorations[2], &content, &cell_range);
        let active_ranges = helpers::table_cell_active_ranges(
            line_start,
            &cell_range,
            &focus(cell_start),
            &decoration,
        );
        let (raw_display, raw_d2b, raw_b2d, prefix_collapsed) = helpers::build_display_text(
            &content,
            &decoration,
            &active_ranges,
            false,
            None,
            0,
            3,
            3,
            2,
            0,
            scales[0],
            scales[1],
            scales[2],
        );
        let display_range =
            helpers::table_cell_display_range(&raw_display, &content, &decoration, &raw_b2d, &[]);
        assert!(display_range.start <= display_range.end);
        assert!(raw_display.is_char_boundary(display_range.start));
        assert!(raw_display.is_char_boundary(display_range.end));
        let display = raw_display[display_range.clone()].to_string();
        let display_to_buffer = raw_d2b[display_range.clone()].to_vec();
        let buffer_to_display = raw_b2d
            .iter()
            .map(|offset| {
                offset
                    .saturating_sub(display_range.start)
                    .min(display.len())
            })
            .collect::<Vec<_>>();
        let theme = Theme::one_dark();
        let runs = helpers::build_text_runs_for_display(
            &display,
            &content,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font("Test Body"),
            &font("Test CJK"),
            &font("Test Mono"),
            &font("Test Heading"),
            &font("Test Heading CJK"),
            &font("Test Italic"),
            &font("Test Italic CJK"),
            3,
            16.0,
            &theme,
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_to_buffer.len(), display.len());
        assert_eq!(buffer_to_display.len(), content.len() + 1);
        assert!(
            display_to_buffer
                .iter()
                .all(|offset| *offset < content.len())
        );
        assert!(
            buffer_to_display
                .iter()
                .all(|offset| *offset <= display.len())
        );
        assert!(buffer_to_display.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(buffer_to_display[content.len()], display.len());
        for offset in content.char_indices().map(|(offset, _)| offset) {
            assert!(
                display.is_char_boundary(buffer_to_display[offset]),
                "source byte {offset} maps inside a display character: source {content:?}, display {display:?}, scales {scales:?}",
            );
        }
        for offset in display.char_indices().map(|(offset, _)| offset) {
            assert!(
                content.is_char_boundary(display_to_buffer[offset]),
                "display byte {offset} maps inside a source character at {}: source {content:?}, display {display:?}, scales {scales:?}",
                display_to_buffer[offset],
            );
        }
        let mut run_end = 0;
        for run in &runs {
            assert!(run.len > 0);
            assert!(display.is_char_boundary(run_end));
            run_end += run.len;
            if !display.is_empty() {
                assert!(display.is_char_boundary(run_end));
            }
        }
        // Empty display is shaped using the editor's one-space placeholder.
        assert_eq!(run_end, display.len().max(1));
        RenderedCell {
            content,
            cell_range,
            decoration,
            active_ranges,
            raw_display,
            display_range,
            display,
            display_to_buffer,
            buffer_to_display,
            prefix_collapsed,
            runs,
        }
    }

    fn run_at(cell: &RenderedCell, byte: usize) -> &TextRun {
        let mut start = 0;
        for run in &cell.runs {
            if (start..start + run.len).contains(&byte) {
                return run;
            }
            start += run.len;
        }
        panic!("no text run at {byte} in {:?}", cell.display);
    }

    fn assert_visible_mapping(cell: &RenderedCell, word: &str) {
        let source = cell.content.find(word).unwrap();
        let display = cell.display.find(word).unwrap();
        for relative in 0..word.len() {
            assert_eq!(
                cell.display_to_buffer[display + relative],
                source + relative
            );
            assert_eq!(
                cell.buffer_to_display[source + relative],
                display + relative
            );
        }
    }

    fn assert_strike_only_for(cell: &RenderedCell, words: &[&str]) {
        let source_ranges = words
            .iter()
            .map(|word| {
                let start = cell.content.find(word).unwrap();
                start..start + word.len()
            })
            .collect::<Vec<_>>();
        for (display, source) in cell.display_to_buffer.iter().enumerate() {
            assert_eq!(
                run_at(cell, display).strikethrough.is_some(),
                source_ranges.iter().any(|range| range.contains(source)),
                "unexpected strike at display byte {display}, source byte {source}: {:?}",
                cell.display,
            );
        }
    }

    #[test]
    fn task_prefix_columns_round_up_the_full_checkbox_and_gap_width() {
        assert_eq!(task_prefix_columns(8.0, 19.2, 10.0), 4);
        assert_eq!(task_prefix_columns(8.0, 14.0, 10.0), 3);
        assert_eq!(task_prefix_columns(8.0, 14.01, 10.0), 4);
        assert_eq!(task_prefix_columns(8.0, 16.0, 0.0), 3);
        assert_eq!(task_prefix_columns(4.0, 19.2, 10.0), 8);
        assert_eq!(task_prefix_columns(8.0, 100.0, 10.0), 14);
    }

    #[test]
    fn task_prefix_columns_cover_checkbox_geometry_across_advances_and_scales() {
        for base_advance in 4..=12 {
            for font_size in [12.0, 16.0, 20.0, 28.0] {
                for scale in [0.75, 1.0, 1.25, 1.5, 2.0] {
                    let advance = base_advance as f32 * scale;
                    let size = 1.2 * font_size * scale;
                    let gap = 10.0 * scale;
                    let columns = task_prefix_columns(advance, size, gap);
                    assert!(columns >= 3);
                    assert!(columns as f32 * advance >= size + gap);
                    if columns > 3 {
                        assert!((columns - 1) as f32 * advance < size + gap);
                    }
                }
            }
        }
    }

    #[test]
    fn task_prefix_columns_use_caller_tracking_adjusted_advance_without_overlap() {
        for base_advance in 4..=12 {
            for tracking in [-4.0, -2.5, -0.5, 0.0] {
                for scale in [0.75, 1.0, 1.25, 1.5, 2.0] {
                    // The caller includes tracking and enforces the same minimum
                    // advance used by layout before handing us the measurement.
                    let advance = (base_advance as f32 * scale + tracking * scale).max(1.0);
                    let size = 1.2 * 16.0 * scale;
                    let gap = 10.0 * scale;
                    let columns = task_prefix_columns(advance, size, gap);
                    assert!(columns as f32 * advance >= size + gap);
                    assert_eq!(columns, ((size + gap) / advance).ceil().max(3.0) as usize);
                    if columns > 3 {
                        assert!((columns - 1) as f32 * advance < size + gap);
                    }
                }
            }
        }
        assert_eq!(task_prefix_columns(8.0, 19.2, 10.0), 4);
        assert_eq!(task_prefix_columns(8.0 - 2.0, 19.2, 10.0), 5);
    }

    #[test]
    fn task_prefix_columns_guard_invalid_measurements_and_subpixel_advances() {
        for advance in [-8.0, 0.0, 0.5, 1.0] {
            assert_eq!(task_prefix_columns(advance, 19.2, 10.0), 30);
        }
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(task_prefix_columns(invalid, 19.2, 10.0), 3);
            assert_eq!(task_prefix_columns(8.0, invalid, 10.0), 3);
            assert_eq!(task_prefix_columns(8.0, 19.2, invalid), 3);
        }
        assert_eq!(task_prefix_columns(8.0, f32::MAX, f32::MAX), 3);
        assert_eq!(task_prefix_columns(8.0, 0.0, 0.0), 3);
    }

    #[test]
    fn narrow_prefixes_reveal_source_instead_of_painting_across_cells() {
        let source = "123456789. item<br>- [x] done<br>  - [ ] child<br>tail";
        let cell = render_cell(source, &[], None);
        for zoom in [0.75, 1.0, 1.5, 2.0] {
            for tracking in [-1.0, 0.0, 2.0] {
                for header in [false, true] {
                    let ranges = helpers::hard_line_byte_ranges(&cell.display);
                    let lines = ranges
                        .iter()
                        .map(|range| {
                            let runs =
                                helpers::extract_runs_for_range(&cell.runs, range.start, range.end);
                            measured_line(
                                &cell.display[range.clone()],
                                &runs,
                                [7.25, 10.0, 18.0],
                                zoom,
                                tracking,
                                header,
                            )
                        })
                        .collect::<Vec<_>>();
                    for width in [8.0, 24.0, 40.0, 100.0, 300.0] {
                        let mut active = vec![source.len()..source.len()];
                        super::expand_narrow_prefixes(
                            source,
                            &cell.decoration,
                            &mut active,
                            &cell.buffer_to_display,
                            &lines,
                            &ranges,
                            width,
                        );
                        assert_eq!(active[0], source.len()..source.len());
                        let (display, _, b2d, _) = helpers::build_display_text(
                            source,
                            &cell.decoration,
                            &active,
                            false,
                            None,
                            0,
                            3,
                            3,
                            2,
                            0,
                            1.0,
                            1.0,
                            1.0,
                        );
                        let final_ranges = helpers::hard_line_byte_ranges(&display);
                        for (index, fragment) in cell
                            .decoration
                            .table_cell_fragments
                            .iter()
                            .enumerate()
                            .take(3)
                        {
                            let prefix = super::display_prefix_end(
                                fragment,
                                &cell.buffer_to_display,
                                &ranges[index],
                            );
                            let measured =
                                f32::from(lines[index].unwrapped_layout.x_for_index(prefix));
                            let folded = helpers::table_cell_fragment_prefix_is_folded(
                                source, fragment, &active,
                            );
                            assert_eq!(folded, measured < width);
                            if !folded {
                                assert_eq!(
                                    &display[final_ranges[index].clone()],
                                    &source[fragment.range.clone()]
                                );
                            }
                        }
                        for word in ["item", "done", "child", "tail"] {
                            let start = source.find(word).unwrap();
                            assert_eq!(&display[b2d[start]..b2d[start] + word.len()], word);
                        }
                    }
                }
            }
        }
    }

    fn assert_hanging_geometry(line: &WrappedLine, hanging: HangingWrap, width: f32, height: f32) {
        let text = line.text.as_ref();
        let segments = super::segment_layouts(line);
        let origin = point(px(47.0), px(83.0));
        let selection = hanging.selection_bounds(line, text, 0..text.len(), origin, height);
        assert_eq!(selection.len(), segments.len());
        for (row, ((range, paint_layout), selected)) in segments.iter().zip(&selection).enumerate()
        {
            let indent = hanging.row_indent(row);
            let right = indent + f32::from(paint_layout.width);
            assert!(
                right <= width + 0.02,
                "row {row}: {right} > {width}, {text:?}"
            );
            assert!((f32::from(selected.origin.x - origin.x) - indent).abs() < 0.02);
            assert!((f32::from(selected.size.width - paint_layout.width)).abs() < 0.02);
            assert_eq!(selected.origin.y - origin.y, line.row_top(row, px(height)));
            assert_eq!(selected.size.height, line.row_height(row, px(height)));
            let center_y = line.row_top(row, px(height)) + line.row_height(row, px(height)) * 0.5;
            let padding_hit = hanging.hit_x(line, indent - 15.0, center_y, height);
            let padding_index =
                line.closest_index_for_position(point(padding_hit, center_y), px(height));
            assert_eq!(padding_index.unwrap_or_else(|index| index), range.start);
            for (local, _) in text[range.clone()].char_indices() {
                let index = range.start + local;
                let position = hanging.position(
                    line,
                    text,
                    index,
                    height,
                    Some(super::WrapBoundaryAffinityKind::Downstream),
                );
                assert_eq!(position.y, line.row_top(row, px(height)));
                let paint_x = paint_layout.x_for_index(local) + px(indent);
                assert!((f32::from(position.x - paint_x)).abs() < 0.02);
                let hit_x = hanging.hit_x(line, f32::from(position.x), center_y, height);
                let hit = line.closest_index_for_position(point(hit_x, center_y), px(height));
                assert_eq!(hit.unwrap_or_else(|index| index), index);
            }
            let end = hanging.position(
                line,
                text,
                range.end,
                height,
                Some(super::WrapBoundaryAffinityKind::Upstream),
            );
            assert_eq!(end.y, line.row_top(row, px(height)));
            assert!((f32::from(end.x) - right).abs() < 0.02);
        }
    }

    #[test]
    fn hanging_wrap_measured_fonts_tracking_zoom_header_and_every_break_fragment() {
        let body = "alpha beta 界🙂 gamma delta epsilon zeta eta theta iota kappa";
        let source = format!(
            "1. {body}<br>12. {body}<BR />12345. {body}<br/>- {body}<br>  - [x] {body}<br>    - [ ] {body}<br>plain {body}"
        );
        for expanded in [false, true] {
            let folded = render_cell(&source, &[], None);
            let carets = if expanded {
                folded
                    .decoration
                    .table_cell_fragments
                    .iter()
                    .filter(|fragment| fragment.prefix_len > 0)
                    .map(|fragment| fragment.range.start + fragment.prefix_len - 1)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let cell = render_cell(&source, &carets, None);
            let original_b2d = cell.buffer_to_display.clone();
            let original_d2b = cell.display_to_buffer.clone();
            for metrics in [[5.5, 9.25, 20.0], [12.5, 6.75, 26.0]] {
                for zoom in [0.75, 1.0, 1.75, 2.5] {
                    for tracking in [-0.5, 0.0, 1.75] {
                        for header in [false, true] {
                            let ranges = helpers::hard_line_byte_ranges(&cell.display);
                            assert_eq!(ranges.len(), cell.decoration.table_cell_fragments.len());
                            for (range, fragment) in
                                ranges.iter().zip(&cell.decoration.table_cell_fragments)
                            {
                                let runs = helpers::extract_runs_for_range(
                                    &cell.runs,
                                    range.start,
                                    range.end,
                                );
                                let mut line = measured_line(
                                    &cell.display[range.clone()],
                                    &runs,
                                    metrics,
                                    zoom,
                                    tracking,
                                    header,
                                );
                                let prefix = super::display_prefix_end(
                                    fragment,
                                    &cell.buffer_to_display,
                                    range,
                                );
                                let measured = f32::from(line.unwrapped_layout.x_for_index(prefix));
                                let unchanged = line.clone();
                                let width = 290.0 * zoom;
                                let hanging =
                                    super::wrap_fragment(&mut line, prefix, width, |_, _| {});
                                assert!((hanging.indent - measured).abs() < 0.02);
                                assert!(!line.wrap_boundaries().is_empty());
                                assert!(
                                    unchanged.wrap_boundaries().is_empty(),
                                    "cached shape mutated"
                                );
                                assert_eq!(
                                    line.unwrapped_layout.width,
                                    unchanged.unwrapped_layout.width
                                );
                                let height = 24.0 * zoom;
                                let heights = (0..=line.wrap_boundaries().len())
                                    .map(|row| px(height * if row % 3 == 0 { 3.0 } else { 1.0 }))
                                    .collect();
                                line.set_row_heights(heights);
                                assert_hanging_geometry(&line, hanging, width, height);
                                if prefix > 0 {
                                    assert_eq!(
                                        hanging.position(&line, &line.text, prefix, height, None).x,
                                        px(measured)
                                    );
                                }
                            }
                        }
                    }
                }
            }
            assert_eq!(cell.buffer_to_display, original_b2d);
            assert_eq!(cell.display_to_buffer, original_d2b);
        }
    }

    #[test]
    fn hanging_wrap_first_row_reserves_prefix_once_and_preserves_upstream_downstream() {
        let mut cell = render_cell("- abcdefghijklmnop", &[], None);
        for run in &mut cell.runs {
            run.font.weight = FontWeight::NORMAL;
        }
        let mut line = measured_line(
            &cell.display,
            &cell.runs,
            [7.0, 10.0, 20.0],
            1.0,
            0.0,
            false,
        );
        let prefix = cell.buffer_to_display[2];
        let hanging = super::wrap_fragment(&mut line, prefix, 60.0, |_, _| {});
        assert_eq!(hanging.indent, 14.0);
        assert_eq!(
            helpers::wrap_boundary_byte_indices(&line, &line.text),
            vec![6, 10, 14]
        );
        line.set_row_heights(vec![px(72.0), px(24.0), px(60.0), px(24.0)]);
        assert_eq!(
            hanging.position(&line, &line.text, 6, 24.0, None),
            point(px(54.0), px(0.0))
        );
        assert_eq!(
            hanging.position(
                &line,
                &line.text,
                6,
                24.0,
                Some(super::WrapBoundaryAffinityKind::Downstream)
            ),
            point(px(14.0), px(72.0))
        );
        assert_hanging_geometry(&line, hanging, 60.0, 24.0);
    }

    #[test]
    fn hanging_wrap_narrow_lane_falls_back_without_wide_glyph_overflow() {
        for width in [10.0, 20.0, 28.0, 40.0] {
            let mut cell = render_cell("- 界界界界", &[], None);
            for run in &mut cell.runs {
                run.font.weight = FontWeight::NORMAL;
            }
            let mut line = measured_line(
                &cell.display,
                &cell.runs,
                [8.0, 20.0, 20.0],
                1.0,
                0.0,
                false,
            );
            let hanging =
                super::wrap_fragment(&mut line, cell.buffer_to_display[2], width, |_, _| {});
            assert_eq!(hanging.indent, if width < 36.0 { 0.0 } else { 16.0 });
            if width >= 20.0 {
                assert_hanging_geometry(&line, hanging, width, 24.0);
            }
        }
    }

    #[test]
    fn hanging_wrap_tall_math_after_break_uses_atomic_width_and_real_row_heights() {
        use super::RenderImage;
        use std::collections::HashMap;
        let source = "1. first words before $x$ after words<br>  - [x] next words before $y$ after words<br>tail";
        let mut cell = render_cell(source, &[], None);
        let cache = [("x", 90, 216), ("y", 120, 288)]
            .into_iter()
            .map(|(formula, width, height)| {
                (
                    helpers::inline_math_cache_key(formula),
                    Some(Arc::new(RenderImage::new([image::Frame::new(
                        image::RgbaImage::new(width, height),
                    )]))),
                )
            })
            .collect::<HashMap<_, _>>();
        helpers::add_inline_math_display_replacements(&mut cell.decoration, source, &cache, 8.0);
        let (display, _, b2d, _) = helpers::build_display_text(
            source,
            &cell.decoration,
            &[],
            false,
            None,
            0,
            3,
            3,
            2,
            0,
            1.0,
            1.0,
            1.0,
        );
        let ranges = helpers::hard_line_byte_ranges(&display);
        let mut hard_top = 0.0;
        for (index, range) in ranges.iter().enumerate() {
            let run = TextRun {
                len: range.len(),
                font: font("Test Mono"),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let mut line = measured_line(
                &display[range.clone()],
                &[run],
                [8.0, 8.0, 16.0],
                1.0,
                1.25,
                false,
            );
            helpers::keep_inline_math_placeholders_unwrapped(
                &mut line,
                &cell.decoration,
                source,
                &b2d,
                &[],
                &cache,
                range.start,
            );
            let prefix = super::display_prefix_end(
                &cell.decoration.table_cell_fragments[index],
                &b2d,
                range,
            );
            let hanging = super::wrap_fragment(&mut line, prefix, 150.0, |body, prefix| {
                helpers::keep_inline_math_placeholders_unwrapped(
                    body,
                    &cell.decoration,
                    source,
                    &b2d,
                    &[],
                    &cache,
                    range.start + prefix,
                )
            });
            helpers::apply_inline_math_row_heights(
                &mut line,
                &cell.decoration,
                source,
                &b2d,
                &[],
                &cache,
                24.0,
                range.start,
            );
            assert_hanging_geometry(&line, hanging, 150.0, 24.0);
            for formula in &cell.decoration.inline_math_spans {
                let Some(mapped) =
                    helpers::inline_math_display_range(&cell.decoration, formula, &b2d)
                else {
                    continue;
                };
                if mapped.start < range.start || mapped.end > range.end {
                    continue;
                }
                let start = mapped.start - range.start;
                let end = mapped.end - range.start;
                assert!(
                    !helpers::wrap_boundary_byte_indices(&line, &line.text)
                        .iter()
                        .any(|&boundary| start < boundary && boundary < end)
                );
                let position = hanging.position(
                    &line,
                    &line.text,
                    start,
                    24.0,
                    Some(super::WrapBoundaryAffinityKind::Downstream),
                );
                let formula_row = line.row_for_y(position.y, px(24.0));
                let height = f32::from(line.row_height(formula_row, px(24.0)));
                let expected_height = if index == 0 { 72.0 } else { 96.0 };
                assert_eq!(height, expected_height);
                assert!(hard_top + f32::from(position.y) >= hard_top);
                let highlight = hanging.selection_bounds(
                    &line,
                    &line.text,
                    start..end,
                    point(px(0.0), px(hard_top)),
                    24.0,
                );
                assert_eq!(highlight.len(), 1);
                assert_eq!(highlight[0].origin.x, position.x);
                assert_eq!(highlight[0].origin.y, px(hard_top) + position.y);
                assert_eq!(highlight[0].size.height, px(expected_height));
            }
            hard_top += f32::from(line.size(px(24.0)).height);
        }
        assert!(hard_top > 72.0 + 96.0 + 24.0);
    }

    #[test]
    fn first_fragment_indent_strips_one_pipe_padding_space_and_preserves_real_source() {
        for (raw, expected) in [
            ("- item ", "- item "),
            (" - item ", "- item "),
            ("  - item ", " - item "),
            ("   - item ", "  - item "),
            ("       12. item \t ", "      12. item \t "),
            (" \t  - [x] item ", "\t  - [x] item "),
            ("\t- item", "\t- item"),
            ("     - [ ]", "    - [ ]"),
            ("     12.", "    12."),
            ("     -", "    -"),
        ] {
            for row in [
                format!("|{raw}| neighbor |"),
                format!("  > | neighbor |{raw}|"),
            ] {
                let col = usize::from(row.starts_with("  >"));
                let context = if col == 0 { "" } else { "  > " };
                let text = format!("{context}| A | B |\n{context}| --- | --- |\n{row}");
                let rows = markdown::parse_decorations(&text);
                let info = rows[2].table_info.as_ref().unwrap();
                let semantic = info.clone();
                let (content, range) = content_with_range(info, &row, col);
                assert_eq!(content, expected, "{row:?}");
                assert_eq!(&row[range.clone()], expected);
                assert_eq!(
                    range.start,
                    row.find(raw).unwrap() + raw.len() - expected.len()
                );
                assert_eq!(
                    range.end,
                    info.cell_ranges[col].end + expected.len()
                        - expected.trim_end_matches([' ', '\t']).len()
                );
                assert_eq!(info, &semantic, "semantic metadata must stay trimmed");
                assert!(row.is_char_boundary(range.start) && row.is_char_boundary(range.end));
            }
        }
        for row in [
            "|    plain | neighbor |",
            "|    `- opaque` | neighbor |",
            "|    \\- literal | neighbor |",
        ] {
            let text = format!("| A | B |\n| --- | --- |\n{row}");
            let rows = markdown::parse_decorations(&text);
            let info = rows[2].table_info.as_ref().unwrap();
            assert_eq!(
                content_with_range(info, row, 0).1.start,
                2,
                "ordinary cells must own newly typed leading spaces too",
            );
        }
        let row = "  - item | neighbor";
        let rows = markdown::parse_decorations("| A | B |\n| --- | --- |");
        let mut info = rows[0].table_info.as_ref().unwrap().clone();
        // Use actual semantic extraction even when block parsing gives a
        // pipe-less list-shaped row precedence over table detection.
        let parsed = markdown::parse_table_cells_with_ranges(row);
        info.cells = parsed.iter().map(|(cell, _)| cell.clone()).collect();
        info.cell_ranges = parsed.into_iter().map(|(_, range)| range).collect();
        assert_eq!(
            content_with_range(&info, row, 0).1.start,
            info.cell_ranges[0].start,
            "a pipe-less first column must not borrow row indentation"
        );
    }

    #[test]
    fn first_fragment_indent_render_pipeline_preserves_depth_inline_syntax_and_hanging_geometry() {
        for (marker, kind) in [
            ("- ", markdown::BlockType::UnorderedList),
            ("+ ", markdown::BlockType::UnorderedList),
            ("12. ", markdown::BlockType::OrderedList),
            ("12) ", markdown::BlockType::OrderedList),
            ("- [x] ", markdown::BlockType::TaskList(true)),
            ("- [ ] ", markdown::BlockType::TaskList(false)),
        ] {
            for indent in [0, 1, 2, 3, 4, 6, 8, 12] {
                let source = format!(
                    "{}{marker}**body** 界🙂 `code<br>literal`<br>  - [ ] child",
                    " ".repeat(indent)
                );
                let cell = render_cell(&source, &[], None);
                assert_eq!(cell.content, source);
                assert_eq!(cell.decoration.block_type, kind, "{source:?}");
                let fragments = &cell.decoration.table_cell_fragments;
                assert_eq!(fragments.len(), 2, "protected breaks must remain opaque");
                assert_eq!(fragments[0].range.start, 0);
                assert_eq!(fragments[0].prefix_len, indent + marker.len());
                assert_eq!(fragments[0].block_type, kind);
                assert_eq!(
                    helpers::list_marker_body_indent(&cell.content[fragments[0].range.clone()]),
                    indent
                );
                assert_eq!(
                    helpers::list_marker_body_indent(&cell.content[fragments[1].range.clone()]),
                    2
                );
                let body = cell.display.find("body").unwrap();
                let expected_columns = if matches!(kind, markdown::BlockType::TaskList(_)) {
                    indent / 2 * 3 + 3
                } else {
                    indent + marker.len()
                };
                assert_eq!(&cell.display[..body], " ".repeat(expected_columns));
                assert!(run_at(&cell, body).font.weight >= FontWeight::BOLD);
                assert_visible_mapping(&cell, "body");
                assert_visible_mapping(&cell, "界🙂");
                assert_visible_mapping(&cell, "child");
                let range = &helpers::hard_line_byte_ranges(&cell.display)[0];
                let runs = helpers::extract_runs_for_range(&cell.runs, range.start, range.end);
                let mut line = measured_line(
                    &cell.display[range.clone()],
                    &runs,
                    [8.0, 9.0, 20.0],
                    1.0,
                    0.5,
                    false,
                );
                let prefix =
                    super::display_prefix_end(&fragments[0], &cell.buffer_to_display, range);
                let measured = f32::from(line.unwrapped_layout.x_for_index(prefix));
                let hanging = super::wrap_fragment(&mut line, prefix, 320.0, |_, _| {});
                assert_eq!(hanging.indent, measured);
                assert_hanging_geometry(&line, hanging, 320.0, 24.0);
                let expanded = render_cell(&source, &[indent + 1], None);
                assert_eq!(expanded.content, source);
                assert_eq!(
                    expanded.decoration.table_cell_fragments[0].prefix_len,
                    indent + marker.len()
                );
                assert!(
                    expanded
                        .display
                        .starts_with(&format!("{}{marker}", " ".repeat(indent)))
                );
                assert_visible_mapping(&expanded, "body");
            }
        }
    }

    fn owned_first_fragment(
        buffer: &Buffer,
        cursor: crate::cursor::Cursor,
    ) -> (Range<usize>, markdown::LineDecoration) {
        let rows = markdown::parse_decorations(buffer.text());
        let cell = super::super::table_cell_edit::cell_range_for_selection(
            buffer,
            &rows,
            true,
            &cursor.selection_range(),
        )
        .unwrap();
        let (line, _) = buffer.offset_to_line_col(cell.start);
        let info = rows[line].table_info.as_ref().unwrap();
        let (source, local) = content_with_range(info, buffer.line_content(line), 0);
        assert_eq!(
            cell,
            buffer.line_start(line) + local.start..buffer.line_start(line) + local.end
        );
        assert_eq!(buffer.text_for_range(cell.clone()), source);
        let decoration = super::super::table_cell_list_edit::cell_decoration(buffer, &rows, &cell);
        (cell, decoration)
    }

    #[test]
    fn first_fragment_indent_repeated_tab_shift_tab_and_enter_use_production_owned_ranges() {
        use super::super::table_cell_list_edit as lists;
        use crate::cursor::Cursor;
        for (marker, step, continuation) in [
            ("- ", 2, "- "),
            ("12. ", 3, "13. "),
            ("- [x] ", 2, "- [ ] "),
        ] {
            let original = format!("| A | B |\n| --- | --- |\n| {marker}body| untouched |");
            let mut buffer = Buffer::from_text(&original);
            let at = buffer.text().find("body").unwrap() + 4;
            let mut cursors = vec![(
                Cursor {
                    offset: at,
                    anchor: at,
                },
                true,
            )];
            for depth in 0..=4 {
                let (cell, decoration) = owned_first_fragment(&buffer, cursors[0].0);
                let fragment = &decoration.table_cell_fragments[0];
                assert_eq!(
                    helpers::list_marker_body_indent(buffer.text_for_range(cell.clone())),
                    step * depth
                );
                assert_eq!(fragment.prefix_len, step * depth + marker.len());
                if depth < 4 {
                    let at = cursors[0].0.offset - cell.start;
                    let edits = lists::cell_indent(
                        buffer.text_for_range(cell.clone()),
                        &decoration,
                        at..at,
                        true,
                    )
                    .unwrap();
                    lists::apply_prefix_edits(
                        &mut buffer,
                        &mut cursors,
                        edits
                            .into_iter()
                            .map(|edit| edit.translated(cell.start))
                            .collect(),
                    );
                }
            }
            for depth in (0..4).rev() {
                let (cell, decoration) = owned_first_fragment(&buffer, cursors[0].0);
                let at = cursors[0].0.offset - cell.start;
                let edits = lists::cell_indent(
                    buffer.text_for_range(cell.clone()),
                    &decoration,
                    at..at,
                    false,
                )
                .unwrap();
                lists::apply_prefix_edits(
                    &mut buffer,
                    &mut cursors,
                    edits
                        .into_iter()
                        .map(|edit| edit.translated(cell.start))
                        .collect(),
                );
                let (cell, _) = owned_first_fragment(&buffer, cursors[0].0);
                assert_eq!(
                    helpers::list_marker_body_indent(buffer.text_for_range(cell)),
                    step * depth
                );
            }
            assert_eq!(
                buffer.text(),
                original,
                "ShiftTab must never delete formatting padding"
            );
            for _ in 0..3 {
                let (cell, decoration) = owned_first_fragment(&buffer, cursors[0].0);
                let at = cursors[0].0.offset - cell.start;
                let edits = lists::cell_indent(
                    buffer.text_for_range(cell.clone()),
                    &decoration,
                    at..at,
                    true,
                )
                .unwrap();
                lists::apply_prefix_edits(
                    &mut buffer,
                    &mut cursors,
                    edits
                        .into_iter()
                        .map(|edit| edit.translated(cell.start))
                        .collect(),
                );
            }
            let (cell, decoration) = owned_first_fragment(&buffer, cursors[0].0);
            let at = cursors[0].0.offset - cell.start;
            let edit = lists::cell_enter(
                buffer.text_for_range(cell.clone()),
                &decoration,
                at..at,
                at,
                false,
            )
            .translated(cell.start);
            assert_eq!(
                edit.text,
                format!("<br>{}{continuation}", " ".repeat(step * 3))
            );
            cursors[0].0 = lists::apply(&mut buffer, &[(edit, true)]).unwrap()[0].0;
            for depth in (0..=3).rev() {
                let (cell, decoration) = owned_first_fragment(&buffer, cursors[0].0);
                let fragment = decoration.table_cell_fragments.last().unwrap();
                assert_eq!(
                    helpers::list_marker_body_indent(
                        &buffer.text_for_range(cell.clone())[fragment.range.clone()]
                    ),
                    step * depth
                );
                if depth > 0 {
                    let at = cursors[0].0.offset - cell.start;
                    let edit = lists::cell_enter(
                        buffer.text_for_range(cell.clone()),
                        &decoration,
                        at..at,
                        at,
                        false,
                    )
                    .translated(cell.start);
                    cursors[0].0 = lists::apply(&mut buffer, &[(edit, true)]).unwrap()[0].0;
                }
            }
            assert!(buffer.text().ends_with("| untouched |"));
        }
    }

    #[test]
    fn first_fragment_indent_empty_enter_outdents_first_fragment_without_eating_padding() {
        use super::super::table_cell_list_edit as lists;
        use crate::cursor::Cursor;
        for (marker, step) in [("- ", 2), ("12. ", 3), ("- [ ] ", 2)] {
            let mut buffer = Buffer::from_text(&format!(
                "| A | B |\n| --- | --- |\n| {}{marker}| untouched |",
                " ".repeat(step * 3)
            ));
            let at = buffer.text().find("| untouched").unwrap();
            let mut cursor = Cursor {
                offset: at,
                anchor: at,
            };
            for depth in (0..=3).rev() {
                let (cell, decoration) = owned_first_fragment(&buffer, cursor);
                assert_eq!(
                    helpers::list_marker_body_indent(buffer.text_for_range(cell.clone())),
                    step * depth
                );
                assert_eq!(
                    decoration.table_cell_fragments[0].prefix_len,
                    step * depth + marker.len()
                );
                let at = cursor.offset - cell.start;
                let edit = lists::cell_enter(
                    buffer.text_for_range(cell.clone()),
                    &decoration,
                    at..at,
                    at,
                    false,
                )
                .translated(cell.start);
                cursor = lists::apply(&mut buffer, &[(edit, true)]).unwrap()[0].0;
            }
            assert_eq!(buffer.line_content(2), "| <br>| untouched |");
        }
    }

    #[test]
    fn first_fragment_indent_fallback_layout_uses_the_same_source_depth_and_maps() {
        for (marker, indent, display_indent) in [("- ", 6, 6), ("12. ", 6, 6), ("- [x] ", 4, 6)] {
            let mut widths = Vec::new();
            for depth in [0, indent] {
                let source = format!(
                    "| A | B |\n| --- | --- |\n| {}{marker}body| {} |",
                    " ".repeat(depth),
                    "other".repeat(30)
                );
                let buffer = Buffer::from_text(&source);
                let rows = markdown::parse_decorations(&source);
                let layout = helpers::compute_table_layout(
                    &buffer, &rows, 0, 3, 1000.0, 8.0, 4.0, 8.0, 24.0, 16.0,
                );
                let cell = &layout.cells[1][0];
                let info = rows[2].table_info.as_ref().unwrap();
                let (content, range) = content_with_range(info, buffer.line_content(2), 0);
                assert_eq!(cell.cell_range, range);
                assert_eq!(cell.buffer_to_display.len(), content.len() + 1);
                let prefix = helpers::table_cell_decoration(&rows[2], &content, &range)
                    .table_cell_fragments[0]
                    .prefix_len;
                let body = cell.buffer_to_display[prefix];
                assert_eq!(&cell.display_text[body..], "body");
                assert_eq!(cell.display_to_buffer[body], prefix);
                widths.push(cell.width);
            }
            assert_eq!(widths[1] - widths[0], display_indent as f32 * 8.0);
        }
    }

    #[test]
    fn first_fragment_indent_front_matter_synthetic_cells_remain_unchanged() {
        let source = "---\ntitle: '  - item  '\nitems: [a, b]\n---";
        let buffer = Buffer::from_text(source);
        let rows = markdown::parse_decorations(source);
        let mut checked = 0;
        for (line, row) in rows.iter().enumerate() {
            let Some(info) = row.table_info.as_ref().filter(|info| info.is_front_matter) else {
                continue;
            };
            for col in 0..info.cells.len() {
                assert_eq!(
                    content_with_range(info, buffer.line_content(line), col),
                    (info.cells[col].clone(), info.cell_ranges[col].clone())
                );
                checked += 1;
            }
            let literal = "    - **literal**";
            assert_eq!(
                helpers::table_cell_decoration(row, literal, &(0..literal.len())).block_type,
                markdown::BlockType::CodeBlock,
                "synthetic cells must not get the new list-depth override"
            );
        }
        assert!(checked > 0);
    }

    #[test]
    fn render_cell_keeps_only_actual_following_whitespace() {
        for marker in ["-", "- ", "-  \t", "1.", "1. ", "- [ ]", "- [ ] "] {
            let row = format!("| {marker}| unchanged |\t ");
            let text = format!("| Input | Other |\n| --- | --- |\n{row}");
            let decorations = markdown::parse_decorations(&text);
            let info = decorations[2].table_info.as_ref().unwrap();
            let semantic = markdown::parse_table_cells_with_ranges(&row);
            let (content, range) = content_with_range(info, &row, 0);
            assert_eq!(content, marker);
            assert_eq!(&row[range.clone()], marker);
            assert_eq!(range.start, info.cell_ranges[0].start);
            assert_eq!(row.as_bytes()[range.end], b'|');
            assert_eq!(info.cells[0], semantic[0].0);
            assert_eq!(info.cell_ranges[0], semantic[0].1);
            assert_eq!(content_with_range(info, &row, 1).0, "unchanged ");
        }
    }

    #[test]
    fn render_cell_keeps_pipe_free_row_tail_but_not_line_endings() {
        for row in ["| left | 1. \t  ", "left | 1. \t  ", "| left | 1. \t  \r"] {
            let text = format!("| A | B |\n| --- | --- |\n{row}");
            let decorations = markdown::parse_decorations(&text);
            let info = decorations[2].table_info.as_ref().unwrap();
            let (content, range) = content_with_range(info, row, 1);
            assert_eq!(content, "1. \t  ");
            assert_eq!(&row[range.clone()], content);
            assert_eq!(range.end, row.trim_end_matches('\r').len());
        }
    }

    #[test]
    fn render_cell_utf8_ranges_keep_carets_in_trailing_whitespace() {
        let text = "  > | 名前 | 値 |\n  > | --- | --- |\n  > | 前🙂 | 中🙂 \t  |\t ";
        let buffer = Buffer::from_text(text);
        let decorations = markdown::parse_decorations(text);
        let row = buffer.line_content(2);
        let info = decorations[2].table_info.as_ref().unwrap();
        let (content, range) = content_with_range(info, row, 1);
        assert_eq!(content, "中🙂 \t  ");
        assert_eq!(range.start, row.find("中🙂").unwrap());
        assert_eq!(range.end, row.rfind('|').unwrap());
        assert_eq!(&row[range.clone()], content);
        let line_start = buffer.line_start(2);
        let caret = line_start + info.cell_ranges[1].end + 1;
        assert!(
            helpers::active_table_cell_ranges(line_start, &info.cell_ranges[1], &[caret], None,)
                .is_empty()
        );
        let relative = caret - line_start - range.start;
        assert_eq!(
            helpers::active_table_cell_ranges(
                line_start,
                &range,
                &[caret, line_start + range.end],
                None,
            ),
            vec![relative..relative, content.len()..content.len()],
        );
        let marked = caret..line_start + range.end;
        assert_eq!(
            helpers::active_table_cell_ranges(line_start, &range, &[], Some(&marked)),
            vec![relative..content.len()],
        );

        let layout = helpers::compute_table_layout(
            &buffer,
            &decorations,
            0,
            3,
            400.0,
            8.0,
            4.0,
            8.0,
            20.0,
            16.0,
        );
        let cell = &layout.cells[1][1];
        assert_eq!(cell.cell_range, range);
        assert_eq!(cell.buffer_to_display.len(), content.len() + 1);
        assert!(row.is_char_boundary(cell.cell_range.start));
        assert!(row.is_char_boundary(cell.cell_range.end));
    }

    #[test]
    fn render_cell_respects_escaped_code_and_wiki_pipes() {
        let row = "| a\\|b `c|d` [[note|名]] \t | - [ ]  |";
        let text = format!("| A | B |\n| --- | --- |\n{row}");
        let decorations = markdown::parse_decorations(&text);
        let info = decorations[2].table_info.as_ref().unwrap();
        assert_eq!(info.cells.len(), 2);
        for (col, expected) in ["a\\|b `c|d` [[note|名]] \t ", "- [ ]  "]
            .iter()
            .enumerate()
        {
            let (content, range) = content_with_range(info, row, col);
            assert_eq!(&content, expected);
            assert_eq!(&row[range], *expected);
        }
    }

    #[test]
    fn render_cell_empty_padding_is_owned_from_the_first_editable_byte() {
        let row = "| \t | value |";
        let text = format!("| A | B |\n| --- | --- |\n{row}");
        let decorations = markdown::parse_decorations(&text);
        let info = decorations[2].table_info.as_ref().unwrap();
        assert_eq!(content_with_range(info, row, 0), ("\t ".to_string(), 2..4));
        assert_eq!(content_with_range(info, row, 9), (String::new(), 0..0));
    }

    #[test]
    fn render_cell_falls_back_for_synthetic_stale_or_invalid_metadata() {
        let row = "| 中🙂 |";
        let text = format!("| A |\n| --- |\n{row}");
        let decorations = markdown::parse_decorations(&text);
        let info = decorations[2].table_info.as_ref().unwrap();
        let mut synthetic = info.clone();
        synthetic.is_front_matter = true;
        assert_eq!(
            content_with_range(&synthetic, row, 0),
            (info.cells[0].clone(), info.cell_ranges[0].clone())
        );
        for source in ["", "| other |"] {
            assert_eq!(
                content_with_range(info, source, 0),
                (info.cells[0].clone(), info.cell_ranges[0].clone())
            );
        }
        for range in [3..4, 99..100, std::ops::Range { start: 5, end: 2 }] {
            let mut invalid = info.clone();
            invalid.cell_ranges[0] = range.clone();
            assert_eq!(
                content_with_range(&invalid, row, 0),
                (info.cells[0].clone(), range)
            );
        }
        let mut missing = info.clone();
        missing.cell_ranges.clear();
        assert_eq!(
            content_with_range(&missing, row, 0),
            (info.cells[0].clone(), 0..0)
        );
    }

    #[test]
    fn render_pipeline_trailing_complete_spaces_keep_padding_and_end_carets() {
        use markdown::BlockType::{OrderedList, TaskList, UnorderedList};

        for (marker, kind, reserved) in [
            ("- ", UnorderedList, 2),
            ("1. ", OrderedList, 3),
            ("- [ ] ", TaskList(false), 3),
            ("- [x] ", TaskList(true), 3),
        ] {
            for lead in ["", "lead<br>"] {
                let source = format!("{lead}{marker}");
                for carets in [vec![], vec![source.len()]] {
                    let cell = render_cell(&source, &carets, None);
                    assert_eq!(cell.content, source);
                    let fragments = &cell.decoration.table_cell_fragments;
                    let fragment = fragments.last().unwrap();
                    assert_eq!(fragment.block_type, kind);
                    assert_eq!(fragment.prefix_len, marker.len());
                    assert!(helpers::table_cell_fragment_prefix_is_folded(
                        &cell.content,
                        fragment,
                        &cell.active_ranges,
                    ));
                    let prefix = " ".repeat(reserved);
                    let expected = if lead.is_empty() {
                        prefix
                    } else {
                        format!("lead\n{prefix}")
                    };
                    assert_eq!(cell.raw_display, expected);
                    assert_eq!(cell.display, expected);
                    assert_eq!(cell.display_range, 0..cell.raw_display.len());
                    assert_eq!(cell.prefix_collapsed, lead.is_empty());
                    if !carets.is_empty() {
                        assert_eq!(cell.active_ranges, vec![source.len()..source.len()]);
                        assert_eq!(cell.buffer_to_display[source.len()], cell.display.len());
                    }
                    for byte in cell.display.len() - reserved..cell.display.len() {
                        assert_eq!(run_at(&cell, byte).font.family.as_ref(), "Test Mono");
                        assert!(run_at(&cell, byte).strikethrough.is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn render_pipeline_incomplete_markers_stay_literal_unfocused_and_at_fragment_end() {
        for marker in ["-", "1.", "- [x]", "- [ ]"] {
            for (source, expected, fragment_index, caret) in [
                (marker.to_string(), marker.to_string(), 0, marker.len()),
                (
                    format!("{marker}<br>tail"),
                    format!("{marker}\ntail"),
                    0,
                    marker.len(),
                ),
                (
                    format!("lead<br>{marker}"),
                    format!("lead\n{marker}"),
                    1,
                    8 + marker.len(),
                ),
            ] {
                for carets in [vec![], vec![caret]] {
                    let cell = render_cell(&source, &carets, None);
                    assert_eq!(
                        cell.display, expected,
                        "source: {source:?}, carets: {carets:?}"
                    );
                    let fragment = &cell.decoration.table_cell_fragments[fragment_index];
                    assert_eq!(fragment.prefix_len, 0);
                    assert_eq!(fragment.block_type, markdown::BlockType::Normal);
                    assert!(!helpers::table_cell_fragment_prefix_is_folded(
                        &cell.content,
                        fragment,
                        &cell.active_ranges,
                    ));
                    assert!(!cell.prefix_collapsed);
                    assert_visible_mapping(&cell, marker);
                    assert_strike_only_for(&cell, &[]);
                }
            }
        }
    }

    #[test]
    fn render_pipeline_completion_is_independent_for_each_fragment() {
        let source = "1.<br>1. <br>- [ ]<br>- [ ] <br>- [x]<br>- [x] ";
        let cell = render_cell(source, &[], None);
        assert_eq!(cell.display, "1.\n   \n- [ ]\n   \n- [x]\n   ");
        assert_eq!(
            cell.decoration
                .table_cell_fragments
                .iter()
                .map(|fragment| fragment.prefix_len)
                .collect::<Vec<_>>(),
            vec![0, 3, 0, 6, 0, 6],
        );
        assert_eq!(cell.display_range.end, cell.raw_display.len());
        assert_strike_only_for(&cell, &[]);
    }

    #[test]
    fn render_pipeline_task_selection_endpoints_and_ime_expand_only_the_target_prefix() {
        let source = "- [x] done<br>  - [ ] next";
        let folded = render_cell(source, &[], None);
        assert_eq!(folded.display, "   done\n      next");
        for (index, marker, body) in [(0, "- [x]", "done"), (1, "- [ ]", "next")] {
            let fragment = &folded.decoration.table_cell_fragments[index];
            let start = fragment.range.start;
            let leading = if index == 0 { 0 } else { 2 };
            let marker_start = start + leading;
            let prefix_end = start + fragment.prefix_len;
            let expected = if index == 0 {
                "- [x] done\n      next"
            } else {
                "   done\n  - [ ] next"
            };
            // Selection endpoints inside the checkbox activate its syntax.
            let selected = render_cell(source, &[marker_start + 2, marker_start + 5], None);
            // A composition range can span the prefix and continue into the body.
            let composing = render_cell(source, &[], Some(marker_start + 3..prefix_end + 2));
            let caret = render_cell(source, &[marker_start + 3], None);
            for cell in [selected, composing, caret] {
                assert_eq!(cell.display, expected);
                assert!(cell.display.contains(marker));
                assert!(!helpers::table_cell_fragment_prefix_is_folded(
                    &cell.content,
                    &cell.decoration.table_cell_fragments[index],
                    &cell.active_ranges,
                ));
                assert!(helpers::table_cell_fragment_prefix_is_folded(
                    &cell.content,
                    &cell.decoration.table_cell_fragments[1 - index],
                    &cell.active_ranges,
                ));
                assert_visible_mapping(&cell, marker);
                assert_visible_mapping(&cell, body);
                let display_prefix =
                    cell.buffer_to_display[start]..cell.buffer_to_display[prefix_end];
                for byte in display_prefix {
                    assert_eq!(run_at(&cell, byte).font.family.as_ref(), "Test Mono");
                    assert!(run_at(&cell, byte).strikethrough.is_none());
                }
                assert_strike_only_for(&cell, &["done"]);
            }
            // At the completed checkbox's closing edge and after its real
            // separator, the marker stays folded rather than expanding again.
            for caret in [marker_start + 5, prefix_end] {
                assert_eq!(render_cell(source, &[caret], None).display, folded.display);
            }
        }
    }

    #[test]
    fn render_pipeline_nested_checked_strikes_are_bounded_and_preserve_explicit_strike() {
        use markdown::BlockType::{TaskList, UnorderedList};

        let source = "- [x] checked<br>  - [ ] open ~~explicit~~ plain<br>    - bullet<br>  - [x] last<br>- [ ] final";
        let cell = render_cell(source, &[], None);
        assert_eq!(
            cell.display,
            "   checked\n      open explicit plain\n      bullet\n      last\n   final"
        );
        assert_eq!(
            cell.decoration
                .table_cell_fragments
                .iter()
                .map(|fragment| fragment.block_type.clone())
                .collect::<Vec<_>>(),
            vec![
                TaskList(true),
                TaskList(false),
                UnorderedList,
                TaskList(true),
                TaskList(false)
            ],
        );
        assert_strike_only_for(&cell, &["checked", "explicit", "last"]);
        for word in [
            "checked", "open", "explicit", "plain", "bullet", "last", "final",
        ] {
            assert_visible_mapping(&cell, word);
        }

        let first_open = render_cell(
            "- [ ] first<br>  - [x] later<br>    - bullet<br>  - [ ] open",
            &[],
            None,
        );
        assert_eq!(
            first_open.display,
            "   first\n      later\n      bullet\n      open"
        );
        assert_strike_only_for(&first_open, &["later"]);
    }

    #[test]
    fn render_pipeline_protected_and_escaped_breaks_do_not_create_list_fragments() {
        for source in [
            "`code<br>- [x] literal`",
            "plain \\<br>- [x] literal",
            "[[note<br>- [x] literal]]",
        ] {
            let cell = render_cell(source, &[], None);
            assert_eq!(cell.decoration.table_cell_fragments.len(), 1, "{source:?}");
            assert_eq!(cell.decoration.table_cell_fragments[0].prefix_len, 0);
            assert_eq!(
                cell.decoration.table_cell_fragments[0].block_type,
                markdown::BlockType::Normal
            );
            assert!(!cell.display.contains('\n'));
            assert!(
                cell.display.contains("<br>- [x] literal"),
                "{:?}",
                cell.display
            );
            assert!(!cell.prefix_collapsed);
            assert_visible_mapping(&cell, "- [x] literal");
            assert_strike_only_for(&cell, &[]);
            if source.starts_with('`') {
                assert_eq!(cell.display, "code<br>- [x] literal");
                for byte in 0..cell.display.len() {
                    assert_eq!(run_at(&cell, byte).font.family.as_ref(), "Test Mono");
                }
            }
        }
    }

    #[test]
    fn render_pipeline_cross_break_emphasis_keeps_prefix_monospace_and_upright() {
        let source = "*lead<br>- [x] task*";
        let folded = render_cell(source, &[], None);
        assert_eq!(folded.display, "lead\n   task");
        assert_eq!(folded.decoration.table_cell_fragments.len(), 2);
        let fragment = &folded.decoration.table_cell_fragments[1];
        assert_eq!(fragment.block_type, markdown::BlockType::TaskList(true));
        assert_eq!(fragment.prefix_len, 6);
        let marker = source.find("- [x]").unwrap();
        let expanded = render_cell(source, &[marker + 3], None);
        assert!(expanded.display.contains("- [x] task"));
        for cell in [folded, expanded] {
            let fragment = &cell.decoration.table_cell_fragments[1];
            let prefix = cell.buffer_to_display[fragment.range.start]
                ..cell.buffer_to_display[fragment.range.start + fragment.prefix_len];
            assert_eq!(
                prefix.len(),
                if cell.active_ranges.is_empty() { 3 } else { 6 }
            );
            for byte in prefix {
                let run = run_at(&cell, byte);
                assert_eq!(run.font.family.as_ref(), "Test Mono");
                assert_eq!(run.font.style, FontStyle::Normal);
                assert_eq!(run.font.weight, FontWeight::BOLD);
                assert!(run.strikethrough.is_none());
            }
            for word in ["lead", "task"] {
                assert_visible_mapping(&cell, word);
                let start = cell.display.find(word).unwrap();
                for byte in start..start + word.len() {
                    let run = run_at(&cell, byte);
                    assert_eq!(run.font.family.as_ref(), "Test Italic");
                    assert_eq!(run.font.style, FontStyle::Italic);
                }
            }
            let task = cell.display.find("task").unwrap();
            for byte in task..task + 4 {
                assert!(run_at(&cell, byte).strikethrough.is_some());
            }
            for byte in 0..cell.display.find("lead").unwrap() + 4 {
                assert!(run_at(&cell, byte).strikethrough.is_none());
            }
        }
    }

    #[test]
    fn render_pipeline_script_breaks_are_atomic_but_following_tasks_still_fold() {
        for delimiter in ['^', '~'] {
            let source = format!("x{delimiter}a<br>b{delimiter}<br>- [x] task");
            let cell = render_cell(&source, &[], None);
            let accepted = source.rfind("<br>").unwrap();
            let script_ranges = if delimiter == '^' {
                &cell.decoration.superscript_spans
            } else {
                &cell.decoration.subscript_spans
            };
            assert_eq!(script_ranges.len(), 1);
            assert_eq!(&cell.content[script_ranges[0].clone()], "a<br>b");
            assert_eq!(cell.decoration.table_cell_fragments.len(), 2);
            assert_eq!(cell.decoration.table_cell_fragments[0].range, 0..accepted);
            let fragment = &cell.decoration.table_cell_fragments[1];
            assert_eq!(fragment.range, accepted + 4..source.len());
            assert_eq!(fragment.block_type, markdown::BlockType::TaskList(true));
            assert_eq!(fragment.prefix_len, 6);
            assert!(helpers::table_cell_fragment_prefix_is_folded(
                &cell.content,
                fragment,
                &cell.active_ranges,
            ));
            assert_eq!(
                cell.decoration
                    .display_replacements
                    .iter()
                    .filter(|replacement| replacement.display_text == "\n")
                    .map(|replacement| replacement.source_range.clone())
                    .collect::<Vec<_>>(),
                vec![accepted..accepted + 4],
            );
            assert_eq!(cell.display.matches('\n').count(), 1);
            assert_eq!(cell.display.split_once('\n').unwrap().1, "   task");
            let display_break = cell.buffer_to_display[accepted];
            assert_eq!(cell.display_to_buffer[display_break], accepted);
            assert_eq!(cell.display.as_bytes()[display_break], b'\n');
            assert_visible_mapping(&cell, "task");
            assert_strike_only_for(&cell, &["task"]);
        }
    }

    #[test]
    fn render_pipeline_inline_math_breaks_do_not_split_the_atomic_expression() {
        let source = "$a<br>b$<br>- [x] task";
        let cell = render_cell(source, &[], None);
        let accepted = source.rfind("<br>").unwrap();
        assert_eq!(cell.decoration.inline_math_spans.len(), 1);
        assert_eq!(
            &cell.content[cell.decoration.inline_math_spans[0].clone()],
            "a<br>b"
        );
        assert_eq!(cell.decoration.table_cell_fragments.len(), 2);
        assert_eq!(cell.decoration.table_cell_fragments[0].range, 0..accepted);
        let fragment = &cell.decoration.table_cell_fragments[1];
        assert_eq!(fragment.range, accepted + 4..source.len());
        assert_eq!(fragment.block_type, markdown::BlockType::TaskList(true));
        assert!(helpers::table_cell_fragment_prefix_is_folded(
            &cell.content,
            fragment,
            &cell.active_ranges,
        ));
        assert_eq!(
            cell.decoration
                .display_replacements
                .iter()
                .filter(|replacement| replacement.display_text == "\n")
                .map(|replacement| replacement.source_range.clone())
                .collect::<Vec<_>>(),
            vec![accepted..accepted + 4],
        );
        assert_eq!(cell.display.matches('\n').count(), 1);
        assert_eq!(cell.display.split_once('\n').unwrap().1, "   task");
        assert_visible_mapping(&cell, "task");
        assert_strike_only_for(&cell, &["task"]);
    }

    #[test]
    fn render_pipeline_raw_html_protects_internal_breaks_without_hiding_later_tasks() {
        for tag in ["code", "pre", "script", "style"] {
            let source = format!("lead <{tag}>raw<br>- [x] literal</{tag}><br>- [x] task");
            let cell = render_cell(&source, &[], None);
            let internal = source.find("<br>").unwrap();
            let accepted = source.rfind("<br>").unwrap();
            assert!(
                cell.decoration
                    .protected_ranges
                    .iter()
                    .any(|range| { range.start <= internal && range.end >= internal + 4 })
            );
            assert_eq!(cell.decoration.table_cell_fragments.len(), 2, "{tag}");
            assert_eq!(cell.decoration.table_cell_fragments[0].prefix_len, 0);
            assert_eq!(cell.decoration.table_cell_fragments[0].range, 0..accepted);
            let fragment = &cell.decoration.table_cell_fragments[1];
            assert_eq!(fragment.block_type, markdown::BlockType::TaskList(true));
            assert_eq!(fragment.prefix_len, 6);
            assert_eq!(
                cell.decoration
                    .display_replacements
                    .iter()
                    .filter(|replacement| replacement.display_text == "\n")
                    .map(|replacement| replacement.source_range.clone())
                    .collect::<Vec<_>>(),
                vec![accepted..accepted + 4],
            );
            assert_eq!(cell.display.matches('\n').count(), 1);
            assert!(cell.display.contains("<br>- [x] literal"));
            assert_eq!(cell.display.split_once('\n').unwrap().1, "   task");
            assert_visible_mapping(&cell, "- [x] literal");
            assert_visible_mapping(&cell, "task");
            assert_strike_only_for(&cell, &["task"]);
        }
    }

    #[test]
    fn render_pipeline_accepted_break_variants_split_into_independent_fragments() {
        let cell = render_cell("one<BR />- [x] two<br/>1. three", &[], None);
        assert_eq!(cell.display, "one\n   two\n   three");
        assert_eq!(cell.decoration.table_cell_fragments.len(), 3);
        assert_strike_only_for(&cell, &["two"]);
        for word in ["one", "two", "three"] {
            assert_visible_mapping(&cell, word);
        }
    }

    #[test]
    fn render_pipeline_utf8_and_blank_fragments_preserve_byte_maps_and_end_caret() {
        let source = "- [x] 完🙂<br><br>  - [ ] 次é<br> <br>- [ ] ";
        let cell = render_cell(source, &[source.len()], None);
        assert_eq!(cell.content, source);
        assert_eq!(cell.display, "   完🙂\n\n      次é\n \n   ");
        assert_eq!(cell.active_ranges, vec![source.len()..source.len()]);
        assert_eq!(cell.decoration.table_cell_fragments.len(), 5);
        let fragments = &cell.decoration.table_cell_fragments;
        assert!(fragments[1].range.is_empty());
        assert_eq!(&cell.content[fragments[3].range.clone()], " ");
        assert_eq!(fragments[1].prefix_len, 0);
        assert_eq!(fragments[3].prefix_len, 0);
        assert_eq!(cell.display_range, 0..cell.raw_display.len());
        let lines = helpers::hard_line_byte_ranges(&cell.display);
        assert_eq!(lines.len(), 5);
        assert!(lines[1].is_empty());
        assert_eq!(&cell.display[lines[3].clone()], " ");
        for word in ["完🙂", "次é"] {
            assert_visible_mapping(&cell, word);
        }
        assert_strike_only_for(&cell, &["完🙂"]);
        for (source_break, _) in source.match_indices("<br>") {
            let display_break = cell.buffer_to_display[source_break];
            assert_eq!(cell.display.as_bytes()[display_break], b'\n');
            assert_eq!(cell.display_to_buffer[display_break], source_break);
            for source_byte in source_break..source_break + 4 {
                assert_eq!(cell.buffer_to_display[source_byte], display_break);
            }
        }
        for source in ["", "<br>", "<br><br>", "<br>  <br>"] {
            let blank = render_cell(source, &[], None);
            assert_eq!(
                blank.decoration.table_cell_fragments.len(),
                source.matches("<br>").count() + 1
            );
            assert_eq!(
                blank.display.matches('\n').count(),
                source.matches("<br>").count()
            );
            assert_strike_only_for(&blank, &[]);
        }
    }

    #[test]
    fn render_pipeline_every_prefix_byte_is_reserved_and_monospace_before_its_body() {
        let source = "- [x] root<br>  12. ordered<br>  - bullet<br>    - [ ] nested<br>1. final";
        let cell = render_cell(source, &[], None);
        assert_eq!(
            cell.display,
            "   root\n      ordered\n    bullet\n         nested\n   final"
        );
        let lines = helpers::hard_line_byte_ranges(&cell.display);
        let reserved = [3, 6, 4, 9, 3];
        let bodies = ["root", "ordered", "bullet", "nested", "final"];
        let mut total_reserved = 0;
        for (index, ((line, prefix_bytes), body)) in
            lines.iter().zip(reserved).zip(bodies).enumerate()
        {
            let fragment = &cell.decoration.table_cell_fragments[index];
            let prefix_source = fragment.range.start..fragment.range.start + fragment.prefix_len;
            let body_start = line.start + prefix_bytes;
            assert_eq!(
                &cell.display[line.start..body_start],
                " ".repeat(prefix_bytes)
            );
            assert_eq!(&cell.display[body_start..line.end], body);
            assert_eq!(cell.buffer_to_display[prefix_source.end], body_start);
            assert_eq!(cell.display_to_buffer[body_start], prefix_source.end);
            for byte in line.start..body_start {
                assert_eq!(run_at(&cell, byte).font.family.as_ref(), "Test Mono");
                assert!(run_at(&cell, byte).strikethrough.is_none());
                assert!(prefix_source.contains(&cell.display_to_buffer[byte]));
            }
            assert_eq!(run_at(&cell, body_start).font.family.as_ref(), "Test Body");
            total_reserved += prefix_bytes;
        }
        assert_eq!(total_reserved, 25);
        assert_eq!(
            cell.runs
                .iter()
                .filter(|run| run.font.family.as_ref() == "Test Mono")
                .map(|run| run.len)
                .sum::<usize>(),
            total_reserved
        );
        assert_eq!(
            cell.display.len(),
            total_reserved + bodies.iter().map(|body| body.len()).sum::<usize>() + 4
        );
    }

    #[test]
    fn render_pipeline_committed_selections_reveal_enclosed_tokens_not_just_endpoints() {
        for token in [
            "**bold**",
            "*italic*",
            "~~strike~~",
            "==mark==",
            "++under++",
            "`code`",
            "^abcd^",
            "~abcd~",
            "[^note]",
            "[[page|alias]]",
            "[label](https://example.com)",
        ] {
            let source = format!("left {token} right");
            let folded = render_cell(&source, &[], None);
            let endpoints = render_cell(&source, &[0, source.len()], None);
            assert_eq!(endpoints.display, folded.display, "{token}");
            assert_ne!(folded.display, source, "{token} must actually fold");
            let selected = render_cell_with_focus(
                &source,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![start..start + source.len()],
                    ..Default::default()
                },
                [0.5, 0.75, 0.5],
            );
            assert_eq!(selected.active_ranges, vec![0..source.len()], "{token}");
            assert_eq!(selected.display, source, "{token}");
            assert_eq!(
                selected.display_to_buffer,
                (0..source.len()).collect::<Vec<_>>()
            );
            assert_eq!(
                selected.buffer_to_display,
                (0..=source.len()).collect::<Vec<_>>()
            );
            assert_visible_mapping(&selected, token);
        }
    }

    #[test]
    fn render_pipeline_cross_cell_selections_clip_without_borrowing_neighbor_bytes() {
        let source = "**one** gap ==two==";
        let second = source.find("==").unwrap();
        for (relative, expected_active, expected_display) in [
            (None, 0..source.len(), source),
            (Some(false), 0..8, "**one** gap two"),
            (Some(true), second - 1..source.len(), "one gap ==two=="),
        ] {
            let cell = render_cell_with_focus(
                source,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![match relative {
                        None => start - 3..start + source.len() + 1,
                        Some(false) => start - 3..start + 8,
                        Some(true) => start + second - 1..start + source.len() + 1,
                    }],
                    ..Default::default()
                },
                [1.0; 3],
            );
            assert_eq!(cell.active_ranges, vec![expected_active]);
            assert_eq!(cell.display, expected_display);
            assert_visible_mapping(&cell, "one");
            assert_visible_mapping(&cell, "two");
        }

        for after in [false, true] {
            let cell = render_cell_with_focus(
                source,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![if after {
                        start + source.len()..start + source.len() + 1
                    } else {
                        start - 3..start
                    }],
                    marked_ranges: vec![if after {
                        start + source.len()..start + source.len() + 1
                    } else {
                        start - 3..start
                    }],
                    ..Default::default()
                },
                [1.0; 3],
            );
            assert!(cell.active_ranges.is_empty());
            assert_eq!(cell.display, "one gap two");
        }
    }

    #[test]
    fn render_pipeline_cell_and_token_boundaries_are_half_open_except_end_carets() {
        let source = "left ==mark== right";
        let token = 5..13;
        for selection in [0..token.start, token.end..source.len()] {
            let cell = render_cell_with_focus(
                source,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![start + selection.start..start + selection.end],
                    ..Default::default()
                },
                [1.0; 3],
            );
            assert_eq!(cell.active_ranges, vec![selection]);
            assert_eq!(cell.display, "left mark right");
        }
        assert_eq!(render_cell(source, &[token.start], None).display, source);
        assert_eq!(
            render_cell(source, &[token.end], None).display,
            "left mark right"
        );
        for caret in [0, source.len()] {
            let cell = render_cell(source, &[caret], None);
            assert_eq!(cell.active_ranges, vec![caret..caret]);
            assert_eq!(cell.display, "left mark right");
        }
        let outside = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![
                    start - 1..start - 1,
                    start + source.len() + 1..start + source.len() + 1,
                ],
                ..Default::default()
            },
            [1.0; 3],
        );
        assert!(outside.active_ranges.is_empty());
        assert_eq!(outside.display, "left mark right");
    }

    #[test]
    fn render_pipeline_multiple_carets_selections_and_ime_ranges_keep_all_targets() {
        let source = "==one== gap ==two== gap ==three==";
        let second = source.find("==two").unwrap();
        let third = source.find("==three").unwrap();
        let carets = render_cell(source, &[2, third + 2], None);
        assert_eq!(carets.active_ranges, vec![2..2, third + 2..third + 2]);
        assert_eq!(carets.display, "==one== gap two gap ==three==");
        let selected = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start + 1..start + 4, start + second + 2..start + second + 4],
                marked_ranges: vec![start + third + 2..start + third + 5],
                ..Default::default()
            },
            [1.0; 3],
        );
        assert_eq!(
            selected.active_ranges,
            vec![1..4, second + 2..second + 4, third + 2..third + 5]
        );
        assert_eq!(selected.display, source);
        let composing = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start + second + 2..start + second + 2],
                marked_ranges: vec![
                    start - 3..start + 4,
                    start + third + 2..start + source.len() + 1,
                ],
                ..Default::default()
            },
            [1.0; 3],
        );
        // Marked ranges take precedence over the fallback empty carets.
        assert_eq!(composing.active_ranges, vec![0..4, third + 2..source.len()]);
        assert_eq!(composing.display, "==one== gap two gap ==three==");
        for cell in [carets, selected, composing] {
            for word in ["one", "two", "three"] {
                assert_visible_mapping(&cell, word);
            }
        }
    }

    #[test]
    fn render_pipeline_drag_inside_one_token_reveals_its_entire_trigger() {
        let source = "left ==one== gap ==two== right";
        let first = 5..12;
        let second = source.find("==two").unwrap();
        let inside = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start + first.start + 2..start + first.start + 4],
                dragging: true,
                ..Default::default()
            },
            [1.0; 3],
        );
        assert_eq!(inside.active_ranges, vec![first]);
        assert_eq!(inside.display, "left ==one== gap two right");
        for dragging in [true, false] {
            let spanning = render_cell_with_focus(
                source,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![start + 7..start + second + 4],
                    dragging,
                    ..Default::default()
                },
                [1.0; 3],
            );
            if dragging {
                assert!(spanning.active_ranges.is_empty());
                assert_eq!(spanning.display, "left one gap two right");
            } else {
                assert_eq!(spanning.active_ranges, vec![7..second + 4]);
                assert_eq!(spanning.display, source);
            }
        }
        let cross_cell = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start - 3..start + second + 4],
                dragging: true,
                ..Default::default()
            },
            [1.0; 3],
        );
        assert!(cross_cell.active_ranges.is_empty());
        assert_eq!(cross_cell.display, "left one gap two right");
    }

    #[test]
    fn render_pipeline_committed_cross_cell_selection_expands_all_fragment_and_inline_syntax() {
        let source = "- [x] done ==marked==<br>  - [ ] next ^abcd^";
        let cell = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start - 3..start + source.len() + 1],
                ..Default::default()
            },
            [0.65; 3],
        );
        assert_eq!(cell.active_ranges, vec![0..source.len()]);
        assert_eq!(cell.display, source.replace("<br>", "\n"));
        assert!(!cell.prefix_collapsed);
        for fragment in &cell.decoration.table_cell_fragments {
            assert!(!helpers::table_cell_fragment_prefix_is_folded(
                &cell.content,
                fragment,
                &cell.active_ranges,
            ));
            let prefix = cell.buffer_to_display[fragment.range.start]
                ..cell.buffer_to_display[fragment.range.start + fragment.prefix_len];
            assert_eq!(prefix.len(), fragment.prefix_len);
            for byte in prefix {
                assert_eq!(run_at(&cell, byte).font.family.as_ref(), "Test Mono");
                assert!(run_at(&cell, byte).strikethrough.is_none());
            }
        }
        for word in ["- [x]", "done", "==marked==", "- [ ]", "next", "^abcd^"] {
            assert_visible_mapping(&cell, word);
        }
        assert!(
            run_at(&cell, cell.display.find("done").unwrap())
                .strikethrough
                .is_some()
        );
        assert!(
            run_at(&cell, cell.display.find("next").unwrap())
                .strikethrough
                .is_none()
        );
    }

    #[test]
    fn render_pipeline_drag_activation_follows_primary_before_earlier_extra_carets() {
        let source = "left ==one== right";
        let token = source.find("==one").unwrap();
        let cell = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![
                    start + token + 2..start + token + 4,
                    start - 2..start - 2,
                    start + 1..start + 1,
                ],
                dragging: true,
                ..Default::default()
            },
            [1.0; 3],
        );
        assert_eq!(cell.display, source);
        assert_eq!(cell.active_ranges.len(), 1);
        let trigger = &cell.active_ranges[0];
        assert!(trigger.start <= token + 2 && trigger.end >= token + 4);
    }

    #[test]
    fn render_pipeline_task_drag_uses_fragment_prefix_triggers() {
        let source = "- [x] one<br>  - [ ] two";
        let second = source.find("  -").unwrap();
        for (selection, active, expected) in [
            (1..4, Some(0..6), "- [x] one\n      two"),
            (
                second + 4..second + 6,
                Some(second..second + 8),
                "   one\n  - [ ] two",
            ),
            (2..second + 6, None, "   one\n      two"),
        ] {
            let cell = render_cell_with_focus(
                source,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![start + selection.start..start + selection.end],
                    dragging: true,
                    ..Default::default()
                },
                [1.0; 3],
            );
            assert_eq!(cell.active_ranges, active.into_iter().collect::<Vec<_>>());
            assert_eq!(cell.display, expected);
            assert_strike_only_for(&cell, &["one"]);
        }
    }

    #[test]
    fn render_pipeline_empty_caret_suppression_does_not_suppress_selections_or_ime() {
        let source = "==one== gap ==two==";
        let second = source.find("==two").unwrap();
        for dragging in [false, true] {
            for suppress_empty_caret in [false, true] {
                let cell = render_cell_with_focus(
                    source,
                    |start| helpers::TableCellSyntaxFocus {
                        selections: vec![start + 2..start + 2],
                        dragging,
                        suppress_empty_caret,
                        ..Default::default()
                    },
                    [1.0; 3],
                );
                let reveal = !dragging && !suppress_empty_caret;
                assert_eq!(cell.active_ranges, if reveal { vec![2..2] } else { vec![] });
                assert_eq!(
                    cell.display,
                    if reveal {
                        "==one== gap two"
                    } else {
                        "one gap two"
                    }
                );
            }
        }
        let selected = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start..start + 8],
                suppress_empty_caret: true,
                ..Default::default()
            },
            [1.0; 3],
        );
        assert_eq!(selected.active_ranges, vec![0..8]);
        assert_eq!(selected.display, "==one== gap two");
        let composing = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start + 2..start + second + 4],
                marked_ranges: vec![start + second + 2..start + second + 4],
                dragging: true,
                suppress_empty_caret: true,
                search_reveal_range: None,
            },
            [1.0; 3],
        );
        assert_eq!(composing.active_ranges, vec![second + 2..second + 4]);
        assert_eq!(composing.display, "one gap ==two==");
    }

    #[test]
    fn render_pipeline_edge_scripts_and_footnotes_preserve_placeholders_and_exact_expansion() {
        for token in ["^abcd^", "~abcd~", "[^note]"] {
            for (lead, tail) in [("", " tail"), ("head ", ""), ("", "")] {
                for padding in ["", " \t  "] {
                    let source = format!("{lead}{token}{tail}{padding}");
                    let expected = format!("{lead}    {tail}");
                    let folded = render_cell(&source, &[], None);
                    assert_eq!(folded.content, source);
                    assert_eq!(folded.raw_display, format!("{expected}{padding}"));
                    assert_eq!(folded.display, expected, "{source:?}");
                    assert_eq!(folded.display_range, 0..expected.len());
                    let placeholder = lead.len()..lead.len() + 4;
                    for byte in placeholder.clone() {
                        assert_eq!(run_at(&folded, byte).color, gpui::transparent_black());
                    }
                    let visible = folded
                        .decoration
                        .superscript_spans
                        .iter()
                        .chain(&folded.decoration.subscript_spans)
                        .chain(
                            folded
                                .decoration
                                .footnote_references
                                .iter()
                                .map(|reference| &reference.collapsed_range),
                        )
                        .cloned()
                        .collect::<Vec<_>>();
                    assert_eq!(visible.len(), 1);
                    let range = &visible[0];
                    assert!(helpers::inline_range_is_collapsed(
                        &folded.decoration,
                        range,
                        &folded.active_ranges
                    ));
                    assert_eq!(
                        folded.buffer_to_display[range.start]..folded.buffer_to_display[range.end],
                        placeholder
                    );
                    assert!(
                        folded.display_to_buffer[placeholder]
                            .iter()
                            .all(|offset| range.contains(offset))
                    );

                    let expanded = render_cell(&source, &[lead.len() + 2], None);
                    let exact = format!("{lead}{token}{tail}");
                    assert_eq!(expanded.raw_display, source);
                    assert_eq!(expanded.display, exact);
                    assert_eq!(
                        expanded.display_to_buffer,
                        (0..exact.len()).collect::<Vec<_>>()
                    );
                    assert_eq!(
                        expanded.buffer_to_display,
                        (0..=source.len())
                            .map(|offset| offset.min(exact.len()))
                            .collect::<Vec<_>>()
                    );
                    assert!(!helpers::inline_range_is_collapsed(
                        &expanded.decoration,
                        range,
                        &expanded.active_ranges
                    ));
                    assert_visible_mapping(&expanded, token);
                    for byte in expanded.buffer_to_display[range.start]
                        ..expanded.buffer_to_display[range.end]
                    {
                        assert_ne!(run_at(&expanded, byte).color, gpui::transparent_black());
                    }
                }
            }
        }
    }

    #[test]
    fn render_pipeline_configured_script_scales_keep_stable_maps_at_cell_and_hard_line_edges() {
        let source = "^abcd^ body<br>~efgh~ 尾🙂<br>[^note] \t ";
        let expanded_default = render_cell_with_focus(
            source,
            |start| helpers::TableCellSyntaxFocus {
                selections: vec![start..start + source.len()],
                ..Default::default()
            },
            [1.0; 3],
        );
        for (scales, widths) in [
            ([0.72; 3], [3, 3, 3]),
            ([0.5, 0.75, 0.65], [2, 3, 3]),
            ([0.65, 0.25, 0.5], [3, 1, 2]),
            ([0.1, 0.5, 0.75], [1, 2, 3]),
        ] {
            let folded = render_cell_with_focus(source, |_| Default::default(), scales);
            let repeated = render_cell_with_focus(source, |_| Default::default(), scales);
            let expected = format!(
                "{} body\n{} 尾🙂\n{}",
                " ".repeat(widths[0]),
                " ".repeat(widths[1]),
                " ".repeat(widths[2])
            );
            assert_eq!(folded.display, expected, "scales: {scales:?}");
            assert_eq!(repeated.display, folded.display);
            assert_eq!(repeated.display_to_buffer, folded.display_to_buffer);
            assert_eq!(repeated.buffer_to_display, folded.buffer_to_display);
            assert_eq!(folded.display_range.end, folded.raw_display.len() - 3);
            assert_visible_mapping(&folded, "body");
            assert_visible_mapping(&folded, "尾🙂");
            let spans = [
                folded.decoration.superscript_spans[0].clone(),
                folded.decoration.subscript_spans[0].clone(),
                folded.decoration.footnote_references[0]
                    .collapsed_range
                    .clone(),
            ];
            for (range, width) in spans.iter().zip(widths) {
                let display =
                    folded.buffer_to_display[range.start]..folded.buffer_to_display[range.end];
                assert_eq!(
                    display.len(),
                    width,
                    "scales: {scales:?}, source range: {range:?}"
                );
                assert_eq!(&folded.display[display.clone()], " ".repeat(width));
                for byte in display {
                    assert!(range.contains(&folded.display_to_buffer[byte]));
                    assert_eq!(run_at(&folded, byte).color, gpui::transparent_black());
                }
            }
            for (source_break, _) in source.match_indices("<br>") {
                let display_break = folded.buffer_to_display[source_break];
                assert_eq!(folded.display.as_bytes()[display_break], b'\n');
                assert_eq!(folded.display_to_buffer[display_break], source_break);
                assert!(
                    folded.buffer_to_display[source_break..source_break + 4]
                        .iter()
                        .all(|offset| *offset == display_break)
                );
            }
            let expanded = render_cell_with_focus(
                source,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![start..start + source.len()],
                    ..Default::default()
                },
                scales,
            );
            assert_eq!(
                expanded.display,
                source.trim_end_matches([' ', '\t']).replace("<br>", "\n")
            );
            assert_eq!(expanded.display, expanded_default.display);
            assert_eq!(
                expanded.display_to_buffer,
                expanded_default.display_to_buffer
            );
            assert_eq!(
                expanded.buffer_to_display,
                expanded_default.buffer_to_display
            );
        }
    }

    #[test]
    fn render_pipeline_utf8_scripts_and_footnotes_keep_character_boundary_maps_at_configured_scales()
     {
        for token in ["^日本^", "~日本~", "[^日本]"] {
            let folded = render_cell_with_focus(token, |_| Default::default(), [0.72; 3]);
            assert_eq!(folded.display, "   ", "token: {token}");
            assert_eq!(folded.display_range, 0..folded.raw_display.len());
            let repeated = render_cell_with_focus(token, |_| Default::default(), [0.72; 3]);
            assert_eq!(folded.display_to_buffer, repeated.display_to_buffer);
            assert_eq!(folded.buffer_to_display, repeated.buffer_to_display);
            let expanded = render_cell_with_focus(
                token,
                |start| helpers::TableCellSyntaxFocus {
                    selections: vec![start..start + token.len()],
                    ..Default::default()
                },
                [0.72; 3],
            );
            assert_eq!(expanded.display, token);
            assert_visible_mapping(&expanded, token);
        }
    }

    #[test]
    fn render_pipeline_enlarged_script_and_footnote_maps_cover_the_full_placeholder() {
        for (token, scales) in [
            ("^abcd^", [1.5, 0.65, 0.65]),
            ("~abcd~", [0.65, 1.5, 0.65]),
            ("[^note]", [0.65, 0.65, 1.5]),
        ] {
            let cell = render_cell_with_focus(token, |_| Default::default(), scales);
            assert_eq!(cell.display, "      ", "token: {token}");
            assert_eq!(cell.display_range, 0..cell.raw_display.len());
            let visible = cell
                .decoration
                .superscript_spans
                .first()
                .or_else(|| cell.decoration.subscript_spans.first())
                .or_else(|| {
                    cell.decoration
                        .footnote_references
                        .first()
                        .map(|reference| &reference.collapsed_range)
                })
                .unwrap();
            let mapped = helpers::buffer_to_display_offset(&cell.buffer_to_display, visible.start)
                ..helpers::buffer_to_display_offset(&cell.buffer_to_display, visible.end);
            assert_eq!(
                mapped,
                0..cell.display.len(),
                "token: {token}, source range: {visible:?}, buffer_to_display: {:?}",
                cell.buffer_to_display
            );
            assert!(
                cell.display_to_buffer
                    .iter()
                    .all(|offset| visible.contains(offset))
            );
        }
    }

    #[test]
    fn render_pipeline_cross_break_highlights_project_whole_cell_spans_onto_both_hard_lines() {
        for (source, words, span_count) in [
            ("==one<br>two==", ["one", "two"], 1),
            ("lead ==一🙂<br>二é== tail", ["一🙂", "二é"], 1),
            ("==one==<br>==two==", ["one", "two"], 2),
        ] {
            for expanded in [false, true] {
                let cell = render_cell_with_focus(
                    source,
                    |start| helpers::TableCellSyntaxFocus {
                        selections: if expanded {
                            vec![start..start + source.len()]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    [1.0; 3],
                );
                assert_eq!(cell.decoration.highlight_spans.len(), span_count);
                let hard_lines = helpers::hard_line_byte_ranges(&cell.display);
                assert_eq!(hard_lines.len(), 2);
                for (hard_line, word) in hard_lines.iter().zip(words) {
                    let text = &cell.display[hard_line.clone()];
                    // These are the whole-cell coordinates passed to the paint
                    // helpers; reparsing either fragment loses a cross-break span.
                    let hard_map = cell
                        .buffer_to_display
                        .iter()
                        .map(|offset| offset.saturating_sub(hard_line.start).min(hard_line.len()))
                        .collect::<Vec<_>>();
                    let highlights = cell
                        .decoration
                        .highlight_spans
                        .iter()
                        .map(|span| {
                            helpers::buffer_to_display_offset(&hard_map, span.start)
                                ..helpers::buffer_to_display_offset(&hard_map, span.end)
                        })
                        .filter(|range| !range.is_empty())
                        .collect::<Vec<_>>();
                    let word_start = text.find(word).unwrap();
                    assert_eq!(
                        highlights,
                        vec![word_start..word_start + word.len()],
                        "{source:?}, expanded: {expanded}"
                    );
                    let runs =
                        helpers::extract_runs_for_range(&cell.runs, hard_line.start, hard_line.end);
                    assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), text.len());
                    let mut start = 0;
                    for run in runs {
                        for byte in start..start + run.len {
                            assert_eq!(
                                run.background_color.is_some(),
                                highlights[0].contains(&byte),
                                "{source:?}, hard line: {text:?}, byte: {byte}"
                            );
                        }
                        start += run.len;
                    }
                    assert_visible_mapping(&cell, word);
                }
                if span_count == 1 {
                    let span = &cell.decoration.highlight_spans[0];
                    let source_break = source.find("<br>").unwrap();
                    assert!(span.start < source_break && span.end > source_break + 4);
                    for fragment in &cell.decoration.table_cell_fragments {
                        let isolated = helpers::table_cell_decoration(
                            &markdown::LineDecoration::default(),
                            &cell.content[fragment.range.clone()],
                            &fragment.range,
                        );
                        assert!(isolated.highlight_spans.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn render_pipeline_numeric_layout_stays_bounded_with_structural_padding_at_each_scale() {
        let source = "- [x] done<br>12. later<br>- [ ] ";
        let rendered = render_cell(source, &[], None);
        let text = format!("| Neighbor | Input |\n| :--- | ---: |\n| 隣🙂 | {source}|");
        let buffer = Buffer::from_text(&text);
        let decorations = markdown::parse_decorations(&text);
        for scale in [0.75, 1.0, 1.5, 2.0] {
            for width in [40.0, 160.0, 400.0] {
                let layout = helpers::compute_table_layout(
                    &buffer,
                    &decorations,
                    0,
                    3,
                    width,
                    8.0 * scale,
                    4.0 * scale,
                    8.0 * scale,
                    20.0 * scale,
                    16.0 * scale,
                );
                assert!(layout.total_width <= width + 0.01);
                assert_eq!(layout.visual_row_to_line, vec![0, 2]);
                let cell = &layout.cells[1][1];
                assert_eq!(cell.display_text, rendered.display);
                assert_eq!(cell.cell_range, rendered.cell_range);
                assert_eq!(cell.display_to_buffer, rendered.display_to_buffer);
                assert_eq!(cell.buffer_to_display, rendered.buffer_to_display);
                assert_eq!(cell.alignment, markdown::TableAlign::Right);
                for row in &layout.cells {
                    assert_eq!(row[0].x, 0.0);
                    assert!((row[0].width - row[1].x).abs() < 0.01);
                    assert!((row[1].x + row[1].width - layout.total_width).abs() < 0.01);
                    assert!(row.iter().all(|cell| cell.width > 0.0 && cell.height > 0.0));
                    assert!(
                        row.iter()
                            .all(|cell| cell.y + cell.height <= layout.total_height + 0.01)
                    );
                }
            }
        }
    }
}
