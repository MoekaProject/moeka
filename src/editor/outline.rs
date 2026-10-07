use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, SharedString, div, prelude::*,
    px, svg,
};
use std::cell::Ref;

use crate::markdown::BlockType;

use super::*;

#[derive(Clone)]
pub(crate) struct OutlineHeading {
    pub line: usize,
    pub level: u8,
    pub text: SharedString,
}

#[derive(Clone, Copy, PartialEq)]
struct OutlinePanelLayoutKey {
    epoch: usize,
    panel_width: f32,
    section_padding_x: f32,
    item_min_height: f32,
    item_padding_y: f32,
    line_height: f32,
    font_size: f32,
    indent_step: f32,
    fold_icon_hit_size: f32,
    fold_icon_gap: f32,
    scrollbar_reserve: f32,
}

#[derive(Default)]
pub(crate) struct OutlinePanelLayoutCache {
    key: Option<OutlinePanelLayoutKey>,
    visible_heading_indices: Vec<(usize, usize)>,
    item_heights: Vec<f32>,
    visible_expanded: Vec<bool>,
    has_later_peer: Vec<bool>,
    has_prior_expanded_peer: Vec<bool>,
    offsets: Vec<f32>,
}

impl OutlinePanelLayoutCache {
    fn rebuild_if_needed(
        &mut self,
        key: OutlinePanelLayoutKey,
        headings: &[OutlineHeading],
        collapsed_lines: &std::collections::HashSet<usize>,
        mut row_height: impl FnMut(&OutlineHeading, usize) -> f32,
    ) {
        if self.key == Some(key) {
            return;
        }

        let visible_heading_indices =
            visible_outline_heading_indices_with_depths(headings, collapsed_lines);
        let mut item_heights = Vec::with_capacity(visible_heading_indices.len());
        let mut visible_expanded = Vec::with_capacity(visible_heading_indices.len());
        let mut offsets = Vec::with_capacity(visible_heading_indices.len() + 1);
        offsets.push(0.0);
        for (heading_index, depth) in &visible_heading_indices {
            let heading = &headings[*heading_index];
            let height = row_height(heading, *depth);
            item_heights.push(height);
            visible_expanded.push(
                outline_heading_has_children(headings, *heading_index)
                    && !collapsed_lines.contains(&heading.line),
            );
            offsets.push(offsets.last().copied().unwrap_or(0.0) + height);
        }

        let mut has_later_peer = vec![false; visible_heading_indices.len()];
        let mut next_peer_at_depth = Vec::<bool>::new();
        for (visible_index, (_, depth)) in visible_heading_indices.iter().enumerate().rev() {
            next_peer_at_depth.truncate(depth + 1);
            if next_peer_at_depth.len() <= *depth {
                next_peer_at_depth.resize(depth + 1, false);
            }
            has_later_peer[visible_index] = next_peer_at_depth[*depth];
            next_peer_at_depth[*depth] = true;
        }

        let mut has_prior_expanded_peer = vec![false; visible_heading_indices.len()];
        let mut expanded_seen_at_depth = Vec::<bool>::new();
        for (visible_index, (_, depth)) in visible_heading_indices.iter().enumerate() {
            expanded_seen_at_depth.truncate(depth + 1);
            if expanded_seen_at_depth.len() <= *depth {
                expanded_seen_at_depth.resize(depth + 1, false);
            }
            has_prior_expanded_peer[visible_index] = expanded_seen_at_depth[*depth];
            expanded_seen_at_depth[*depth] |= visible_expanded[visible_index];
        }

        self.key = Some(key);
        self.visible_heading_indices = visible_heading_indices;
        self.item_heights = item_heights;
        self.visible_expanded = visible_expanded;
        self.has_later_peer = has_later_peer;
        self.has_prior_expanded_peer = has_prior_expanded_peer;
        self.offsets = offsets;
    }
}

#[derive(Clone, Default)]
pub(crate) struct MarkdownOutlineIndex {
    pub(crate) headings: Vec<OutlineHeading>,
    fence_before_line: Vec<u16>,
}

impl MarkdownOutlineIndex {
    pub(crate) fn rebuild(buffer: &Buffer) -> Self {
        let mut index = Self {
            headings: Vec::new(),
            fence_before_line: vec![0; buffer.line_count() + 1],
        };
        let mut state = 0;
        for line in 0..buffer.line_count() {
            index.fence_before_line[line] = state;
            state = next_fence_state(buffer.line_content(line), state);
            if let Some(heading) = outline_heading(buffer, line, state) {
                index.headings.push(heading);
            }
        }
        index.fence_before_line[buffer.line_count()] = state;
        index
    }

    pub(crate) fn update_after_edit(
        &mut self,
        buffer: &Buffer,
        start_line: usize,
        old_end_line_exclusive: usize,
        new_end_line_exclusive: usize,
    ) {
        let old_line_count = self.fence_before_line.len().saturating_sub(1);
        let line_delta = new_end_line_exclusive as isize - old_end_line_exclusive as isize;
        let start = start_line.min(buffer.line_count());
        let old_end = old_end_line_exclusive.min(old_line_count).max(start);
        let new_end = new_end_line_exclusive.min(buffer.line_count()).max(start);

        let old_inner_start = (start + 1).min(self.fence_before_line.len());
        let old_inner_end = old_end.min(self.fence_before_line.len());
        let new_inner_count = new_end.saturating_sub(start + 1);
        self.fence_before_line.splice(
            old_inner_start..old_inner_end,
            std::iter::repeat_n(0, new_inner_count),
        );
        self.fence_before_line.resize(buffer.line_count() + 1, 0);

        for heading in &mut self.headings {
            if heading.line >= old_end {
                heading.line = (heading.line as isize + line_delta).max(0) as usize;
            }
        }

        let scan_start = start.saturating_sub(1);
        let mut scan_end = new_end.max(scan_start + 1).min(buffer.line_count());
        let mut state = self.fence_before_line[scan_start];
        let mut line = scan_start;
        while line < buffer.line_count() {
            self.fence_before_line[line] = state;
            let next = next_fence_state(buffer.line_content(line), state);
            let previous_next = self.fence_before_line[line + 1];
            self.fence_before_line[line + 1] = next;
            line += 1;
            state = next;
            scan_end = line;
            if line >= new_end && next == previous_next {
                break;
            }
        }

        self.headings
            .retain(|heading| heading.line < scan_start || heading.line >= scan_end);
        let replacement = (scan_start..scan_end)
            .filter_map(|line| outline_heading(buffer, line, self.fence_before_line[line + 1]))
            .collect::<Vec<_>>();
        let insert_at = self
            .headings
            .partition_point(|heading| heading.line < scan_start);
        self.headings.splice(insert_at..insert_at, replacement);
    }
}

fn structural_line_content(content: &str) -> Option<&str> {
    let leading = content.bytes().take_while(|byte| *byte == b' ').count();
    if leading > 3 {
        return None;
    }
    let mut content = &content[leading..];
    while let Some(rest) = content.strip_prefix('>') {
        content = rest.strip_prefix([' ', '\t']).unwrap_or(rest);
    }
    Some(content)
}

fn atx_outline_heading(content: &str) -> Option<(u8, &str)> {
    let content = structural_line_content(content)?;
    let marker_len = content.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&marker_len)
        || !content
            .as_bytes()
            .get(marker_len)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        return None;
    }
    let text = content[marker_len..]
        .trim()
        .trim_end_matches('#')
        .trim_end();
    Some((marker_len as u8, text))
}

fn next_fence_state(content: &str, state: u16) -> u16 {
    let Some(content) = structural_line_content(content) else {
        return state;
    };
    let marker = content.as_bytes().first().copied().unwrap_or_default();
    if !matches!(marker, b'`' | b'~') {
        return state;
    }
    let marker_len = content.bytes().take_while(|byte| *byte == marker).count();
    if marker_len < 3 {
        return state;
    }
    let encoded = u16::from(marker == b'~') << 8 | marker_len.min(255) as u16;
    if state == 0 {
        encoded
    } else {
        let state_marker = if state >> 8 == 0 { b'`' } else { b'~' };
        let state_len = usize::from(state & 0xff);
        if marker == state_marker
            && marker_len >= state_len
            && content[marker_len..].trim().is_empty()
        {
            0
        } else {
            state
        }
    }
}

