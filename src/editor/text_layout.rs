//! Pure numeric text-layout calculations; no GPUI text shaping or window state.

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

pub(crate) fn effective_wrap_width_for_letter_spacing(
    width: f32,
    font_size: f32,
    letter_spacing: f32,
) -> f32 {
    if letter_spacing > 0.0 {
        width * font_size / (font_size + letter_spacing)
    } else {
        width
    }
}

pub(crate) fn effective_monospace_wrap_width_for_letter_spacing(
    width: f32,
    character_advance: f32,
    letter_spacing: f32,
) -> f32 {
    if letter_spacing > 0.0 && character_advance > 0.0 {
        width * character_advance / (character_advance + letter_spacing)
    } else {
        width
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
    fn zero_spacing_preserves_wrap_width() {
        for width in [0.0, 0.5, 150.0] {
            assert_eq!(
                effective_wrap_width_for_letter_spacing(width, 15.0, 0.0),
                width
            );
            assert_eq!(
                effective_monospace_wrap_width_for_letter_spacing(width, 8.0, 0.0),
                width
            );
        }
    }

    #[test]
    fn monospace_spacing_uses_supplied_actual_advance() {
        assert_eq!(
            effective_monospace_wrap_width_for_letter_spacing(90.0, 8.0, 2.0),
            72.0
        );
        assert_eq!(
            effective_wrap_width_for_letter_spacing(90.0, 16.0, 2.0),
            80.0
        );
        assert_eq!(
            effective_monospace_wrap_width_for_letter_spacing(90.0, 0.0, 2.0),
            90.0
        );
    }

    #[test]
    fn narrow_widths_preserve_existing_numeric_limits() {
        assert_eq!(effective_wrap_width_for_letter_spacing(0.5, 8.0, 2.0), 0.4);
        assert_eq!(
            effective_monospace_wrap_width_for_letter_spacing(0.5, 8.0, 2.0),
            0.4
        );
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
