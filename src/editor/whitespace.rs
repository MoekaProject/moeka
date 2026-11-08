use gpui::{Bounds, Corners, PaintQuad, Pixels, Point, WrappedLine, fill, point, px, size};

use crate::markdown::{BlockType, InlineKind, LineDecoration};
use crate::style::WhitespaceMarkerStyle;

#[derive(Clone, Copy)]
pub(super) struct Visibility<'a> {
    pub all: bool,
    pub decoration: &'a LineDecoration,
    pub display_to_buffer: &'a [usize],
    pub display_offset: usize,
}

impl Visibility<'_> {
    pub fn enabled(self) -> bool {
        self.all
            || !self.decoration.inline_math_spans.is_empty()
            || self
                .decoration
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::InlineCode)
    }

    fn at(self, display_index: usize) -> bool {
        self.all
            || self
                .display_to_buffer
                .get(self.display_offset + display_index)
                .is_some_and(|index| {
                    self.decoration.inline_spans.iter().any(|span| {
                        span.kind == InlineKind::InlineCode && span.range.contains(index)
                    }) || self
                        .decoration
                        .inline_math_spans
                        .iter()
                        .any(|range| range.contains(index))
                })
    }
}

pub(super) fn force_line(
    source_mode: bool,
    non_markdown: bool,
    decoration: &LineDecoration,
) -> bool {
    source_mode
        || non_markdown
        || matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        )
        || decoration.front_matter.is_some()
        || decoration.block_preview.is_some()
}

