//! Pixel-based metadata columns shared by painting, caret/IME and hit testing.
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    FontFallbacks, FontWeight, LineLayout, TextRun, Window, WrappedLine, WrappedLineLayout, px,
};

use crate::front_matter::FrontMatterValue;
use crate::markdown::{BlockType, LineDecoration};

use super::{EditorView, helpers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Columns {
    pub value_x: f32,
    space_width: f32,
    gap: f32,
}

impl EditorView {
    pub(super) fn measure_front_matter_columns(&self, window: &mut Window) -> Option<Columns> {
        let table = self.decorations.first()?.table_info.as_ref()?;
        if !table.is_front_matter {
            return None;
        }
        let styles = &self.styles.editor;
        let mut font = window.text_style().font();
        let family = styles.editor_monospace_font_family;
        if family != crate::platform::fonts::SYSTEM_UI {
            font.family = family.into();
        }
        font.fallbacks = (family != styles.editor_body_cjk_font_family).then(|| {
            FontFallbacks::from_fonts(vec![styles.editor_body_cjk_font_family.to_string()])
        });
        font.weight = FontWeight::NORMAL;
        font.style = gpui::FontStyle::Normal;
        let font_size = self.font_size_for_block(&BlockType::Table);
        let spacing = styles.editor_letter_spacing_px;
        let run = |len, weight| {
            let mut font = font.clone();
            font.weight = weight;
            TextRun {
                len,
                font,
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        };
        let measure = |text: &str, runs: &[TextRun]| {
            let mut shaped = window
                .text_system()
                .shape_text(text.to_owned().into(), px(font_size), runs, None, None)
                .ok()?
                .into_iter()
                .next()?;
            helpers::apply_letter_spacing(&mut shaped, spacing);
            Some(f32::from(shaped.width()))
        };
        let space_width = measure(" ", &[run(1, FontWeight::NORMAL)])? + spacing;
        let space_width = space_width.max(1.0);
        let gap = self
            .scaled(self.styles.table.front_matter_key_padding_right_px)
            .max(space_width);
        let mut max_label_width = 0.0_f32;
        for line in table.table_start_line..table.table_end_line.min(self.decorations.len()) {
            let Some(key) = self.decorations[line]
                .front_matter
                .as_ref()
                .and_then(|info| info.key_range.as_ref())
            else {
                continue;
            };
            let source = self.buffer.line_content(line);
            let Some(end) = source
                .get(key.end..)
                .and_then(|tail| tail.find(':'))
                .map(|colon| key.end + colon + 1)
            else {
                continue;
            };
            let runs = label_runs(
                &source[..end],
                key.clone(),
                &font,
                gpui::transparent_black(),
            );
            if let Some(width) = measure(&source[..end], &runs) {
                // Tracking after the colon is part of the clear key/value gap.
                max_label_width = max_label_width.max(width + spacing);
            }
        }
        Some(Columns {
            value_x: max_label_width + gap,
            space_width,
            gap,
        })
    }
}

pub(super) fn label_runs(
    label: &str,
    key: Range<usize>,
    font: &gpui::Font,
    color: gpui::Hsla,
) -> Vec<TextRun> {
    [
        (0..key.start, FontWeight::NORMAL),
        (key.clone(), FontWeight::BOLD),
        (key.end..label.len(), FontWeight::NORMAL),
    ]
    .into_iter()
    .filter(|(range, _)| !range.is_empty())
    .map(|(range, weight)| {
        let mut font = font.clone();
        font.weight = weight;
        font.style = gpui::FontStyle::Normal;
        TextRun {
            len: range.len(),
            font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    })
    .collect()
}

/// Synthetic spaces carry byte maps, not column geometry. Stretch only those
/// spaces, leaving each key and value's shaped advances intact.
fn resize_span(wrapped: &mut WrappedLine, range: Range<usize>, width: f32) {
    let old = &wrapped.unwrapped_layout;
    let left = f32::from(old.x_for_index(range.start));
    let old_width = f32::from(old.x_for_index(range.end)) - left;
    let delta = width - old_width;
    let mut runs = old.runs.clone();
    for glyph in runs.iter_mut().flat_map(|run| &mut run.glyphs) {
        if glyph.index >= range.end {
            glyph.position.x += px(delta);
        } else if glyph.index >= range.start {
            let fraction = if old_width > 0.0 {
                (f32::from(glyph.position.x) - left) / old_width
            } else {
                0.0
            };
            glyph.position.x = px(left + fraction * width);
        }
    }
    let layout = Arc::new(LineLayout {
        font_size: old.font_size,
        width: old.width + px(delta),
        ascent: old.ascent,
        descent: old.descent,
        runs,
        len: old.len,
    });
    let arc: &mut Arc<WrappedLineLayout> = wrapped;
    *arc = Arc::new(WrappedLineLayout {
        unwrapped_layout: layout,
        wrap_boundaries: Default::default(),
        wrap_width: None,
        row_heights: Vec::new(),
    });
}

/// Wrap the value alone: a reduced value width must never also wrap the key and
/// padding. The returned indent is used for every continuation row.
pub(super) fn align_and_wrap(
    wrapped: &mut WrappedLine,
    decoration: &LineDecoration,
    buffer_to_display: &[usize],
    active: bool,
    columns: Columns,
    available_width: Option<f32>,
    checkbox_width: f32,
) -> Option<f32> {
    let info = decoration.front_matter.as_ref()?;
    let prefix = *buffer_to_display.get(info.value_range.start)?;
    let display = wrapped.text.clone();
    let prefix_text = display.get(..prefix)?;
    if prefix_text.is_empty()
        || (info.key_range.is_none()
            && (matches!(info.value, FrontMatterValue::Raw) || !prefix_text.starts_with(' ')))
    {
        return None;
    }
    let padding = if info.key_range.is_some() {
        prefix_text.rfind(':')? + 1..prefix
    } else {
        0..prefix_text.len() - prefix_text.trim_start_matches(' ').len()
    };
    if padding.is_empty() {
        return None;
    }
    let old = &wrapped.unwrapped_layout;
    let padding_left = f32::from(old.x_for_index(padding.start));
    let tail_width = f32::from(old.x_for_index(prefix) - old.x_for_index(padding.end));
    let desired_x = columns.value_x
        + if info.key_range.is_none() {
            info.indent_columns as f32 * columns.space_width
        } else {
            0.0
        };
    let target_x = desired_x.max(padding_left + tail_width + columns.gap);
    let boolean = !active && matches!(info.value, FrontMatterValue::Boolean(_));
    let stacked = available_width.is_some_and(|width| {
        target_x > width * 0.7 || (boolean && checkbox_width + columns.gap > width - target_x)
    });
    let stack_break = (stacked && info.key_range.is_some()).then_some(padding.start);
    // Zero-width mapped spaces also provide an explicit insertion row for an
    // empty stacked value. Keyless bodies have no label row to stack beneath.
    let padding_width = if stacked {
        0.0
    } else {
        target_x - padding_left - tail_width
    };
    resize_span(wrapped, padding, padding_width);
    let atomic_value = if boolean {
        let end = *buffer_to_display.get(info.value_range.end)?;
        if end > prefix {
            resize_span(wrapped, prefix..end, checkbox_width + columns.gap);
        }
        Some(prefix..end)
    } else {
        None
    };
    let value_x = if stacked {
        padding_left + tail_width
    } else {
        target_x
    };
    let indent = if stacked {
        if info.key_range.is_some() {
            0.0
        } else {
            tail_width
        }
    } else {
        target_x
    };
    wrap_value(
        wrapped,
        prefix,
        value_x,
        indent,
        available_width,
        stack_break,
        atomic_value,
    );
    Some(indent)
}

fn wrap_value(
    wrapped: &mut WrappedLine,
    prefix: usize,
    value_x: f32,
    indent: f32,
    available_width: Option<f32>,
    stack_break: Option<usize>,
    atomic_value: Option<Range<usize>>,
) {
    let Some(width) = available_width else {
        return;
    };
    let original = wrapped.unwrapped_layout.clone();
    let mut maps = Vec::new();
    let value_runs = original
        .runs
        .iter()
        .enumerate()
        .filter_map(|(run_ix, run)| {
            let indices = run
                .glyphs
                .iter()
                .enumerate()
                .filter_map(|(index, glyph)| {
                    (glyph.index >= prefix
                        && !atomic_value.as_ref().is_some_and(|range| {
                            range.start < glyph.index && glyph.index < range.end
                        }))
                    .then_some(index)
                })
                .collect::<Vec<_>>();
            if indices.is_empty() {
                return None;
            }
            let glyphs = indices
                .iter()
                .map(|&index| {
                    let mut glyph = run.glyphs[index].clone();
                    glyph.position.x -= px(value_x);
                    glyph
                })
                .collect();
            maps.push((run_ix, indices));
            Some(gpui::ShapedRun {
                font_id: run.font_id,
                glyphs,
            })
        })
        .collect();
    let arc: &mut Arc<WrappedLineLayout> = wrapped;
    *arc = Arc::new(WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            font_size: original.font_size,
            width: (original.width - px(value_x)).max(px(0.0)),
            ascent: original.ascent,
            descent: original.descent,
            runs: value_runs,
            len: original.len,
        }),
        wrap_boundaries: Default::default(),
        wrap_width: None,
        row_heights: Vec::new(),
    });
    let value_width = px((width - indent).max(1.0));
    wrapped.rewrap(value_width);
    let mut boundaries = wrapped
        .wrap_boundaries
        .iter()
        .map(|boundary| gpui::WrapBoundary {
            run_ix: maps[boundary.run_ix].0,
            glyph_ix: maps[boundary.run_ix].1[boundary.glyph_ix],
        })
        .collect::<Vec<_>>();
    if let Some(key_end) = stack_break {
        // Stack values below oversized keys instead of squeezing them over keys.
        let mut key = wrapped.clone();
        let arc: &mut Arc<WrappedLineLayout> = &mut key;
        *arc = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: original.font_size,
                width: original.x_for_index(key_end),
                ascent: original.ascent,
                descent: original.descent,
                runs: original
                    .runs
                    .iter()
                    .map(|run| gpui::ShapedRun {
                        font_id: run.font_id,
                        glyphs: run
                            .glyphs
                            .iter()
                            .filter(|glyph| glyph.index < key_end)
                            .cloned()
                            .collect(),
                    })
                    .collect(),
                len: original.len,
            }),
            wrap_boundaries: Default::default(),
            wrap_width: None,
            row_heights: Vec::new(),
        });
        key.rewrap(px(width.max(1.0)));
        let mut key_boundaries = key
            .wrap_boundaries
            .iter()
            .copied()
            .filter(|boundary| {
                let glyph = &original.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
                glyph.index < key_end
            })
            .collect::<Vec<_>>();
        if let Some((run_ix, glyph_ix)) =
            original.runs.iter().enumerate().find_map(|(run_ix, run)| {
                run.glyphs
                    .iter()
                    .position(|glyph| glyph.index >= key_end)
                    .map(|glyph_ix| (run_ix, glyph_ix))
            })
        {
            key_boundaries.push(gpui::WrapBoundary { run_ix, glyph_ix });
        }
        key_boundaries.extend(boundaries);
        boundaries = key_boundaries;
    }
    let arc: &mut Arc<WrappedLineLayout> = wrapped;
    *arc = Arc::new(WrappedLineLayout {
        unwrapped_layout: original,
        wrap_boundaries: boundaries.into_iter().collect(),
        wrap_width: Some(value_width),
        row_heights: Vec::new(),
    });
}

