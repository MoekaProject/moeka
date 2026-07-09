use std::collections::HashMap;
use std::ops::Range;

use gpui::{Pixels, Point, point, px};

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

#[cfg(test)]
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

    pub(crate) fn visual_position_for_wrapped_display(
        wrapped_line: &WrappedLine,
        display_text: &str,
        disp_col: usize,
        line_height: f32,
        indent_px: f32,
        wrap_boundary_upstream_xs: &HashMap<usize, f32>,
        affinity: Option<WrapBoundaryAffinityKind>,
    ) -> Option<Point<Pixels>> {
        if let Some(upstream_visual_idx) = helpers::wrap_boundary_byte_indices(wrapped_line)
            .iter()
            .position(|boundary| *boundary == disp_col)
        {
            let downstream_visual_idx = upstream_visual_idx + 1;
            if affinity == Some(WrapBoundaryAffinityKind::Downstream) {
                let downstream_x = if downstream_visual_idx > 0 {
                    indent_px
                } else {
                    0.0
                };
                return Some(point(
                    px(downstream_x),
                    px(downstream_visual_idx as f32 * line_height),
                ));
            }

            if let Some(upstream_x) = wrap_boundary_upstream_xs.get(&disp_col).copied() {
                let previous_boundary = display_text[..disp_col]
                    .char_indices()
                    .next_back()
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                let previous_pos =
                    wrapped_line.position_for_index(previous_boundary, px(line_height))?;
                let upstream_x = upstream_x
                    + if upstream_visual_idx > 0 {
                        indent_px
                    } else {
                        0.0
                    };
                return Some(point(px(upstream_x), previous_pos.y));
            }
        }

        let pos = wrapped_line
            .position_for_index(disp_col, px(line_height))
            .unwrap_or_default();
        Some(point(
            px(f32::from(pos.x) + helpers::indent_for_y(indent_px, pos.y, line_height)),
            pos.y,
        ))
    }

    pub(crate) fn cursor_visual_position(&self, offset: usize) -> Option<(usize, f32)> {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
        let affinity = self
            .wrap_boundary_affinity
            .and_then(|current| (current.offset == offset).then_some(current.kind));
        let pos = Self::visual_position_for_wrapped_display(
            &info.wrapped_line,
            &info.display_text,
            disp_col,
            info.line_height,
            info.indent_px,
            &info.wrap_boundary_upstream_xs,
            affinity,
        )?;
        Some((
            (f32::from(pos.y) / info.line_height) as usize,
            f32::from(pos.x),
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
        let text_top_y = self.text_y_offset_for_line(line);
        let local_y = (y - text_top_y).max(0.0);
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
        self.buffer.offset_for_utf16_offset(utf16_offset)
    }

    pub(crate) fn offset_to_utf16(&self, utf8_offset: usize) -> usize {
        self.buffer.utf16_offset_for_offset(utf8_offset)
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
    use super::{utf8_offset_in_text_for_utf16, utf16_offset_in_text_for_utf8};

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
