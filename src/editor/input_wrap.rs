use std::ops::Range;

use gpui::{Pixels, Point, px};

use super::*;

fn utf8_offset_in_text_for_utf16(text: &str, utf16_offset: usize) -> usize {
    let mut utf8_offset = 0usize;
    let mut utf16_count = 0usize;

    for ch in text.chars() {
        if utf16_count >= utf16_offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }

    utf8_offset.min(text.len())
}

fn utf16_offset_in_text_for_utf8(text: &str, utf8_offset: usize) -> usize {
    let mut utf16_offset = 0usize;
    let mut utf8_count = 0usize;

    for ch in text.chars() {
        if utf8_count >= utf8_offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }

    utf16_offset
}

impl EditorView {
    pub(crate) fn wrap_boundary_display_index_for_visual_line(
        info: &LineDisplayInfo,
        visual_line_idx: usize,
    ) -> Option<usize> {
        if visual_line_idx == 0 {
            return Some(0);
        }
        helpers::wrap_boundary_byte_indices(&info.wrapped_line)
            .get(visual_line_idx - 1)
            .copied()
    }

    fn wrap_boundary_visual_indices(
        info: &LineDisplayInfo,
        disp_col: usize,
    ) -> Option<(usize, usize)> {
        helpers::wrap_boundary_byte_indices(&info.wrapped_line)
            .iter()
            .position(|boundary| *boundary == disp_col)
            .map(|upstream_visual_idx| (upstream_visual_idx, upstream_visual_idx + 1))
    }

    fn wrap_boundary_visual_x_pair(info: &LineDisplayInfo, disp_col: usize) -> Option<(f32, f32)> {
        let (upstream_visual_idx, downstream_visual_idx) =
            Self::wrap_boundary_visual_indices(info, disp_col)?;
        let upstream_x = info.wrap_boundary_upstream_xs.get(&disp_col).copied()?
            + if upstream_visual_idx > 0 {
                info.indent_px
            } else {
                0.0
            };
        let downstream_x = if downstream_visual_idx > 0 {
            info.indent_px
        } else {
            0.0
        };
        Some((upstream_x, downstream_x))
    }

    pub(crate) fn cursor_visual_position(&self, offset: usize) -> Option<(usize, f32)> {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);

        if let Some((upstream_visual_idx, downstream_visual_idx)) =
            Self::wrap_boundary_visual_indices(info, disp_col)
        {
            if self.wrap_boundary_affinity
                == Some(WrapBoundaryAffinity {
                    offset,
                    kind: WrapBoundaryAffinityKind::Downstream,
                })
            {
                let downstream_x = if downstream_visual_idx > 0 {
                    info.indent_px
                } else {
                    0.0
                };
                return Some((downstream_visual_idx, downstream_x));
            }

            if let Some((upstream_x, _)) = Self::wrap_boundary_visual_x_pair(info, disp_col) {
                return Some((upstream_visual_idx, upstream_x));
            }
        }

        let pos = info
            .wrapped_line
            .position_for_index(disp_col, px(info.line_height))
            .unwrap_or_default();
        Some((
            (f32::from(pos.y) / info.line_height) as usize,
            f32::from(pos.x) + helpers::indent_for_y(info.indent_px, pos.y, info.line_height),
        ))
    }

    pub(crate) fn cursor_visual_x(&self, offset: usize) -> Option<f32> {
        self.cursor_visual_position(offset).map(|(_, x)| x)
    }

    pub(crate) fn is_wrap_boundary_offset(&self, offset: usize) -> bool {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(Some(info)) = self.line_display_info.get(line) else {
            return false;
        };

        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        Self::wrap_boundary_visual_indices(info, disp_col).is_some()
    }

    pub(crate) fn update_wrap_boundary_affinity_for_position(
        &mut self,
        offset: usize,
        position: Point<Pixels>,
    ) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(Some(info)) = self.line_display_info.get(line) else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let Some((upstream_visual_idx, downstream_visual_idx)) =
            Self::wrap_boundary_visual_indices(info, disp_col)
        else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        let pos_y: f32 = position.y.into();
        let origin_y: f32 = self.content_origin.y.into();
        let y = (pos_y - origin_y) + self.scroll_offset;
        let line_top_y = self.line_y_offset(line);
        let local_y = (y - line_top_y).max(0.0);
        let visual_line_idx = (local_y / info.line_height) as usize;

        if visual_line_idx >= downstream_visual_idx {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Downstream);
        } else if visual_line_idx <= upstream_visual_idx {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Upstream);
        } else {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Downstream);
        }
    }

    pub(crate) fn update_wrap_boundary_affinity_for_goal_x(&mut self, offset: usize, goal_x: f32) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(Some(info)) = self.line_display_info.get(line) else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let Some((upstream_x, downstream_x)) = Self::wrap_boundary_visual_x_pair(info, disp_col)
        else {
            self.clear_wrap_boundary_affinity();
            return;
        };

        if (goal_x - downstream_x).abs() <= (goal_x - upstream_x).abs() {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Downstream);
        } else {
            self.set_wrap_boundary_affinity(offset, WrapBoundaryAffinityKind::Upstream);
        }
    }

    pub(crate) fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        utf8_offset_in_text_for_utf16(self.buffer.text(), utf16_offset)
    }

    pub(crate) fn offset_to_utf16(&self, utf8_offset: usize) -> usize {
        utf16_offset_in_text_for_utf8(self.buffer.text(), utf8_offset)
    }

    pub(crate) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(crate) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    pub(crate) fn utf8_offset_in_text_for_utf16(text: &str, utf16_offset: usize) -> usize {
        utf8_offset_in_text_for_utf16(text, utf16_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::{utf16_offset_in_text_for_utf8, utf8_offset_in_text_for_utf16};

    #[test]
    fn utf16_to_utf8_offset_handles_multibyte_chars() {
        let text = "a中🙂b";

        assert_eq!(utf8_offset_in_text_for_utf16(text, 0), 0);
        assert_eq!(utf8_offset_in_text_for_utf16(text, 1), 1);
        assert_eq!(utf8_offset_in_text_for_utf16(text, 2), 4);
        assert_eq!(utf8_offset_in_text_for_utf16(text, 4), 8);
        assert_eq!(utf8_offset_in_text_for_utf16(text, 5), 9);
    }

    #[test]
    fn utf8_to_utf16_offset_handles_multibyte_chars() {
        let text = "a中🙂b";

        assert_eq!(utf16_offset_in_text_for_utf8(text, 0), 0);
        assert_eq!(utf16_offset_in_text_for_utf8(text, 1), 1);
        assert_eq!(utf16_offset_in_text_for_utf8(text, 4), 2);
        assert_eq!(utf16_offset_in_text_for_utf8(text, 8), 4);
        assert_eq!(utf16_offset_in_text_for_utf8(text, 9), 5);
    }
}