fn outline_heading(buffer: &Buffer, line: usize, state_after_line: u16) -> Option<OutlineHeading> {
    if state_after_line != 0 {
        return None;
    }
    let content = buffer.line_content(line);
    if let Some((level, text)) = atx_outline_heading(content) {
        return Some(OutlineHeading {
            line,
            level,
            text: SharedString::from(text.to_string()),
        });
    }

    let trimmed = structural_line_content(content)?.trim();
    let setext_level = if trimmed.len() >= 3 && trimmed.bytes().all(|byte| byte == b'=') {
        Some(1)
    } else if trimmed.len() >= 3 && trimmed.bytes().all(|byte| byte == b'-') {
        Some(2)
    } else {
        None
    };
    let level = setext_level?;
    let text_line = line.checked_sub(1)?;
    let text = structural_line_content(buffer.line_content(text_line))?.trim();
    let previous_is_list_item = text
        .as_bytes()
        .first()
        .is_some_and(|marker| matches!(marker, b'-' | b'*' | b'+'))
        && text
            .as_bytes()
            .get(1)
            .is_some_and(|separator| matches!(separator, b' ' | b'\t'))
        || crate::markdown::ordered_list_prefix_len(text) > 0;
    if previous_is_list_item {
        return None;
    }
    (!text.is_empty()).then(|| OutlineHeading {
        line: text_line,
        level,
        text: SharedString::from(text.to_string()),
    })
}

fn outline_rail_row_top(
    row_index: usize,
    row_count: usize,
    viewport_height: f32,
    row_height: f32,
    tick_thickness: f32,
) -> f32 {
    let progress = if row_count <= 1 {
        0.0
    } else {
        row_index.min(row_count - 1) as f32 / (row_count - 1) as f32
    };
    let tick_travel = (viewport_height - tick_thickness).max(0.0);
    let tick_inset = (row_height - tick_thickness).max(0.0) / 2.0;
    progress * tick_travel - tick_inset
}

fn outline_rail_compact_row_top(
    row_index: usize,
    row_count: usize,
    viewport_height: f32,
    row_height: f32,
) -> f32 {
    let group_height = row_count as f32 * row_height;
    let group_top = (viewport_height - group_height).max(0.0) / 2.0;
    group_top + row_index as f32 * row_height
}

fn outline_rail_slot_count(viewport_height: f32, row_height: f32) -> usize {
    if viewport_height <= 0.0 {
        return 0;
    }
    (viewport_height / row_height.max(1.0)).ceil() as usize
}

fn outline_rail_render_count(heading_count: usize, viewport_height: f32, row_height: f32) -> usize {
    heading_count.min(outline_rail_slot_count(viewport_height, row_height))
}

fn outline_rail_inner_height(viewport_height: f32, vertical_padding: f32) -> f32 {
    (viewport_height - vertical_padding.max(0.0) * 2.0).max(0.0)
}

fn outline_rail_project_index(index: usize, source_count: usize, target_count: usize) -> usize {
    if source_count <= 1 || target_count <= 1 {
        return 0;
    }
    let progress = index.min(source_count - 1) as f32 / (source_count - 1) as f32;
    (progress * (target_count - 1) as f32).round() as usize
}

fn outline_text_columns(text: &str) -> usize {
    text.chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum()
}

fn outline_heading_text_width(text: &str, font_size: f32) -> f32 {
    let columns = outline_text_columns(text);
    columns.max(1) as f32 * font_size.max(1.0) * 0.56 + font_size.max(1.0) * 0.35
}