/// The left edge of a stacked value row belongs to the value, not to source
/// whitespace before it. Otherwise typing there can turn `key: value` into
/// `key:x value` and break YAML ownership.
pub(super) fn align_stacked_padding_hits(
    wrapped: &WrappedLine,
    decoration: &LineDecoration,
    buffer_to_display: &[usize],
    display_to_buffer: &mut [usize],
) {
    let Some(info) = decoration
        .front_matter
        .as_ref()
        .filter(|info| info.key_range.is_some())
    else {
        return;
    };
    let Some(&prefix) = buffer_to_display.get(info.value_range.start) else {
        return;
    };
    let Some(padding_start) = wrapped
        .text
        .get(..prefix)
        .and_then(|text| text.rfind(':'))
        .map(|colon| colon + 1)
    else {
        return;
    };
    if wrapped.unwrapped_layout.x_for_index(padding_start)
        == wrapped.unwrapped_layout.x_for_index(prefix)
        && helpers::wrap_boundary_byte_indices(wrapped, &wrapped.text).contains(&padding_start)
        && let Some(padding) = display_to_buffer.get_mut(padding_start..prefix)
    {
        padding.fill(info.value_range.start);
    }
}

/// A separately painted soft row must retain the adjusted glyphs instead of
/// reshaping its padding back into font-dependent spaces.
pub(super) fn copy_segment_geometry(
    segment: &mut WrappedLine,
    original: &WrappedLine,
    range: Range<usize>,
) {
    let layout = &original.unwrapped_layout;
    let start_x = layout.x_for_index(range.start);
    let runs = layout
        .runs
        .iter()
        .filter_map(|run| {
            let glyphs = run
                .glyphs
                .iter()
                .filter(|glyph| range.contains(&glyph.index))
                .map(|glyph| {
                    let mut glyph = glyph.clone();
                    glyph.index -= range.start;
                    glyph.position.x -= start_x;
                    glyph
                })
                .collect::<Vec<_>>();
            (!glyphs.is_empty()).then_some(gpui::ShapedRun {
                font_id: run.font_id,
                glyphs,
            })
        })
        .collect();
    let arc: &mut Arc<WrappedLineLayout> = segment;
    *arc = Arc::new(WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            font_size: layout.font_size,
            width: layout.x_for_index(range.end) - start_x,
            ascent: layout.ascent,
            descent: layout.descent,
            runs,
            len: range.len(),
        }),
        wrap_boundaries: Default::default(),
        wrap_width: None,
        row_heights: Vec::new(),
    });
}

