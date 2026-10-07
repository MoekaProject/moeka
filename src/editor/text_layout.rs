//! Pure numeric text-layout calculations; no GPUI text shaping or window state.

/// GPUI centers the shaped ascent/descent in a fixed-height row. Translate only
/// the paint origin so content-dependent metrics cannot move the row's baseline.
pub(crate) fn text_baseline_paint_offset(
    line_height: f32,
    reference_baseline: f32,
    shaped_ascent: f32,
    shaped_descent: f32,
) -> f32 {
    reference_baseline - ((line_height - shaped_ascent - shaped_descent) * 0.5 + shaped_ascent)
}

pub(crate) fn unordered_list_marker_origin_x(anchor_x: f32, glyph_ink_left: f32) -> f32 {
    anchor_x - glyph_ink_left
}

pub(crate) fn table_cell_content_offset(
    alignment: crate::markdown::TableAlign,
    inner_width: f32,
    text_width: f32,
) -> f32 {
    let available = (inner_width - text_width).max(0.0);
    match alignment {
        crate::markdown::TableAlign::Left => 0.0,
        crate::markdown::TableAlign::Center => available * 0.5,
        crate::markdown::TableAlign::Right => available,
    }
}

pub(crate) fn code_block_soft_wrap_width(viewport_width: f32, character_advance: f32) -> f32 {
    (viewport_width - character_advance.max(1.0)).max(1.0)
}