fn normalized_outline_width_bounds(min_width: f32, max_width: f32) -> (f32, f32) {
    let min_width = if min_width.is_finite() {
        min_width.max(0.0)
    } else {
        0.0
    };
    let max_width = if max_width.is_finite() {
        max_width.max(0.0)
    } else {
        min_width
    };
    (min_width.min(max_width), min_width.max(max_width))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::hint::black_box;
    use std::time::Instant;

    use crate::buffer::Buffer;

    use super::{
        MarkdownOutlineIndex, OutlineHeading, OutlinePanelLayoutCache, OutlinePanelLayoutKey,
        animated_outline_tick_influence, invalidate_outline_navigation,
        normalized_outline_width_bounds, outline_active_line, outline_has_later_peer,
        outline_has_prior_expanded_peer, outline_heading_at_y, outline_hover_animation_value,
        outline_navigation_in_progress, outline_nearest_guide_ranges, outline_rail_compact_row_top,
        outline_rail_inner_height, outline_rail_project_index, outline_rail_render_count,
        outline_rail_row_top, outline_rail_slot_count, outline_scroll_target,
        outline_settled_frames, outline_tick_influence,
    };

    #[test]
    fn markdown_outline_index_skips_fences_and_supports_all_heading_forms() {
        let buffer =
            Buffer::from_text("# ATX\n\n> ## Quoted\n\nSetext\n---\n\n```md\n# hidden\n```\n");
        let index = MarkdownOutlineIndex::rebuild(&buffer);
        let headings = index
            .headings
            .iter()
            .map(|heading| (heading.line, heading.level, heading.text.as_ref()))
            .collect::<Vec<_>>();
        assert_eq!(
            headings,
            vec![(0, 1, "ATX"), (2, 2, "Quoted"), (4, 2, "Setext")]
        );
    }

    #[test]
    fn markdown_outline_index_updates_only_the_edited_region() {
        let mut buffer = Buffer::from_text("# One\n\ntext\n\n## Two\n");
        let mut index = MarkdownOutlineIndex::rebuild(&buffer);
        let start = buffer.line_start(2);
        let end = buffer.line_end(2);
        buffer.replace(start..end, "### Middle");
        index.update_after_edit(&buffer, 2, 3, 3);

        let headings = index
            .headings
            .iter()
            .map(|heading| (heading.line, heading.level, heading.text.as_ref()))
            .collect::<Vec<_>>();
        assert_eq!(
            headings,
            vec![(0, 1, "One"), (2, 3, "Middle"), (4, 2, "Two")]
        );
    }

    #[test]
    fn outline_active_heading_tracks_the_section_at_the_viewport_top() {
        let headings = [0, 10, 20].map(|line| OutlineHeading {
            line,
            level: 1,
            text: "Heading".into(),
        });
        // The viewport remains fixed while body text edits move the next heading
        // across its bottom edge; the active section must not change.
        for next_y in [180.0, 220.0] {
            assert_eq!(
                outline_heading_at_y(&headings, 140.0, |line| match line {
                    0 => 20.0,
                    10 => 100.0,
                    _ => next_y,
                }),
                Some(10),
            );
        }
        assert_eq!(
            outline_heading_at_y(&headings, 100.0, |line| line as f32 * 10.0),
            Some(10)
        );
        assert_eq!(
            outline_heading_at_y(&headings, 0.0, |line| line as f32 * 10.0 + 20.0),
            Some(0)
        );
        assert_eq!(outline_heading_at_y(&[], 100.0, |_| 0.0), None);
    }

    #[test]
    fn outline_click_override_expires_when_editing_another_line() {
        let headings = [
            OutlineHeading {
                line: 10,
                level: 1,
                text: "One".into(),
            },
            OutlineHeading {
                line: 20,
                level: 1,
                text: "Two".into(),
            },
        ];
        assert_eq!(
            outline_active_line(Some(20), 20, &headings, Some(10)),
            Some(20)
        );
        assert_eq!(
            outline_active_line(Some(20), 11, &headings, Some(10)),
            Some(10)
        );
        assert_eq!(
            outline_active_line(Some(30), 30, &headings, Some(10)),
            Some(10)
        );
    }

    #[test]
    fn outline_scroll_target_rechecks_geometry_after_lazy_preview_height_changes() {
        assert_eq!(outline_scroll_target(480.0, 40.0, 1200.0), 440.0);
        assert_eq!(outline_scroll_target(680.0, 40.0, 1200.0), 640.0);
        assert_eq!(outline_scroll_target(680.0, 40.0, 500.0), 500.0);
    }

    #[test]
    fn preview_refresh_does_not_cancel_only_the_current_outline_navigation() {
        assert!(outline_navigation_in_progress(Some(4), 4));
        assert!(!outline_navigation_in_progress(Some(4), 5));
        assert!(!outline_navigation_in_progress(None, 4));
    }

    #[test]
    fn user_navigation_cancels_outline_animation_and_confirmation() {
        let mut scroll_epoch = 4;
        let mut navigation_epoch = Some(4);
        invalidate_outline_navigation(&mut scroll_epoch, &mut navigation_epoch);
        assert_eq!(scroll_epoch, 5);
        assert_eq!(navigation_epoch, None);
        assert!(!outline_navigation_in_progress(
            navigation_epoch,
            scroll_epoch
        ));

        // An already-cancelled task must not invalidate a newer scroll animation.
        navigation_epoch = Some(4);
        invalidate_outline_navigation(&mut scroll_epoch, &mut navigation_epoch);
        assert_eq!(scroll_epoch, 5);
        assert_eq!(navigation_epoch, None);
    }

    #[test]
    fn outline_navigation_waits_for_visible_lazy_previews_to_settle() {
        assert_eq!(outline_settled_frames(0, true), 0);
        assert_eq!(outline_settled_frames(0, false), 1);
        assert_eq!(outline_settled_frames(1, true), 0);
        assert_eq!(outline_settled_frames(1, false), 2);
    }

    #[test]
    fn outline_width_bounds_are_normalized_before_clamp() {
        assert_eq!(
            normalized_outline_width_bounds(324.0, 316.0),
            (316.0, 324.0)
        );
        assert_eq!(
            normalized_outline_width_bounds(f32::NAN, 316.0),
            (0.0, 316.0)
        );
        assert_eq!(
            normalized_outline_width_bounds(324.0, f32::NAN),
            (324.0, 324.0)
        );
    }

    #[test]
    fn outline_resize_minimum_is_independent_of_preferred_width() {
        for scale in [0.8, 1.0, 1.6] {
            let min_width = 10.0 * scale;
            let max_width = 420.0 * scale;
            let (min_width, max_width) = normalized_outline_width_bounds(min_width, max_width);
            for preferred_width in [150.0, 240.0, 380.0] {
                let preferred_width = preferred_width * scale;
                assert_eq!(0.0_f32.clamp(min_width, max_width), 10.0 * scale);
                assert_eq!(preferred_width.clamp(min_width, max_width), preferred_width);
            }
            assert_eq!(f32::MAX.clamp(min_width, max_width), 420.0 * scale);
        }
    }

    #[test]
    fn outline_rail_respects_vertical_padding_without_scrolling() {
        let row_height = 6.0;
        let tick_thickness = 2.0;
        let viewport_height = 100.0;
        let vertical_padding = 20.0;
        let inner_height = outline_rail_inner_height(viewport_height, vertical_padding);
        let tick_inset = (row_height - tick_thickness) / 2.0;

        let first_top =
            vertical_padding + outline_rail_row_top(0, 3, inner_height, row_height, tick_thickness);
        let middle_top =
            vertical_padding + outline_rail_row_top(1, 3, inner_height, row_height, tick_thickness);
        let last_top =
            vertical_padding + outline_rail_row_top(2, 3, inner_height, row_height, tick_thickness);

        assert_eq!(first_top + tick_inset, vertical_padding);
        assert_eq!(middle_top + row_height / 2.0, viewport_height / 2.0);
        assert_eq!(
            last_top + tick_inset + tick_thickness,
            viewport_height - vertical_padding
        );
    }

    #[test]
    fn outline_rail_line_count_depends_only_on_height_and_spacing() {
        let slot_count = outline_rail_slot_count(800.0, 6.0);

        assert_eq!(slot_count, 134);
        assert_eq!(outline_rail_slot_count(800.0, 6.0), slot_count);
        assert_eq!(outline_rail_slot_count(400.0, 6.0), 67);
    }

    #[test]
    fn outline_rail_uses_heading_count_until_capacity_is_exceeded() {
        assert_eq!(outline_rail_render_count(5, 800.0, 6.0), 5);
        assert_eq!(outline_rail_render_count(134, 800.0, 6.0), 134);
        assert_eq!(outline_rail_render_count(135, 800.0, 6.0), 134);
        assert_eq!(outline_rail_render_count(50_000, 400.0, 6.0), 67);
        assert_eq!(outline_rail_render_count(5, 0.0, 6.0), 0);

        for index in 0..5 {
            assert_eq!(outline_rail_project_index(index, 5, 5), index);
        }
    }

    #[test]
    fn outline_rail_keeps_fixed_spacing_when_headings_fit() {
        let row_height = 6.0;
        let tick_thickness = 2.0;
        let viewport_height = 60.0;
        let row_count = 3;
        let tick_inset = (row_height - tick_thickness) / 2.0;
        let centers = (0..row_count)
            .map(|index| {
                outline_rail_compact_row_top(index, row_count, viewport_height, row_height)
                    + tick_inset
            })
            .collect::<Vec<_>>();

        assert_eq!(centers, vec![23.0, 29.0, 35.0]);
        assert_eq!(centers[1] - centers[0], row_height);
        assert_eq!(centers[2] - centers[1], row_height);
        assert_eq!((centers[0] + centers[2] + tick_thickness) / 2.0, 30.0);
    }

    #[test]
    fn outline_headings_project_onto_fixed_rail_slots() {
        let slot_count = outline_rail_slot_count(800.0, 6.0);

        assert_eq!(outline_rail_project_index(0, 50_000, slot_count), 0);
        assert_eq!(outline_rail_project_index(25_000, 50_000, slot_count), 67);
        assert_eq!(
            outline_rail_project_index(49_999, 50_000, slot_count),
            slot_count - 1
        );
        assert_eq!(outline_rail_project_index(0, 1, slot_count), 0);
    }

    #[test]
    fn outline_guides_continue_only_to_later_peers() {
        let visible = [(0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 1)];
        assert!(outline_has_later_peer(&visible, 1, 1));
        assert!(outline_has_later_peer(&visible, 3, 2));
        assert!(!outline_has_later_peer(&visible, 4, 2));
        assert!(!outline_has_later_peer(&visible, 5, 1));
    }

    #[test]
    fn expanded_outline_item_hands_guide_to_the_child_column() {
        assert_eq!(
            outline_nearest_guide_ranges(24.0, 7.0, 2.0, true, true),
            vec![(0.0, 5.0)]
        );
        assert_eq!(
            outline_nearest_guide_ranges(24.0, 7.0, 2.0, false, true),
            vec![(0.0, 12.0), (12.0, 24.0)]
        );
        assert_eq!(
            outline_nearest_guide_ranges(24.0, 7.0, 2.0, false, false),
            vec![(0.0, 12.0)]
        );
    }

    #[test]
    fn old_guide_does_not_restart_after_an_expanded_peer() {
        let visible = [
            (0, 0),
            (1, 1),
            (2, 1),
            (3, 2),
            (4, 2),
            (5, 1),
            (6, 0),
            (7, 1),
        ];
        let expanded = [true, false, true, false, false, false, true, false];

        assert!(!outline_has_prior_expanded_peer(&visible, &expanded, 2, 1));
        assert!(outline_has_prior_expanded_peer(&visible, &expanded, 5, 1));
        assert!(!outline_has_prior_expanded_peer(&visible, &expanded, 7, 1));
    }

    #[test]
    fn cached_outline_guide_relations_match_direct_scans() {
        let headings = [
            (0, 1, "Root"),
            (1, 2, "One"),
            (2, 2, "Two"),
            (3, 3, "Nested"),
            (4, 3, "Nested peer"),
            (5, 2, "Three"),
            (6, 1, "Second root"),
            (7, 2, "Child"),
        ]
        .into_iter()
        .map(|(line, level, text)| OutlineHeading {
            line,
            level,
            text: text.into(),
        })
        .collect::<Vec<_>>();
        let mut cache = OutlinePanelLayoutCache::default();
        cache.rebuild_if_needed(test_layout_key(0), &headings, &HashSet::new(), |_, _| 24.0);

        for (index, (_, depth)) in cache.visible_heading_indices.iter().enumerate() {
            assert_eq!(
                cache.has_later_peer[index],
                outline_has_later_peer(&cache.visible_heading_indices, index, *depth),
            );
            assert_eq!(
                cache.has_prior_expanded_peer[index],
                outline_has_prior_expanded_peer(
                    &cache.visible_heading_indices,
                    &cache.visible_expanded,
                    index,
                    *depth,
                ),
            );
        }
    }

    fn test_layout_key(epoch: usize) -> OutlinePanelLayoutKey {
        OutlinePanelLayoutKey {
            epoch,
            panel_width: 320.0,
            section_padding_x: 12.0,
            item_min_height: 24.0,
            item_padding_y: 4.0,
            line_height: 18.0,
            font_size: 13.0,
            indent_step: 14.0,
            fold_icon_hit_size: 20.0,
            fold_icon_gap: 4.0,
            scrollbar_reserve: 12.0,
        }
    }

    #[test]
    #[ignore = "50k-heading release benchmark; run explicitly with --ignored --nocapture"]
    fn outline_panel_layout_cache_50k_benchmark() {
        let headings = (0..50_000)
            .map(|index| OutlineHeading {
                line: index * 40,
                level: (index % 6 + 1) as u8,
                text: format!("Section {index} with CJK 标题 and emoji 🙂").into(),
            })
            .collect::<Vec<_>>();
        let mut cache = OutlinePanelLayoutCache::default();

        let rebuild_started = Instant::now();
        cache.rebuild_if_needed(test_layout_key(1), &headings, &HashSet::new(), |_, _| 26.0);
        let rebuild_ms = rebuild_started.elapsed().as_secs_f64() * 1_000.0;

        let cached_started = Instant::now();
        for _ in 0..10_000 {
            cache.rebuild_if_needed(test_layout_key(1), &headings, &HashSet::new(), |_, _| {
                unreachable!("cache hit must not rebuild rows")
            });
        }
        let cached_hits_ms = cached_started.elapsed().as_secs_f64() * 1_000.0;

        let scroll_started = Instant::now();
        let total_height = cache.offsets.last().copied().unwrap_or(0.0);
        let mut rendered_rows = 0usize;
        for sample in 0..10_000 {
            let scroll_y = total_height * sample as f32 / 10_000.0;
            let window = crate::editor::list_viewport::variable_list_window_for_offsets(
                scroll_y,
                600.0,
                0.0,
                &cache.offsets,
                12,
                24,
            );
            rendered_rows += window.render.len();
            black_box(window);
        }
        let scroll_queries_ms = scroll_started.elapsed().as_secs_f64() * 1_000.0;

        assert_eq!(cache.visible_heading_indices.len(), 50_000);
        assert!(rendered_rows / 10_000 < 64);
        println!(
            "outline-ui headings=50000 rebuild_ms={rebuild_ms:.3} cached_hits_10k_ms={cached_hits_ms:.3} scroll_queries_10k_ms={scroll_queries_ms:.3} avg_rendered_rows={:.1}",
            rendered_rows as f64 / 10_000.0,
        );
    }

    #[test]
    fn outline_tick_influence_spans_center_and_five_neighbors_per_side() {
        let weights = (4..=16)
            .map(|index| outline_tick_influence(index, Some(10), 5, 2.2))
            .collect::<Vec<_>>();
        assert_eq!(weights[0], 0.0);
        assert!(weights[1] < 0.02);
        assert_eq!(weights[6], 1.0);
        assert!(weights[11] < 0.02);
        assert_eq!(weights[12], 0.0);
    }

    #[test]
    fn outline_tick_influence_animates_old_center_down_and_new_center_up() {
        assert_eq!(
            animated_outline_tick_influence(10, Some(10), Some(20), 5, 2.2, 0.0),
            1.0
        );
        assert_eq!(
            animated_outline_tick_influence(10, Some(10), Some(20), 5, 2.2, 1.0),
            0.0
        );
        assert_eq!(
            animated_outline_tick_influence(20, Some(10), Some(20), 5, 2.2, 0.0),
            0.0
        );
        assert_eq!(
            animated_outline_tick_influence(20, Some(10), Some(20), 5, 2.2, 1.0),
            1.0
        );
    }

    #[test]
    fn outline_tick_color_uses_the_same_animation_weight_as_length() {
        let resting = gpui::Hsla {
            h: 0.1,
            s: 0.2,
            l: 0.3,
            a: 0.4,
        };
        let active = gpui::Hsla {
            h: 0.6,
            s: 0.7,
            l: 0.8,
            a: 0.9,
        };
        assert_eq!(super::lerp_hsla(resting, active, 0.0), resting);
        assert_eq!(super::lerp_hsla(resting, active, 1.0), active);
    }

    #[test]
    fn outline_hover_animation_fades_in_and_out() {
        assert_eq!(outline_hover_animation_value(0.0, 1.0, 0.0, 0.2), 0.0);
        assert_eq!(outline_hover_animation_value(0.0, 1.0, 0.2, 0.2), 1.0);
        assert_eq!(outline_hover_animation_value(1.0, 0.0, 0.0, 0.2), 1.0);
        assert_eq!(outline_hover_animation_value(1.0, 0.0, 0.2, 0.2), 0.0);
    }
}