fn marker_quads(
    character: char,
    cell: Bounds<Pixels>,
    style: WhitespaceMarkerStyle,
) -> Vec<PaintQuad> {
    let center_y = cell.top() + cell.size.height / 2.0;
    if character == ' ' {
        let dot = px(style.dot_size_px)
            .min(cell.size.width)
            .min(cell.size.height);
        return vec![
            fill(
                Bounds::new(
                    point(
                        cell.left() + (cell.size.width - dot) / 2.0,
                        center_y - dot / 2.0,
                    ),
                    size(dot, dot),
                ),
                style.color,
            )
            .corner_radii(Corners::all(dot / 2.0)),
        ];
    }
    if character != '\t' {
        return Vec::new();
    }
    let stroke = px(style.tab_stroke_px)
        .min(cell.size.width)
        .min(cell.size.height);
    let width = (cell.size.width - stroke * 2.0).max(stroke);
    let left = cell.left() + (cell.size.width - width) / 2.0;
    let right = left + width - stroke;
    let head = px(style.tab_height_px).min(cell.size.height).min(width) / 2.0;
    let head = head
        .min((width - stroke).max(px(0.0)))
        .min((cell.size.height - stroke).max(px(0.0)) / 2.0);
    let mut quads = vec![fill(
        Bounds::new(point(left, center_y - stroke / 2.0), size(width, stroke)),
        style.color,
    )];
    if head <= px(0.0) {
        return quads;
    }
    for step in 1..=2 {
        let offset = head * (step as f32 / 2.0);
        for sign in [-1.0, 1.0] {
            quads.push(fill(
                Bounds::new(
                    point(right - offset, center_y + offset * sign - stroke / 2.0),
                    size(stroke, stroke),
                ),
                style.color,
            ));
        }
    }
    quads
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_markers(
    output: &mut Vec<PaintQuad>,
    text: &str,
    wrapped: &WrappedLine,
    origin: Point<Pixels>,
    line_height: f32,
    style: WhitespaceMarkerStyle,
    skip_prefix_bytes: usize,
    indent: f32,
    visibility: Visibility<'_>,
    clip: Option<Bounds<Pixels>>,
) {
    if !visibility.enabled() {
        return;
    }
    if let Some(clip) = clip
        && (clip.size.width <= px(0.0)
            || clip.size.height <= px(0.0)
            || origin.y > clip.bottom()
            || origin.y + wrapped.size(px(line_height)).height < clip.top())
    {
        return;
    }
    for (index, character) in text.char_indices() {
        if index < skip_prefix_bytes || !matches!(character, ' ' | '\t') || !visibility.at(index) {
            continue;
        }
        let Some(start) =
            super::element::downstream_wrap_boundary_position(wrapped, text, index, line_height)
                .or_else(|| wrapped.position_for_index(index, px(line_height)))
        else {
            continue;
        };
        let width = (wrapped
            .unwrapped_layout
            .x_for_index(index + character.len_utf8())
            - wrapped.unwrapped_layout.x_for_index(index))
        .abs()
        .max(px(style.dot_size_px));
        let row = wrapped.row_for_y(start.y, px(line_height));
        let cell = Bounds::new(
            origin
                + start
                + point(
                    px(super::helpers::indent_for_y(indent, start.y, line_height)),
                    px(0.0),
                ),
            size(width, wrapped.row_height(row, px(line_height))),
        );
        for mut quad in marker_quads(character, cell, style) {
            if let Some(clip) = clip {
                quad.bounds = quad.bounds.intersect(&clip);
            }
            if quad.bounds.size.width > px(0.0) && quad.bounds.size.height > px(0.0) {
                output.push(quad);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_preview_source_and_yaml_force_markers_but_rendered_tables_do_not() {
        let rows = crate::markdown::parse_decorations(
            "---\nname: value\n---\n\n```mermaid\ngraph LR\nA --> B\n```\n\n$$\na + b\n$$\n\n| A | B |\n| --- | --- |\n| a | b |\n",
        );
        assert!(force_line(false, false, &rows[1]));
        assert!(force_line(false, false, &rows[5]));
        assert!(force_line(false, false, &rows[10]));
        assert!(!force_line(false, false, &rows[13]));
        assert!(force_line(true, false, &rows[13]));
    }

    #[test]
    fn collapsed_inline_code_maps_whitespace_back_to_source() {
        let source = "plain text `中 a\tb` tail";
        let rows = crate::markdown::parse_decorations(source);
        let (display, mapping, _, _) = super::super::helpers::build_display_text(
            source,
            &rows[0],
            &[],
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
        let visibility = Visibility {
            all: false,
            decoration: &rows[0],
            display_to_buffer: &mapping,
            display_offset: 0,
        };
        let visible: Vec<_> = display
            .char_indices()
            .filter(|(index, ch)| matches!(ch, ' ' | '\t') && visibility.at(*index))
            .map(|(_, ch)| ch)
            .collect();
        assert_eq!(visible, vec![' ', '\t']);
        assert!(!visibility.at(display.find(' ').unwrap()));
    }

    #[test]
    fn code_and_source_force_whitespace_but_prose_respects_the_setting() {
        let normal = LineDecoration::default();
        assert!(!force_line(false, false, &normal));
        assert!(force_line(true, false, &normal));
        assert!(force_line(false, true, &normal));
        for block_type in [BlockType::CodeBlock, BlockType::CodeBlockFence] {
            let mut row = LineDecoration::default();
            row.block_type = block_type;
            assert!(force_line(false, false, &row));
        }
    }

    #[test]
    fn inline_code_and_math_use_source_mappings_not_entire_paragraphs() {
        let source = "plain space `code space` and $a + b$";
        let rows = crate::markdown::parse_decorations(source);
        let mapping: Vec<_> = (0..source.len()).collect();
        let visibility = Visibility {
            all: false,
            decoration: &rows[0],
            display_to_buffer: &mapping,
            display_offset: 0,
        };
        assert!(visibility.enabled());
        assert!(!visibility.at(5));
        assert!(visibility.at(source.find("code space").unwrap() + 4));
        assert!(visibility.at(source.find("a + b").unwrap() + 1));
        let shifted = Visibility {
            display_offset: source.find("code space").unwrap(),
            ..visibility
        };
        assert!(shifted.at(4));
    }

    #[test]
    fn spaces_are_dots_and_tabs_are_distinct_arrows_with_custom_metrics() {
        let style = WhitespaceMarkerStyle {
            dot_size_px: 3.0,
            tab_height_px: 8.0,
            tab_stroke_px: 1.5,
            color: gpui::black(),
        };
        let cell = Bounds::new(point(px(10.0), px(20.0)), size(px(24.0), px(30.0)));
        let dot = marker_quads(' ', cell, style);
        let tab = marker_quads('\t', cell, style);
        assert_eq!(dot.len(), 1);
        assert_eq!(dot[0].bounds.size, size(px(3.0), px(3.0)));
        assert_eq!(tab.len(), 5);
        assert!(tab[0].bounds.size.width > dot[0].bounds.size.width);
        for quad in tab {
            assert!(quad.bounds.left() >= cell.left());
            assert!(quad.bounds.right() <= cell.right());
            assert!(quad.bounds.top() >= cell.top());
            assert!(quad.bounds.bottom() <= cell.bottom());
        }
        for width in [0.25, 1.0, 2.0, 4.0] {
            let cell = Bounds::new(point(px(0.0), px(0.0)), size(px(width), px(3.0)));
            for quad in marker_quads('\t', cell, style) {
                assert!(quad.bounds.left() >= cell.left());
                assert!(quad.bounds.right() <= cell.right());
                assert!(quad.bounds.top() >= cell.top());
                assert!(quad.bounds.bottom() <= cell.bottom());
            }
        }
    }
}