/// Formula positions and sorted wrap boundaries are display-text byte offsets.
/// A formula starting exactly at a boundary belongs to the downstream row.
pub(crate) fn inline_math_row_heights(
    base_line_height: f32,
    boundaries: &[usize],
    formulas: &[(usize, f32)],
) -> Vec<f32> {
    let mut heights = vec![base_line_height; boundaries.len() + 1];
    for &(start, height) in formulas {
        let row = boundaries.partition_point(|&boundary| boundary <= start);
        heights[row] = heights[row].max(height);
    }
    heights
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::TableAlign;

    #[test]
    fn inline_math_expands_only_its_wrapped_row() {
        assert_eq!(
            inline_math_row_heights(24.0, &[10, 20], &[(15, 60.0)]),
            vec![24.0, 60.0, 24.0]
        );
        assert_eq!(
            inline_math_row_heights(24.0, &[10, 20], &[(5, 60.0)]),
            vec![60.0, 24.0, 24.0]
        );
    }

    #[test]
    fn inline_math_redistribution_preserves_total_height() {
        let first_row = inline_math_row_heights(24.0, &[10, 20], &[(5, 60.0)]);
        let second_row = inline_math_row_heights(24.0, &[10, 20], &[(10, 60.0)]);
        assert_eq!(first_row, vec![60.0, 24.0, 24.0]);
        assert_eq!(second_row, vec![24.0, 60.0, 24.0]);
        assert_eq!(first_row.iter().sum::<f32>(), 108.0);
        assert_eq!(second_row.iter().sum::<f32>(), 108.0);
    }

    #[test]
    fn inline_math_at_exact_wrap_boundaries_belongs_to_downstream_rows() {
        assert_eq!(
            inline_math_row_heights(24.0, &[10, 20], &[(10, 60.0)]),
            vec![24.0, 60.0, 24.0]
        );
        assert_eq!(
            inline_math_row_heights(24.0, &[10, 20], &[(20, 40.0)]),
            vec![24.0, 24.0, 40.0]
        );
        assert_eq!(
            inline_math_row_heights(24.0, &[10, 20], &[(9, 60.0)]),
            vec![60.0, 24.0, 24.0]
        );
    }

    #[test]
    fn inline_math_takes_the_maximum_only_within_each_row() {
        assert_eq!(
            inline_math_row_heights(
                24.0,
                &[10, 20],
                &[(12, 40.0), (5, 30.0), (18, 60.0), (11, 50.0), (25, 10.0)]
            ),
            vec![30.0, 60.0, 24.0]
        );
    }

    #[test]
    fn inline_math_without_tall_formulas_keeps_base_row_heights() {
        assert_eq!(
            inline_math_row_heights(24.0, &[10, 20], &[]),
            vec![24.0, 24.0, 24.0]
        );
        assert_eq!(
            inline_math_row_heights(24.0, &[10, 20], &[(0, 10.0), (10, 24.0)]),
            vec![24.0, 24.0, 24.0]
        );
    }

    #[test]
    fn inline_math_unwrapped_row_fits_its_tallest_formula() {
        assert_eq!(inline_math_row_heights(24.0, &[], &[]), vec![24.0]);
        assert_eq!(
            inline_math_row_heights(24.0, &[], &[(0, 40.0), (10, 60.0)]),
            vec![60.0]
        );
        assert_eq!(inline_math_row_heights(80.0, &[], &[(0, 60.0)]), vec![80.0]);
    }

    #[test]
    fn first_cjk_run_does_not_move_the_latin_baseline() {
        // A 20px row with Latin ascent/descent 11/3 has a baseline at 14px.
        assert_eq!(text_baseline_paint_offset(20.0, 14.0, 11.0, 3.0), 0.0);
        // A taller CJK ascent used to move all Latin glyphs down by 1px.
        assert_eq!(text_baseline_paint_offset(20.0, 14.0, 13.0, 3.0), -1.0);
        // A larger descent needs the opposite correction, not a fixed -1px.
        assert_eq!(text_baseline_paint_offset(20.0, 14.0, 11.0, 5.0), 1.0);
        assert_eq!(text_baseline_paint_offset(20.0, 14.0, 13.0, 5.0), 0.0);
    }

    #[test]
    fn mixed_wrapped_segments_keep_the_same_baseline_and_row_pitch() {
        for scale in [0.75, 1.0, 1.25, 2.0] {
            let line_height = 20.0 * scale;
            let reference_baseline = 14.0 * scale;
            for (row, (ascent, descent)) in [(11.0, 3.0), (13.0, 3.0), (11.0, 5.0)]
                .into_iter()
                .enumerate()
            {
                let row_top = 100.0 + row as f32 * line_height;
                let ascent = ascent * scale;
                let descent = descent * scale;
                let paint_top = row_top
                    + text_baseline_paint_offset(line_height, reference_baseline, ascent, descent);
                let painted_baseline = paint_top + (line_height - ascent - descent) * 0.5 + ascent;
                assert_eq!(painted_baseline, row_top + reference_baseline);
            }
        }
    }

    #[test]
    fn unordered_list_markers_align_visible_left_edges_at_each_level() {
        for scale in [0.75, 1.0, 1.5, 2.0] {
            let content_left = 100.0;
            let parent_prefix_width = 12.0 * scale;
            for (level, bearing) in [3.0, 4.0, 2.0].into_iter().enumerate() {
                let anchor = content_left + level as f32 * parent_prefix_width;
                let bearing = bearing * scale;
                let origin = unordered_list_marker_origin_x(anchor, bearing);
                assert_eq!(origin + bearing, anchor);
                if level > 0 {
                    let parent_text_left = content_left + level as f32 * parent_prefix_width;
                    assert_eq!(origin + bearing, parent_text_left);
                }
            }
        }
        assert_eq!(unordered_list_marker_origin_x(100.0, -2.0), 102.0);
    }

    #[test]
    fn narrow_widths_preserve_existing_numeric_limits() {
        for width in [0.0, 4.0, 8.0, 8.5] {
            assert_eq!(code_block_soft_wrap_width(width, 8.0), 1.0);
        }
        assert_eq!(code_block_soft_wrap_width(100.0, 8.0), 92.0);
        assert_eq!(code_block_soft_wrap_width(10.0, 0.0), 9.0);
    }

    #[test]
    fn table_offsets_use_nonnegative_remaining_width() {
        assert_eq!(
            table_cell_content_offset(TableAlign::Left, 100.0, 40.0),
            0.0
        );
        assert_eq!(
            table_cell_content_offset(TableAlign::Center, 100.0, 40.0),
            30.0
        );
        assert_eq!(
            table_cell_content_offset(TableAlign::Right, 100.0, 40.0),
            60.0
        );
        for alignment in [TableAlign::Left, TableAlign::Center, TableAlign::Right] {
            assert_eq!(table_cell_content_offset(alignment, 40.0, 40.0), 0.0);
            assert_eq!(table_cell_content_offset(alignment, 40.0, 100.0), 0.0);
        }
    }
}