pub(super) fn wrap_boundary_upstream_xs(wrapped: &WrappedLine) -> HashMap<usize, f32> {
    let layout = &wrapped.unwrapped_layout;
    let mut start = 0;
    helpers::wrap_boundary_byte_indices(wrapped, &wrapped.text)
        .into_iter()
        .map(|end| {
            let width = f32::from(layout.x_for_index(end) - layout.x_for_index(start));
            start = end;
            (end, width)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use gpui::{FontId, GlyphId, ShapedGlyph, ShapedRun, point};

    use super::*;
    use crate::buffer::Buffer;
    use crate::markdown::parse_decorations;

    // Deterministic proportional advances emulate bold Latin, CJK fallback and
    // emoji without requiring a native window/font installation in unit tests.
    fn shaped(text: &str, scale: f32) -> WrappedLine {
        let glyph_id = unsafe { std::mem::zeroed::<GlyphId>() };
        let key_end = text.rfind(':').map_or(0, |index| index + 1);
        let mut x = 0.0;
        let mut key_glyphs = Vec::new();
        let mut value_glyphs = Vec::new();
        for (index, ch) in text.char_indices() {
            let glyph = ShapedGlyph {
                id: glyph_id,
                position: point(px(x), px(0.0)),
                index,
                is_emoji: ch == '🌸',
            };
            let key = index < key_end;
            if key {
                key_glyphs.push(glyph);
            } else {
                value_glyphs.push(glyph);
            }
            let width = match ch {
                ' ' | '\u{a0}' => 4.0,
                'i' | 'l' | ':' => 3.0,
                'W' => 14.0,
                '🌸' => 22.0,
                ch if !ch.is_ascii() => 19.0,
                _ => 7.0,
            };
            x += (width + if key { 1.0 } else { 0.0 }) * scale;
        }
        let mut line = WrappedLine::default();
        line.text = text.to_owned().into();
        let arc: &mut Arc<WrappedLineLayout> = &mut line;
        *arc = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: px(16.0 * scale),
                width: px(x),
                ascent: px(14.0 * scale),
                descent: px(4.0 * scale),
                runs: vec![
                    ShapedRun {
                        font_id: FontId(0),
                        glyphs: key_glyphs,
                    },
                    ShapedRun {
                        font_id: FontId(1),
                        glyphs: value_glyphs,
                    },
                ],
                len: text.len(),
            }),
            wrap_boundaries: Default::default(),
            wrap_width: None,
            row_heights: Vec::new(),
        });
        line
    }

    fn display(
        source: &str,
        decoration: &LineDecoration,
        active: bool,
    ) -> (String, Vec<usize>, Vec<usize>) {
        let ranges = if active {
            vec![source.len()..source.len()]
        } else {
            vec![]
        };
        let (display, d2b, b2d, _) = helpers::build_display_text(
            source, decoration, &ranges, false, None, 0, 3, 3, 4, 2, 0.7, 0.7, 0.7,
        );
        (display, d2b, b2d)
    }

    fn columns(scale: f32) -> Columns {
        Columns {
            value_x: 180.0 * scale,
            space_width: 4.0 * scale,
            gap: 10.0 * scale,
        }
    }

    #[test]
    fn proportional_keys_and_values_share_pixel_column_at_every_zoom_and_tracking() {
        let source = "---\nWWW: \"text\"\ni: 12\n中文🌸: null\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        for scale in [0.75, 1.0, 1.5, 2.0] {
            for spacing in [0.0, 0.5, 1.0] {
                for active in [false, true] {
                    for row in 1..4 {
                        let info = decorations[row].front_matter.as_ref().unwrap();
                        let (display, d2b, b2d) =
                            display(buffer.line_content(row), &decorations[row], active);
                        let prefix = b2d[info.value_range.start];
                        let mut wrapped = shaped(&display, scale);
                        helpers::apply_letter_spacing(&mut wrapped, spacing);
                        let key_end = display[..prefix].rfind(':').unwrap() + 1;
                        let key_width = wrapped.unwrapped_layout.x_for_index(key_end);
                        let value_width = wrapped.unwrapped_layout.width
                            - wrapped.unwrapped_layout.x_for_index(prefix);
                        assert_eq!(
                            align_and_wrap(
                                &mut wrapped,
                                &decorations[row],
                                &b2d,
                                active,
                                columns(scale),
                                None,
                                18.0 * scale,
                            ),
                            Some(180.0 * scale)
                        );
                        assert_eq!(
                            wrapped.unwrapped_layout.x_for_index(prefix),
                            px(180.0 * scale)
                        );
                        assert_eq!(wrapped.unwrapped_layout.x_for_index(key_end), key_width);
                        assert!(px(180.0 * scale) - key_width >= px(10.0 * scale));
                        assert_eq!(
                            wrapped.unwrapped_layout.width - px(180.0 * scale),
                            value_width
                        );
                        assert_eq!(wrapped.text.as_ref(), display);
                        assert!(d2b[prefix] >= info.value_range.start);
                        assert!(d2b[prefix] <= info.value_range.end);
                    }
                }
            }
        }
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn multiline_and_sequence_body_columns_do_not_move_on_focus() {
        let source = "---\nbody: |-\n  first text\n  中文🌸\nitems:\n  - 'one'\n  - two\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        for row in [2, 3, 5, 6] {
            for active in [false, true] {
                let info = decorations[row].front_matter.as_ref().unwrap();
                let (display, _, b2d) =
                    display(buffer.line_content(row), &decorations[row], active);
                let prefix = b2d[info.value_range.start];
                let mut wrapped = shaped(&display, 1.0);
                let expected = columns(1.0).value_x + info.indent_columns as f32 * 4.0;
                assert_eq!(
                    align_and_wrap(
                        &mut wrapped,
                        &decorations[row],
                        &b2d,
                        active,
                        columns(1.0),
                        Some(500.0),
                        18.0
                    ),
                    Some(expected)
                );
                assert_eq!(wrapped.unwrapped_layout.x_for_index(prefix), px(expected));
            }
        }
    }

    #[test]
    fn soft_wrap_never_wraps_key_or_padding_and_paint_uses_adjusted_glyphs() {
        let source = "---\nWWW: one two three four five six seven eight nine ten\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let info = decorations[1].front_matter.as_ref().unwrap();
        let (display, _, b2d) = display(buffer.line_content(1), &decorations[1], false);
        let prefix = b2d[info.value_range.start];
        let mut wrapped = shaped(&display, 1.0);
        helpers::apply_letter_spacing(&mut wrapped, 1.0);
        assert_eq!(
            align_and_wrap(
                &mut wrapped,
                &decorations[1],
                &b2d,
                false,
                columns(1.0),
                Some(300.0),
                18.0
            ),
            Some(180.0)
        );
        let boundaries = helpers::wrap_boundary_byte_indices(&wrapped, &display);
        assert!(!boundaries.is_empty());
        assert!(boundaries.iter().all(|&boundary| boundary > prefix));
        let first_end = boundaries[0];
        let mut segment = shaped(&display[..first_end], 1.0);
        copy_segment_geometry(&mut segment, &wrapped, 0..first_end);
        assert_eq!(segment.unwrapped_layout.x_for_index(prefix), px(180.0));
        let start = first_end;
        let end = boundaries.get(1).copied().unwrap_or(display.len());
        let mut next = shaped(&display[start..end], 1.0);
        copy_segment_geometry(&mut next, &wrapped, start..end);
        assert_eq!(next.unwrapped_layout.x_for_index(0), px(0.0));
        assert!(f32::from(next.width()) <= 120.0);
        let xs = wrap_boundary_upstream_xs(&wrapped);
        assert_eq!(
            xs[&first_end],
            f32::from(wrapped.unwrapped_layout.x_for_index(first_end))
        );
    }

    #[test]
    fn oversized_keys_stack_values_without_wrapping_synthetic_padding() {
        let source = "---\nWWWWWWWWWWWW: one two three four five\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let info = decorations[1].front_matter.as_ref().unwrap();
        let (display, _, b2d) = display(buffer.line_content(1), &decorations[1], false);
        let prefix = b2d[info.value_range.start];
        let mut wrapped = shaped(&display, 1.0);
        let metrics = Columns {
            value_x: 220.0,
            ..columns(1.0)
        };
        assert_eq!(
            align_and_wrap(
                &mut wrapped,
                &decorations[1],
                &b2d,
                false,
                metrics,
                Some(100.0),
                18.0
            ),
            Some(0.0)
        );
        let boundaries = helpers::wrap_boundary_byte_indices(&wrapped, &display);
        assert!(boundaries.iter().any(|&boundary| boundary <= prefix));
        assert!(wrapped.position_for_index(prefix + 1, px(24.0)).unwrap().y >= px(24.0));
        let padding_start = display[..prefix].rfind(':').unwrap() + 1;
        assert!(boundaries.contains(&padding_start));
        assert!(
            boundaries
                .iter()
                .all(|&boundary| boundary <= padding_start || boundary >= prefix)
        );
        assert!(
            wrap_boundary_upstream_xs(&wrapped)
                .values()
                .all(|&x| x <= 100.0)
        );
    }

    #[test]
    fn checkbox_reserves_exact_control_width_and_gap_for_comments() {
        let source = "---\ni: true # comment\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let info = decorations[1].front_matter.as_ref().unwrap();
        let (display, _, b2d) = display(buffer.line_content(1), &decorations[1], false);
        let mut wrapped = shaped(&display, 1.0);
        align_and_wrap(
            &mut wrapped,
            &decorations[1],
            &b2d,
            false,
            columns(1.0),
            Some(500.0),
            32.0,
        )
        .unwrap();
        let left = wrapped
            .unwrapped_layout
            .x_for_index(b2d[info.value_range.start]);
        let right = wrapped
            .unwrapped_layout
            .x_for_index(b2d[info.value_range.end]);
        assert_eq!(left, px(180.0));
        assert_eq!(right - left, px(42.0));
        assert!(
            wrapped
                .unwrapped_layout
                .x_for_index(display.find('#').unwrap())
                >= right
        );
    }

    #[test]
    fn empty_stacked_values_and_short_quoted_keys_keep_caret_visible() {
        for token in ["", "''", "\"\""] {
            let source = format!("---\n\"i\": {token}\nWWWWWWWWWWWW: text\n---");
            let buffer = Buffer::from_text(&source);
            let decorations = parse_decorations(&source);
            for active in [false, true] {
                let info = decorations[1].front_matter.as_ref().unwrap();
                let (display, _, b2d) = display(buffer.line_content(1), &decorations[1], active);
                let mut wrapped = shaped(&display, 1.0);
                align_and_wrap(
                    &mut wrapped,
                    &decorations[1],
                    &b2d,
                    active,
                    columns(1.0),
                    Some(100.0),
                    18.0,
                )
                .unwrap();
                let boundaries = helpers::wrap_boundary_byte_indices(&wrapped, &display);
                let key_end = display.find(':').unwrap() + 1;
                assert_eq!(boundaries, vec![key_end]);
                let caret = wrapped
                    .position_for_index(b2d[info.value_range.end], px(24.0))
                    .unwrap();
                assert_eq!(caret.y, px(24.0));
                assert!(caret.x < px(100.0));
                assert!(
                    wrap_boundary_upstream_xs(&wrapped)
                        .values()
                        .all(|&x| x < 100.0)
                );
            }
        }
    }

    #[test]
    fn deeply_indented_narrow_body_rows_have_no_phantom_label_row() {
        let source = "---\nparent:\n  body: |-\n    first\n    second\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let metrics = Columns {
            value_x: 130.0,
            space_width: 10.0,
            gap: 10.0,
        };
        let mut plans = helpers::front_matter_group_plan(&buffer, &decorations, &[], true);
        constrain_groups(&mut plans, Some(metrics), 200.0, &decorations);
        assert!(plans.iter().all(|plan| plan.source_visible));
        for row in [3, 4] {
            let info = decorations[row].front_matter.as_ref().unwrap();
            let (display, _, b2d) = display(buffer.line_content(row), &decorations[row], false);
            let mut wrapped = shaped(&display, 1.0);
            assert_eq!(
                align_and_wrap(
                    &mut wrapped,
                    &decorations[row],
                    &b2d,
                    false,
                    metrics,
                    Some(200.0),
                    18.0
                ),
                Some(0.0)
            );
            assert!(wrapped.wrap_boundaries().is_empty());
            assert_eq!(
                wrapped
                    .position_for_index(b2d[info.value_range.start], px(24.0))
                    .unwrap()
                    .y,
                px(0.0)
            );
        }
    }

    #[test]
    fn narrow_checkbox_reservation_is_atomic_and_stacks_as_a_whole() {
        let source = "---\ni: true # comment\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let info = decorations[1].front_matter.as_ref().unwrap();
        let (display, _, b2d) = display(buffer.line_content(1), &decorations[1], false);
        let mut wrapped = shaped(&display, 1.0);
        let metrics = Columns {
            value_x: 60.0,
            ..columns(1.0)
        };
        assert_eq!(
            align_and_wrap(
                &mut wrapped,
                &decorations[1],
                &b2d,
                false,
                metrics,
                Some(100.0),
                40.0
            ),
            Some(0.0)
        );
        let range = b2d[info.value_range.start]..b2d[info.value_range.end];
        let boundaries = helpers::wrap_boundary_byte_indices(&wrapped, &display);
        assert!(
            boundaries
                .iter()
                .all(|&boundary| boundary <= range.start || boundary >= range.end)
        );
        let position = wrapped.position_for_index(range.start, px(24.0)).unwrap();
        assert_eq!(position.y, px(24.0));
        assert_eq!(position.x, px(0.0));
        assert!(position.x + px(40.0) <= px(100.0));
    }

    #[test]
    fn stacked_value_left_edge_hits_insert_after_the_yaml_separator() {
        for value in ["abc", "", "''"] {
            let source = format!("---\ni: {value}\nWWWWWWWWWWWW: text\n---");
            let mut buffer = Buffer::from_text(&source);
            let decorations = parse_decorations(&source);
            let line = buffer.line_content(1).to_owned();
            let info = decorations[1].front_matter.as_ref().unwrap();
            let (display, mut d2b, b2d) = display(&line, &decorations[1], false);
            let mut wrapped = shaped(&display, 1.0);
            align_and_wrap(
                &mut wrapped,
                &decorations[1],
                &b2d,
                false,
                columns(1.0),
                Some(100.0),
                18.0,
            )
            .unwrap();
            align_stacked_padding_hits(&wrapped, &decorations[1], &b2d, &mut d2b);
            // offset_for_position and vertical movement use this boundary map
            // when the pointer/preferred x is at the downstream row's left edge.
            let start = helpers::wrap_boundary_byte_indices(&wrapped, &display)[0];
            let buffer_col = d2b[start];
            assert_eq!(buffer_col, info.value_range.start);
            let offset = buffer.line_start(1) + buffer_col;
            buffer.replace(offset..offset, "x");
            assert_eq!(buffer.line_content(1), format!("i: x{value}"));
        }
    }

    #[test]
    fn raw_empty_keyless_rows_keep_caret_at_column_zero() {
        let source = "---\nbody: |-\n  text\n\n---";
        let decorations = parse_decorations(source);
        let (display, _, b2d) = display("", &decorations[3], true);
        let mut wrapped = shaped(&display, 1.0);
        assert!(
            align_and_wrap(
                &mut wrapped,
                &decorations[3],
                &b2d,
                true,
                columns(1.0),
                Some(100.0),
                18.0
            )
            .is_none()
        );
        assert_eq!(wrapped.unwrapped_layout.x_for_index(0), px(0.0));
    }

    #[test]
    fn measured_narrow_columns_keep_both_lists_and_multiline_groups_source_visible() {
        let source = "---\nbody: |-\n  text\nitems:\n  - one\n  - two\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let mut plans = helpers::front_matter_group_plan(&buffer, &decorations, &[], true);
        assert_eq!(plans.len(), 2);
        constrain_groups(&mut plans, Some(columns(1.0)), 200.0, &decorations);
        for plan in plans {
            assert!(plan.source_visible);
            assert!(
                plan.group
                    .source_lines
                    .clone()
                    .all(|line| !plan.hides_line(line))
            );
        }
    }
}

pub(super) fn constrain_groups(
    plans: &mut [helpers::FrontMatterGroupPlan],
    columns: Option<Columns>,
    width: f32,
    decorations: &[LineDecoration],
) {
    let Some(columns) = columns else {
        return;
    };
    for plan in plans {
        let indent = plan
            .group
            .source_lines
            .clone()
            .filter_map(|line| decorations.get(line)?.front_matter.as_ref())
            .filter(|info| info.key_range.is_none())
            .map(|info| info.indent_columns)
            .max()
            .unwrap_or(0);
        plan.source_visible |= columns.value_x + indent as f32 * columns.space_width > width * 0.7;
    }
}