fn visible_outline_heading_indices_with_depths(
    headings: &[OutlineHeading],
    collapsed_lines: &std::collections::HashSet<usize>,
) -> Vec<(usize, usize)> {
    let mut visible = Vec::new();
    let mut level_stack: Vec<(u8, bool)> = Vec::new();
    for (index, heading) in headings.iter().enumerate() {
        while level_stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            level_stack.pop();
        }
        let hidden_by_ancestor = level_stack.last().is_some_and(|(_, hidden)| *hidden);
        if !hidden_by_ancestor {
            let depth = level_stack.iter().filter(|(_, hidden)| !*hidden).count();
            visible.push((index, depth));
        }
        level_stack.push((
            heading.level,
            hidden_by_ancestor || collapsed_lines.contains(&heading.line),
        ));
    }
    visible
}

fn outline_heading_has_children(headings: &[OutlineHeading], heading_index: usize) -> bool {
    let Some(current) = headings.get(heading_index) else {
        return false;
    };
    headings
        .get(heading_index + 1)
        .is_some_and(|next| next.level > current.level)
}

fn outline_heading_index_for_line(headings: &[OutlineHeading], line: usize) -> Option<usize> {
    let index = headings.partition_point(|heading| heading.line < line);
    headings
        .get(index)
        .is_some_and(|heading| heading.line == line)
        .then_some(index)
}

