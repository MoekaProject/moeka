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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::TableAlign;

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