fn outline_heading_at_y(
    headings: &[OutlineHeading],
    y: f32,
    mut line_y: impl FnMut(usize) -> f32,
) -> Option<usize> {
    if headings.is_empty() {
        return None;
    }
    let mut low = 0;
    let mut high = headings.len();
    while low < high {
        let middle = (low + high) / 2;
        if line_y(headings[middle].line) <= y {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Some(headings[low.saturating_sub(1)].line)
}

fn outline_active_line(
    clicked_line: Option<usize>,
    cursor_line: usize,
    headings: &[OutlineHeading],
    viewport_line: Option<usize>,
) -> Option<usize> {
    clicked_line
        .filter(|line| {
            *line == cursor_line && outline_heading_index_for_line(headings, *line).is_some()
        })
        .or(viewport_line)
}

fn outline_scroll_target(line_y: f32, padding_top: f32, max_scroll: f32) -> f32 {
    (line_y - padding_top).clamp(0.0, max_scroll)
}

pub(super) fn outline_navigation_in_progress(
    navigation_epoch: Option<usize>,
    epoch: usize,
) -> bool {
    navigation_epoch == Some(epoch)
}

fn invalidate_outline_navigation(scroll_epoch: &mut usize, navigation_epoch: &mut Option<usize>) {
    if outline_navigation_in_progress(*navigation_epoch, *scroll_epoch) {
        *scroll_epoch = scroll_epoch.wrapping_add(1);
    }
    *navigation_epoch = None;
}

fn outline_settled_frames(previous: usize, preview_pending: bool) -> usize {
    if preview_pending { 0 } else { previous + 1 }
}

#[cfg(test)]
fn outline_has_later_peer(
    visible_headings: &[(usize, usize)],
    current_index: usize,
    depth: usize,
) -> bool {
    visible_headings
        .iter()
        .skip(current_index + 1)
        .map(|(_, next_depth)| *next_depth)
        .take_while(|next_depth| *next_depth >= depth)
        .any(|next_depth| next_depth == depth)
}

#[cfg(test)]
fn outline_has_prior_expanded_peer(
    visible_headings: &[(usize, usize)],
    expanded: &[bool],
    current_index: usize,
    depth: usize,
) -> bool {
    visible_headings
        .iter()
        .zip(expanded)
        .take(current_index)
        .rev()
        .take_while(|((_, previous_depth), _)| *previous_depth >= depth)
        .any(|((_, previous_depth), has_children)| *previous_depth == depth && *has_children)
}

fn outline_dashed_guide_segments(
    x: f32,
    start_y: f32,
    end_y: f32,
    width: f32,
    dash_length: f32,
    gap_length: f32,
    color: gpui::Hsla,
) -> Vec<AnyElement> {
    let height = (end_y - start_y).max(0.0);
    let step = (dash_length + gap_length).max(1.0);
    let count = (height / step).ceil() as usize;
    (0..count)
        .filter_map(|index| {
            let top = start_y + index as f32 * step;
            let segment_height = dash_length.min(end_y - top);
            (segment_height > 0.0).then(|| {
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(top))
                    .w(px(width))
                    .h(px(segment_height))
                    .bg(color)
                    .into_any_element()
            })
        })
        .collect()
}

fn outline_nearest_guide_ranges(
    row_height: f32,
    icon_top: f32,
    icon_clearance: f32,
    has_children: bool,
    has_later_peer: bool,
) -> Vec<(f32, f32)> {
    let upper_end = if has_children {
        (icon_top - icon_clearance).max(0.0)
    } else {
        row_height * 0.5
    };
    let mut ranges = vec![(0.0, upper_end)];
    if has_later_peer && !has_children {
        ranges.push((row_height * 0.5, row_height));
    }
    ranges
}

fn multiply_alpha(color: gpui::Hsla, factor: f32) -> gpui::Hsla {
    gpui::Hsla {
        a: color.a * factor.clamp(0.0, 1.0),
        ..color
    }
}

fn lerp_hsla(from: gpui::Hsla, to: gpui::Hsla, t: f32) -> gpui::Hsla {
    let t = t.clamp(0.0, 1.0);
    gpui::Hsla {
        h: from.h + (to.h - from.h) * t,
        s: from.s + (to.s - from.s) * t,
        l: from.l + (to.l - from.l) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

fn heading_level_opacity(level: u8, level_opacities: [f32; 6]) -> f32 {
    let index = level.saturating_sub(1).min(5) as usize;
    level_opacities[index].clamp(0.0, 1.0)
}

fn outline_tick_influence(
    row_index: usize,
    active_index: Option<usize>,
    neighbor_count: usize,
    falloff_power: f32,
) -> f32 {
    let Some(active_index) = active_index else {
        return 0.0;
    };
    let distance = row_index.abs_diff(active_index);
    if distance > neighbor_count {
        return 0.0;
    }
    let linear = (neighbor_count + 1 - distance) as f32 / (neighbor_count + 1) as f32;
    linear.powf(falloff_power.max(0.01))
}

fn animated_outline_tick_influence(
    row_index: usize,
    previous_active_index: Option<usize>,
    active_index: Option<usize>,
    neighbor_count: usize,
    falloff_power: f32,
    progress: f32,
) -> f32 {
    let from = outline_tick_influence(
        row_index,
        previous_active_index,
        neighbor_count,
        falloff_power,
    );
    let to = outline_tick_influence(row_index, active_index, neighbor_count, falloff_power);
    from + (to - from) * progress.clamp(0.0, 1.0)
}

fn outline_hover_animation_value(
    from: f32,
    target: f32,
    elapsed_seconds: f32,
    duration_seconds: f32,
) -> f32 {
    let progress = (elapsed_seconds / duration_seconds.max(0.001)).clamp(0.0, 1.0);
    let eased = super::ease_in_out_cubic(progress);
    from + (target - from) * eased
}

impl EditorView {
    pub(crate) fn cancel_outline_navigation(&mut self) {
        invalidate_outline_navigation(
            &mut self.scroll_anim_epoch,
            &mut self.outline_navigation_epoch,
        );
        self.outline_rail_clicked_active_line = None;
    }

    pub(crate) fn invalidate_outline_layout_cache(&mut self) {
        self.outline_panel_layout_epoch = self.outline_panel_layout_epoch.wrapping_add(1);
        self.outline_panel_layout_cache.borrow_mut().key = None;
    }

    pub(crate) fn flash_outline_heading_line(
        &mut self,
        target_line: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.current_file_path.clone() else {
            return;
        };

        let start = self.buffer.line_start(target_line);
        let end = self.buffer.line_end(target_line);
        if start >= end {
            return;
        }

        let range = start..end;
        let outline_styles = &self.styles.outline;
        let initial_delay_ms =
            std::time::Duration::from_millis(outline_styles.outline_flash_initial_delay_ms);
        let show_ms =
            std::time::Duration::from_millis(outline_styles.outline_flash_show_duration_ms);
        let gap_ms = std::time::Duration::from_millis(outline_styles.outline_flash_gap_duration_ms);
        self.outline_flash_epoch += 1;
        let epoch = self.outline_flash_epoch;
        self.outline_flash_highlight_path = Some(path.clone());
        self.outline_flash_highlight_ranges = vec![range.clone()];
        self.outline_flash_highlight_current_index = Some(0);
        cx.notify();

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(initial_delay_ms).await;
            let continue_flash = this
                .update(cx, |this, cx| {
                    if this.outline_flash_epoch != epoch {
                        return false;
                    }
                    this.outline_flash_highlight_path = Some(path.clone());
                    this.outline_flash_highlight_ranges = vec![range.clone()];
                    this.outline_flash_highlight_current_index = Some(0);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !continue_flash {
                return;
            }

            cx.background_executor().timer(show_ms).await;
            let _ = this.update(cx, |this, cx| {
                if this.outline_flash_epoch != epoch {
                    return;
                }
                this.outline_flash_highlight_path = None;
                this.outline_flash_highlight_ranges.clear();
                this.outline_flash_highlight_current_index = None;
                cx.notify();
            });

            cx.background_executor().timer(gap_ms).await;
            let _ = this.update(cx, |this, cx| {
                if this.outline_flash_epoch != epoch {
                    return;
                }
                this.outline_flash_highlight_path = Some(path.clone());
                this.outline_flash_highlight_ranges = vec![range.clone()];
                this.outline_flash_highlight_current_index = Some(0);
                cx.notify();
            });

            cx.background_executor().timer(show_ms).await;
            let _ = this.update(cx, |this, cx| {
                if this.outline_flash_epoch != epoch {
                    return;
                }
                this.outline_flash_highlight_path = None;
                this.outline_flash_highlight_ranges.clear();
                this.outline_flash_highlight_current_index = None;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn schedule_large_markdown_outline_index(&mut self, cx: &mut Context<Self>) {
        let outline_visible = self.outline_rail_visible
            || self.utility_panel == Some(RightUtilityPanel::Outline)
            || self.displayed_utility_panel == Some(RightUtilityPanel::Outline);
        if !outline_visible
            || !self.uses_lazy_markdown_decorations()
            || self.markdown_outline_index.borrow().is_some()
            || self.markdown_outline_build_task.is_some()
        {
            return;
        }
        let pane_id = self.current_pane_id();
        let epoch = self.markdown_outline_epoch;
        let text = self.buffer.text().to_string();
        let build = cx.background_executor().spawn(async move {
            let buffer = Buffer::from_string(text);
            MarkdownOutlineIndex::rebuild(&buffer)
        });
        self.markdown_outline_build_task = Some(cx.spawn(async move |this, cx| {
            let index = build.await;
            this.update(cx, |this, cx| {
                this.with_pane_if_alive(pane_id, |this| {
                    this.markdown_outline_build_task = None;
                    if this.markdown_outline_epoch == epoch && this.uses_lazy_markdown_decorations()
                    {
                        *this.markdown_outline_index.borrow_mut() = Some(index);
                        this.invalidate_outline_layout_cache();
                    }
                });
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn collect_outline_headings(&self) -> Ref<'_, Vec<OutlineHeading>> {
        if self.uses_lazy_markdown_decorations() && self.markdown_outline_index.borrow().is_some() {
            return Ref::map(self.markdown_outline_index.borrow(), |index| {
                &index.as_ref().expect("checked above").headings
            });
        }
        if self.outline_headings_cache_dirty.get() {
            {
                let mut headings = self.outline_headings_cache.borrow_mut();
                headings.clear();
                for (line_idx, dec) in self.decorations.iter().enumerate() {
                    if let BlockType::Heading(level) = dec.block_type {
                        let raw = self.buffer.line_content(line_idx);
                        let text = raw.trim_start_matches('#').trim_start();
                        headings.push(OutlineHeading {
                            line: line_idx,
                            level,
                            text: SharedString::from(text.to_string()),
                        });
                    }
                }
            }
            self.outline_headings_cache_dirty.set(false);
        }
        self.outline_headings_cache.borrow()
    }

    /// Follow the section at the top of the editor, not the next heading at
    /// the bottom of the viewport (which can move while body text is edited).
    fn current_outline_heading_line(&self, headings: &[OutlineHeading]) -> Option<usize> {
        outline_heading_at_y(headings, self.scroll_offset + self.padding_top(), |line| {
            self.line_y_offset(line)
        })
    }

    fn outline_rail_active_line(&self, headings: &[OutlineHeading]) -> Option<usize> {
        outline_active_line(
            self.outline_rail_clicked_active_line,
            self.cursor_line(),
            headings,
            self.current_outline_heading_line(headings),
        )
    }

    fn set_outline_rail_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let rail_changed = self.outline_rail_hovered != hovered;
        self.outline_rail_hovered = hovered;
        if hovered {
            // Cancel any pending toolbar auto-hide that was scheduled while the
            // cursor was still over the content. Entering the rail does not
            // re-run the editor's corner-hover check (the rail occludes it), so
            // without this the queued hide fires ~2s later and hides the toolbar
            // even though the cursor is on the rail.
            self.toolbar_corner_hide_task = None;
        }
        if rail_changed {
            let anim_seconds = self
                .styles
                .outline
                .outline_rail_hover_animation_seconds
                .max(0.001);
            let now = std::time::Instant::now();
            let current = self
                .outline_rail_hover_since
                .get()
                .map(|started| {
                    outline_hover_animation_value(
                        self.outline_rail_hover_from.get(),
                        self.outline_rail_hover_target.get(),
                        started.elapsed().as_secs_f32(),
                        anim_seconds,
                    )
                })
                .unwrap_or(self.outline_rail_hover_target.get());
            let target = if hovered { 1.0 } else { 0.0 };
            self.outline_rail_hover_from.set(current);
            self.outline_rail_hover_target.set(target);
            self.outline_rail_hover_since.set(Some(now));
            cx.spawn(async move |this, cx| {
                let frames = ((anim_seconds / 0.016).ceil() as usize).max(1) + 1;
                for _ in 0..frames {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                    let keep = this
                        .update(cx, |this, cx| {
                            if this.outline_rail_hover_since.get() != Some(now) {
                                return false;
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            })
            .detach();
        }
        let toolbar_changed = self.toolbar_corner_hovered != hovered;
        if self.toolbar_corner_hovered != hovered {
            self.toolbar_corner_hovered = hovered;
            self.sync_toolbar_opacity(cx);
        }
        if rail_changed || toolbar_changed {
            cx.notify();
        }
    }

    fn toggle_outline_rail_heading_collapsed(&mut self, line: usize, cx: &mut Context<Self>) {
        if !self.outline_rail_collapsed_heading_lines.remove(&line) {
            self.outline_rail_collapsed_heading_lines.insert(line);
        }
        self.invalidate_outline_layout_cache();
        cx.notify();
    }

    pub(crate) fn scroll_to_line(&mut self, target_line: usize, cx: &mut Context<Self>) {
        let line_end = self.buffer.line_end(target_line);
        self.cursor.offset = line_end;
        self.cursor.anchor = line_end;
        self.pending_preview_reveal = None;
        self.pending_click_anchor = None;
        self.pending_typing_scroll_guard = None;
        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        self.pending_center_cursor_in_view_immediately = false;
        self.center_cursor_confirm_frames = 0;

        let target_scroll = outline_scroll_target(
            self.line_y_offset(target_line),
            self.padding_top(),
            self.max_scroll_offset(),
        );
        let start_scroll = self.scroll_offset;
        let delta = target_scroll - start_scroll;
        let should_animate = delta.abs() >= 1.0;
        if !should_animate {
            self.scroll_offset = target_scroll;
            self.clamp_scroll();
        }
        let total_steps = 20u32;
        let step_ms = 8u64;
        self.scroll_anim_epoch += 1;
        let epoch = self.scroll_anim_epoch;
        self.outline_navigation_epoch = Some(epoch);
        cx.notify();

        cx.spawn(async move |this, cx| {
            if should_animate {
                for step in 1..=total_steps {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(step_ms))
                        .await;
                    let result = this.update(cx, |this, cx| {
                        if this.scroll_anim_epoch != epoch {
                            return false;
                        }
                        // Offscreen wrapping and generated previews refine line geometry as the
                        // target enters view. Recompute the destination instead of animating to
                        // the estimate captured on the click frame.
                        let latest_target = outline_scroll_target(
                            this.line_y_offset(target_line),
                            this.padding_top(),
                            this.max_scroll_offset(),
                        );
                        let t = step as f32 / total_steps as f32;
                        let ease = 1.0 - (1.0 - t).powi(3);
                        this.scroll_offset = start_scroll + (latest_target - start_scroll) * ease;
                        this.clamp_scroll();
                        cx.notify();
                        true
                    });
                    match result {
                        Ok(true) => {}
                        _ => return,
                    }
                }
            }

            // A newly visible chart can be rendered over several frames. Keep the
            // target anchored until visible previews are loaded and layout has
            // settled for two frames (bounded in case a preview cannot load).
            let mut settled_frames = 0;
            for _ in 0..60 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.scroll_anim_epoch != epoch {
                            return false;
                        }
                        this.scroll_offset = outline_scroll_target(
                            this.line_y_offset(target_line),
                            this.padding_top(),
                            this.max_scroll_offset(),
                        );
                        this.clamp_scroll();
                        let (start, end) = this.generated_preview_priority_line_range();
                        let pending =
                            this.decorations
                                .get(start..=end)
                                .is_some_and(|decorations| {
                                    decorations.iter().any(|decoration| {
                                        decoration.block_preview.as_ref().is_some_and(|preview| {
                                            preview.is_anchor
                                                && (!this
                                                    .image_cache
                                                    .contains_key(&preview.cache_key)
                                                    || !this
                                                        .generated_preview_svg_cache
                                                        .contains_key(&preview.cache_key))
                                        })
                                    })
                                });
                        cx.notify();
                        !pending
                    })
                    .unwrap_or(false);
                settled_frames = outline_settled_frames(settled_frames, !keep);
                if settled_frames >= 2 {
                    break;
                }
                if this
                    .read_with(cx, |this, _| this.scroll_anim_epoch != epoch)
                    .unwrap_or(true)
                {
                    return;
                }
            }
            let _ = this.update(cx, |this, _| {
                if this.outline_navigation_epoch == Some(epoch) {
                    this.outline_navigation_epoch = None;
                }
            });
        })
        .detach();
    }

    pub(crate) fn render_outline_rail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.outline_rail_visible || !self.active_tab_is_markdown() {
            self.outline_rail_zone_from_right.set(0.0);
            return None;
        }
        let headings = self.collect_outline_headings();
        if headings.is_empty() {
            self.outline_rail_zone_from_right.set(0.0);
            return None;
        }

        let active_line = self.outline_rail_active_line(&headings);

        let os = &self.styles.outline;
        let right_margin = self.scaled(os.outline_rail_right_margin_px);
        let scrollbar_hit_left_from_right = self.scaled(
            self.styles.editor.editor_scrollbar_right_margin_px
                + self
                    .styles
                    .editor
                    .editor_scrollbar_hit_width_px
                    .max(self.styles.editor.editor_scrollbar_width_px)
                    .max(self.styles.editor.editor_scrollbar_hover_width_px),
        );
        let track_right = scrollbar_hit_left_from_right.min(right_margin);
        let track_extra_right_padding = (right_margin - track_right).max(0.0);
        let row_height = self.scaled(os.outline_rail_row_height_px);
        let tick_width = self.scaled(os.outline_rail_tick_width_px);
        let tick_thickness = self
            .scaled(os.outline_rail_tick_thickness_px)
            .round()
            .max(1.0);
        let tick_color = os.outline_rail_tick_color;
        let tick_active_color = os.outline_rail_tick_active_color;
        let tick_level_opacities = os.outline_rail_tick_level_opacities;
        let active_extra_width = self.scaled(os.outline_rail_tick_active_extra_width_px);
        let active_neighbor_count = os.outline_rail_tick_active_neighbor_count;
        let active_falloff_power = os.outline_rail_tick_active_falloff_power;
        let active_anim_seconds = os.outline_rail_tick_active_animation_seconds.max(0.001);
        let vertical_padding = self.scaled(os.outline_rail_vertical_padding_px);

        let previous_active_line = self.outline_rail_last_active_line.get();
        if previous_active_line != active_line {
            self.outline_rail_previous_active_line
                .set(previous_active_line);
            self.outline_rail_last_active_line.set(active_line);
            self.outline_rail_active_since
                .set(Some(std::time::Instant::now()));
            let target_active_line = active_line;
            let animation_frames = ((active_anim_seconds / 0.016).ceil() as usize).max(1) + 1;
            cx.spawn(async move |this, cx| {
                for _ in 0..animation_frames {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                    let keep = this
                        .update(cx, |this, cx| {
                            if this.outline_rail_last_active_line.get() != target_active_line {
                                return false;
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            })
            .detach();
        }

        let viewport_height = self.last_viewport_height.max(0.0);
        let rail_height = outline_rail_inner_height(viewport_height, vertical_padding);
        let row_count = headings.len();
        let hover_ease = self
            .outline_rail_hover_since
            .get()
            .map(|started| {
                outline_hover_animation_value(
                    self.outline_rail_hover_from.get(),
                    self.outline_rail_hover_target.get(),
                    started.elapsed().as_secs_f32(),
                    os.outline_rail_hover_animation_seconds,
                )
            })
            .unwrap_or(self.outline_rail_hover_target.get());
        let active_heading_index =
            active_line.and_then(|line| outline_heading_index_for_line(&headings, line));
        let previous_active_heading_index = self
            .outline_rail_previous_active_line
            .get()
            .and_then(|line| outline_heading_index_for_line(&headings, line));
        let slot_count = outline_rail_render_count(row_count, rail_height, row_height);
        let compressed = row_count > slot_count;
        let active_slot = active_heading_index
            .map(|index| outline_rail_project_index(index, row_count, slot_count));
        let previous_active_slot = previous_active_heading_index
            .map(|index| outline_rail_project_index(index, row_count, slot_count));
        let mut row_elements: Vec<AnyElement> = Vec::with_capacity(slot_count);
        let active_progress = self
            .outline_rail_active_since
            .get()
            .map(|started| (started.elapsed().as_secs_f32() / active_anim_seconds).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let active_ease = super::ease_in_out_cubic(active_progress);
        for slot_index in 0..slot_count {
            let heading_index = outline_rail_project_index(slot_index, slot_count, row_count);
            let heading = &headings[heading_index];
            let level_opacity = heading_level_opacity(heading.level, tick_level_opacities);
            let tick_influence = animated_outline_tick_influence(
                slot_index,
                previous_active_slot,
                active_slot,
                active_neighbor_count,
                active_falloff_power,
                active_ease,
            );
            let animated_tick_width = tick_width + active_extra_width * tick_influence;
            let resting_color = multiply_alpha(tick_color, level_opacity);
            let hover_color = lerp_hsla(resting_color, tick_active_color, hover_ease);
            let base_tick_color = lerp_hsla(hover_color, tick_active_color, tick_influence);
            let row_top = vertical_padding
                + if compressed {
                    outline_rail_row_top(
                        slot_index,
                        slot_count,
                        rail_height,
                        row_height,
                        tick_thickness,
                    )
                } else {
                    outline_rail_compact_row_top(slot_index, slot_count, rail_height, row_height)
                };
            row_elements.push(
                div()
                    .id(SharedString::from(format!(
                        "outline-rail-slot-{slot_index}"
                    )))
                    .absolute()
                    .top(px(row_top))
                    .right(px(0.0))
                    .w_full()
                    .h(px(row_height))
                    .flex()
                    .items_center()
                    .justify_end()
                    .cursor(CursorStyle::Arrow)
                    .child(
                        div()
                            .w(px(tick_width + active_extra_width))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "outline-rail-tick-{slot_index}"
                                    )))
                                    .w(px(animated_tick_width))
                                    .h(px(tick_thickness))
                                    .rounded_full()
                                    .bg(base_tick_color),
                            ),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.toggle_outline_panel(cx);
                        }),
                    )
                    .into_any_element(),
            );
        }
        let rail_collapsed_width = tick_width + active_extra_width;
        let rail_width = rail_collapsed_width;
        let rail_outer_width = rail_width + right_margin;
        let track_width = rail_width + track_extra_right_padding;

        let column = div()
            .id("outline-rail-tick-column")
            .relative()
            .h_full()
            .w(px(rail_collapsed_width))
            .children(row_elements);

        let content = div()
            .id("outline-rail-content")
            .relative()
            .h_full()
            .w_full()
            .overflow_hidden();

        let track_inner = div()
            .id("outline-rail-track-inner")
            .relative()
            .h_full()
            .w_full()
            .overflow_hidden();

        let track_inner = track_inner.child(column.absolute().top_0().bottom_0().right(px(0.0)));

        let track_hover_width = if self.outline_rail_hovered {
            track_width
        } else {
            (tick_width + active_extra_width).max(tick_thickness) + track_extra_right_padding
        };

        // Record how far in from the right edge the rail's interactive zone
        // reaches this frame so the editor's toolbar-hover logic can keep the
        // toolbar visible while the rail is hovered, regardless of event
        // occlusion or content width.
        self.outline_rail_zone_from_right
            .set(track_right + track_width + right_margin);

        let track = div()
            .id("outline-rail-track")
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(track_right))
            .w(px(track_hover_width))
            .occlude()
            .cursor(CursorStyle::Arrow)
            // Always listen for hover (not only once already hovered) so hovering
            // anywhere over the full-height rail track — including the empty space
            // above the first tick and below the last — keeps the rail (and the
            // editor toolbar) shown instead of letting the toolbar hide.
            .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                this.set_outline_rail_hovered(*hovering, cx);
            }))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(track_extra_right_padding))
                    .w(px(if self.outline_rail_hovered {
                        rail_width
                    } else {
                        (tick_width + active_extra_width).max(tick_thickness)
                    }))
                    .child(track_inner),
            );
        let content = content.child(track);

        Some(
            div()
                .id("outline-rail")
                .absolute()
                .top_0()
                .right(px(0.0))
                .h_full()
                .w(px(rail_outer_width))
                .child(content)
                .into_any_element(),
        )
    }

    pub(crate) fn render_outline_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let headings = self.collect_outline_headings();
        let active_line = self.outline_rail_active_line(&headings);
        let ups = &self.styles.utility_panel;
        let os = &self.styles.outline;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let item_min_height = self.scaled(os.outline_rail_label_item_height_px).max(22.0);
        let item_padding_y = self.scaled(os.outline_rail_label_item_padding_y_px);
        let line_height = self.scaled(os.outline_rail_label_line_height_px);
        let font_size = self.scaled(os.outline_rail_label_font_size_px);
        let indent_step = self.scaled(os.outline_rail_label_indent_step_px);
        let fold_icon_size = self.scaled(os.outline_rail_label_fold_icon_size_px);
        let fold_icon_hit_size = (fold_icon_size + self.scaled(8.0)).round();
        let fold_icon_visual_inset = ((fold_icon_hit_size - fold_icon_size) * 0.5).max(0.0);
        let fold_icon_gap = self.scaled(os.outline_rail_label_fold_icon_gap_px);
        let indent_guide_width = self.scaled(os.outline_rail_label_indent_guide_width_px);
        let indent_guide_dash_length =
            self.scaled(os.outline_rail_label_indent_guide_dash_length_px);
        let indent_guide_gap_length = self.scaled(os.outline_rail_label_indent_guide_gap_length_px);
        let indent_guide_color = multiply_alpha(os.outline_rail_label_indent_guide_color, 0.45);
        let panel_width = self.utility_panel_layout_width().max(self.scaled(10.0));
        let scrollbar_reserve = self.scaled(
            self.styles
                .editor
                .editor_scrollbar_hit_width_px
                .max(self.styles.editor.editor_scrollbar_width_px)
                + self.styles.editor.editor_scrollbar_right_margin_px,
        );
        let outline_row_height = |_: &OutlineHeading, _: usize| {
            (item_padding_y * 2.0 + line_height).max(item_min_height)
        };
        let layout_key = OutlinePanelLayoutKey {
            epoch: self.outline_panel_layout_epoch,
            panel_width,
            section_padding_x,
            item_min_height,
            item_padding_y,
            line_height,
            font_size,
            indent_step,
            fold_icon_hit_size,
            fold_icon_gap,
            scrollbar_reserve,
        };
        {
            let mut cache = self.outline_panel_layout_cache.borrow_mut();
            cache.rebuild_if_needed(
                layout_key,
                &headings,
                &self.outline_rail_collapsed_heading_lines,
                outline_row_height,
            );
        }

        let mut children: Vec<AnyElement> = vec![
            div().h(px(top_spacer)).flex_shrink_0().into_any_element(),
            div()
                .h(px(header_height))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(px(header_padding_x))
                .text_size(px(header_font_size))
                .text_color(ups.utility_panel_header_text_color)
                .child(crate::i18n::outline_title(self.ui_language))
                .into_any_element(),
        ];

        if headings.is_empty() {
            children.push(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(empty_font_size))
                    .text_color(ups.utility_panel_empty_text_color)
                    .child(crate::i18n::outline_empty(self.ui_language))
                    .into_any_element(),
            );
        } else {
            children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            let prefix_height = top_spacer + header_height + section_gap;
            let layout_cache = self.outline_panel_layout_cache.borrow();
            let visible_heading_indices = &layout_cache.visible_heading_indices;
            let list_window = super::list_viewport::variable_list_window_from_offsets(
                &self.outline_panel_scroll_handle,
                prefix_height,
                &layout_cache.offsets,
                12,
                24,
            );
            if list_window.top_spacer_px > 0.0 {
                children.push(
                    div()
                        .h(px(list_window.top_spacer_px))
                        .flex_shrink_0()
                        .into_any_element(),
                );
            }
            let active_bg = ups.utility_panel_button_hover_bg;
            let fold_icon_hover_color = os.outline_rail_label_hover_color;
            let render_start = list_window.render.start;
            for (visible_offset, (heading_index, depth)) in visible_heading_indices
                [list_window.render]
                .iter()
                .copied()
                .enumerate()
            {
                let heading = &headings[heading_index];

                let target_line = heading.line;
                let has_children = outline_heading_has_children(&headings, heading_index);
                let is_collapsed = self
                    .outline_rail_collapsed_heading_lines
                    .contains(&target_line);
                let is_active = Some(target_line) == active_line;
                let color = if is_active {
                    os.outline_rail_label_active_color
                } else {
                    ups.utility_panel_item_value_text_color
                };
                let indent = depth as f32 * indent_step.max(0.0);
                let inner_padding_left = header_padding_x - fold_icon_visual_inset + indent;
                let row_height = layout_cache.item_heights[render_start + visible_offset];
                let visible_index = render_start + visible_offset;
                let icon_clearance = self.scaled(2.0);
                let icon_top = ((row_height - fold_icon_size) * 0.5).max(0.0);
                let mut guide_segments = Vec::new();
                let guide_was_handed_off = layout_cache.has_prior_expanded_peer[visible_index];
                if depth > 0 && !guide_was_handed_off {
                    let guide_depth = depth - 1;
                    let guide_x = section_padding_x
                        + guide_depth as f32 * indent_step.max(0.0)
                        + fold_icon_hit_size
                        + fold_icon_gap;
                    let has_later_peer = layout_cache.has_later_peer[visible_index];
                    for (start_y, end_y) in outline_nearest_guide_ranges(
                        row_height,
                        icon_top,
                        icon_clearance,
                        has_children,
                        has_later_peer,
                    ) {
                        guide_segments.extend(outline_dashed_guide_segments(
                            guide_x,
                            start_y,
                            end_y,
                            indent_guide_width,
                            indent_guide_dash_length,
                            indent_guide_gap_length,
                            indent_guide_color,
                        ));
                    }
                }
                let icon_path = if is_collapsed {
                    FOLD_SVG_PATH
                } else {
                    FOLD_EXPANDED_SVG_PATH
                };

                children.push(
                    div()
                        .w_full()
                        .max_w_full()
                        .overflow_hidden()
                        .min_h(px(row_height))
                        .flex_shrink_0()
                        .relative()
                        .pl(px(inner_padding_left))
                        .pr(px(section_padding_x))
                        .py(px(item_padding_y))
                        .flex()
                        .items_center()
                        .gap(px(fold_icon_gap))
                        .text_size(px(font_size))
                        .text_color(color)
                        .line_height(px(line_height))
                        .cursor(CursorStyle::Arrow)
                        .when(is_active, |this| this.bg(active_bg))
                        .hover(move |style| style.bg(active_bg))
                        .children(guide_segments)
                        .when(has_children, |this| {
                            this.child(
                                div()
                                    .w(px(fold_icon_hit_size))
                                    .h(px(fold_icon_hit_size))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(color)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.toggle_outline_rail_heading_collapsed(
                                                target_line,
                                                cx,
                                            );
                                        }),
                                    )
                                    .child(
                                        svg()
                                            .path(icon_path)
                                            .size(px(fold_icon_size))
                                            .text_color(color)
                                            .hover(move |style| {
                                                style.text_color(fold_icon_hover_color)
                                            }),
                                    ),
                            )
                        })
                        .when(!has_children, |this| {
                            this.child(div().w(px(fold_icon_hit_size)).flex_none())
                        })
                        .child(
                            div()
                                .w(px(0.0))
                                .min_w(px(0.0))
                                .flex_1()
                                .truncate()
                                .child(heading.text.clone()),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.outline_rail_clicked_active_line = Some(target_line);
                                this.scroll_to_line(target_line, cx);
                                this.flash_outline_heading_line(target_line, cx);
                            }),
                        )
                        .into_any_element(),
                );
            }
            if list_window.bottom_spacer_px > 0.0 {
                children.push(
                    div()
                        .h(px(list_window.bottom_spacer_px))
                        .flex_shrink_0()
                        .into_any_element(),
                );
            }
            children.push(
                div()
                    .h(px(bottom_spacer))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }

        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::Outline,
            &self.outline_panel_scroll_handle,
            f32::from(self.outline_panel_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        div()
            .id("outline-panel")
            .h_full()
            .w_full()
            .min_w(px(0.0))
            .bg(ups.utility_panel_bg)
            .relative()
            .overflow_hidden()
            .border_l(px(ups.utility_panel_border_width_px))
            .border_color(ups.utility_panel_border_color)
            .child(
                div()
                    .id("outline-panel-scroll")
                    .h_full()
                    .w_full()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        this.reveal_panel_scrollbar(PanelScrollbarKind::Outline, cx);
                    }))
                    .track_scroll(&self.outline_panel_scroll_handle)
                    .flex()
                    .flex_col()
                    .children(children),
            )
            .when_some(scrollbar, |this, scrollbar| this.child(scrollbar))
            .into_any_element()
    }

    pub(crate) fn outline_panel_preferred_width(&self) -> f32 {
        let headings = self.collect_outline_headings();
        let ups = &self.styles.utility_panel;
        let os = &self.styles.outline;
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let font_size = self.scaled(os.outline_rail_label_font_size_px);
        let indent_step = self.scaled(os.outline_rail_label_indent_step_px);
        let fold_icon_size = self.scaled(os.outline_rail_label_fold_icon_size_px);
        let fold_icon_gap = self.scaled(os.outline_rail_label_fold_icon_gap_px);
        let text_cap = self.scaled(os.outline_rail_label_max_width_px);
        let visible_heading_indices = visible_outline_heading_indices_with_depths(
            &headings,
            &self.outline_rail_collapsed_heading_lines,
        );
        let header_width = header_padding_x * 2.0
            + outline_heading_text_width(
                crate::i18n::outline_title(self.ui_language),
                header_font_size,
            )
            + self.scaled(20.0);
        let content_width = visible_heading_indices
            .iter()
            .take(512)
            .map(|(heading_index, depth)| {
                let heading = &headings[*heading_index];
                let text_width = outline_heading_text_width(heading.text.as_ref(), font_size)
                    .min(text_cap.max(font_size));
                section_padding_x * 2.0
                    + *depth as f32 * indent_step.max(0.0)
                    + fold_icon_size
                    + fold_icon_gap
                    + text_width
                    + self.scaled(20.0)
            })
            .fold(header_width, f32::max);
        let min_width = self.scaled(150.0);
        let max_width = self.scaled(self.styles.utility_panel.utility_panel_resize_max_width_px);
        let (min_width, max_width) = normalized_outline_width_bounds(min_width, max_width);
        let content_width = if content_width.is_finite() {
            content_width.max(0.0)
        } else {
            min_width
        };
        content_width.clamp(min_width, max_width)
    }

    pub(crate) fn outline_panel_resize_bounds(&self) -> (f32, f32) {
        let min_width = self.scaled(10.0);
        let global_max = self.scaled(self.styles.utility_panel.utility_panel_resize_max_width_px);
        normalized_outline_width_bounds(min_width, global_max)
    }
}
