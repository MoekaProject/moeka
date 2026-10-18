use std::collections::HashMap;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{
    Bounds, Font, FontStyle, FontWeight, Hsla, LineLayout, PaintQuad, Pixels, ShapedGlyph,
    ShapedRun, SharedString, StrikethroughStyle, TextRun, UnderlineStyle, Window, WrappedLine,
    WrappedLineLayout, fill, point, px,
};

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, ScopeSelectors, StyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use image::{Frame, RgbaImage};
use mathjax_svg_rs::{HorizontalAlign, MathJax, Options as MathJaxOptions};
use resvg::tiny_skia::{Pixmap, Transform};

use crate::front_matter::{self, FrontMatterValue};
use crate::markdown::{
    self, BlockPreview, BlockPreviewKind, BlockType, DisplayReplacement, InlineKind,
    LineDecoration, TableLineInfo,
};
use crate::theme::{CodeSyntaxPalette, Theme};

pub(crate) use super::text_layout::{code_block_soft_wrap_width, table_cell_content_offset};
use super::*;

const CODE_HIGHLIGHT_CACHE_MAX_ENTRIES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AnchoredPopupVertical {
    Above { bottom: f32 },
    Below { top: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AnchoredPopupPlacement {
    pub left: f32,
    pub width: f32,
    pub max_height: f32,
    pub vertical: AnchoredPopupVertical,
}

pub(crate) fn anchored_popup_placement(
    viewport_width: f32,
    viewport_height: f32,
    anchor: Bounds<Pixels>,
    desired_width: f32,
    desired_height: f32,
    match_anchor_width: bool,
    gap: f32,
    margin: f32,
) -> AnchoredPopupPlacement {
    let anchor_left = f32::from(anchor.origin.x);
    let anchor_top = f32::from(anchor.origin.y);
    let anchor_width = f32::from(anchor.size.width);
    let anchor_height = f32::from(anchor.size.height);
    let anchor_right = anchor_left + anchor_width;
    let anchor_bottom = anchor_top + anchor_height;
    let available_width = (viewport_width - margin * 2.0).max(0.0);
    let width = if match_anchor_width {
        anchor_width
    } else {
        desired_width
    }
    .min(available_width)
    .max(0.0);

    let left = if anchor_left + width <= viewport_width - margin {
        // Fits: align the popup's left edge exactly with the anchor so
        // trigger buttons and their menus line up, even flush at an edge.
        anchor_left.max(0.0)
    } else {
        (anchor_right - width).clamp(margin, (viewport_width - width - margin).max(margin))
    };

    let space_above = (anchor_top - gap - margin).max(0.0);
    let space_below = (viewport_height - anchor_bottom - gap - margin).max(0.0);
    let open_above = space_above >= desired_height || space_above >= space_below;
    let (max_height, vertical) = if open_above {
        (
            desired_height.min(space_above),
            AnchoredPopupVertical::Above {
                bottom: viewport_height - anchor_top + gap,
            },
        )
    } else {
        (
            desired_height.min(space_below),
            AnchoredPopupVertical::Below {
                top: anchor_bottom + gap,
            },
        )
    };

    AnchoredPopupPlacement {
        left,
        width,
        max_height,
        vertical,
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct CodeHighlightCacheKey {
    language: String,
    line_content: String,
    palette: CodeSyntaxPalette,
    math_context: bool,
}

#[derive(Clone)]
struct CodeHighlightSegment {
    len: usize,
    color: Hsla,
    weight: FontWeight,
    style: FontStyle,
}

fn code_highlight_cache()
-> &'static Mutex<HashMap<CodeHighlightCacheKey, Vec<CodeHighlightSegment>>> {
    static CACHE: OnceLock<Mutex<HashMap<CodeHighlightCacheKey, Vec<CodeHighlightSegment>>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn apply_code_highlight_segments(
    segments: &[CodeHighlightSegment],
    colors: &mut [Hsla],
    weights: &mut [FontWeight],
    styles: &mut [FontStyle],
) {
    let mut byte_pos = 0usize;
    for segment in segments {
        let end = (byte_pos + segment.len).min(colors.len());
        for idx in byte_pos..end {
            colors[idx] = segment.color;
            weights[idx] = segment.weight;
            styles[idx] = segment.style;
        }
        byte_pos = end;
    }
}

pub(crate) const GENERATED_PREVIEW_RASTER_SCALE: f32 = 2.0;
pub(crate) const GENERATED_PREVIEW_MATH_RASTER_SCALE: f32 = 3.0;
const GENERATED_PREVIEW_EDITOR_MAX_RASTER_EDGE: f32 = 2048.0;
const GENERATED_PREVIEW_EDITOR_MAX_RASTER_PIXELS: f32 = 2_097_152.0;
const GENERATED_PREVIEW_OVERLAY_MAX_RASTER_EDGE: f32 = 4096.0;
const GENERATED_PREVIEW_OVERLAY_MAX_RASTER_PIXELS: f32 = 8_388_608.0;
const GENERATED_PREVIEW_DEFAULT_MONOSPACE_FONT_FAMILY: &str =
    "ui-monospace, Menlo, Monaco, Consolas, monospace";
const GENERATED_PREVIEW_DEFAULT_CJK_FONT_FAMILY: &str =
    "PingFang SC, Hiragino Sans GB, Microsoft YaHei, Noto Sans CJK SC, sans-serif";
const GENERATED_PREVIEW_DEFAULT_EMOJI_FONT_FAMILY: &str =
    "Apple Color Emoji, Noto Color Emoji, Segoe UI Emoji";

#[derive(Clone, Copy)]
pub(crate) struct GeneratedPreviewFontFamilies<'a> {
    pub monospace: &'a str,
    pub body_cjk: &'a str,
    pub mermaid_theme: MermaidTheme,
}

pub(crate) fn generated_preview_raster_scale(kind: BlockPreviewKind) -> f32 {
    match kind {
        BlockPreviewKind::Math => GENERATED_PREVIEW_MATH_RASTER_SCALE,
        BlockPreviewKind::Mermaid => GENERATED_PREVIEW_RASTER_SCALE,
    }
}

/// Calculate the display width of a string, counting CJK characters as 2 columns.
pub(crate) fn display_width(s: &str) -> usize {
    s.chars().map(|ch| char_display_width(ch)).sum()
}

fn char_display_width(ch: char) -> usize {
    let code = ch as u32;
    if (0x1100..=0x115F).contains(&code)
        || (0x2E80..=0x303E).contains(&code)
        || (0x3040..=0x33BF).contains(&code)
        || (0x3400..=0x4DBF).contains(&code)
        || (0x4E00..=0x9FFF).contains(&code)
        || (0xA000..=0xA4CF).contains(&code)
        || (0xAC00..=0xD7AF).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
        || (0xFE30..=0xFE6F).contains(&code)
        || (0xFF01..=0xFF60).contains(&code)
        || (0xFFE0..=0xFFE6).contains(&code)
        || (0x20000..=0x2FA1F).contains(&code)
    {
        2
    } else {
        1
    }
}

pub(crate) fn prefers_latin_text_font(ch: char) -> bool {
    matches!(
        ch,
        '\u{0000}'..='\u{024F}'
            | '\u{1E00}'..='\u{1EFF}'
            | '\u{2000}'..='\u{206F}'
            | '\u{20A0}'..='\u{20CF}'
    )
}

pub(crate) fn task_visual_indent_display_columns(
    leading_ws: usize,
    visual_indent_step_display_columns: usize,
    task_source_indent_step_chars: usize,
) -> usize {
    if visual_indent_step_display_columns == 0 || task_source_indent_step_chars == 0 {
        return 0;
    }

    let indent_levels = leading_ws / task_source_indent_step_chars;
    indent_levels * visual_indent_step_display_columns.max(1)
}

pub(crate) fn task_prefix_display_columns(
    body_space_width: f32,
    checkbox_size_px: f32,
    checkbox_text_gap_px: f32,
) -> usize {
    if body_space_width <= 0.0 {
        return 1;
    }

    ((checkbox_size_px + checkbox_text_gap_px) / body_space_width)
        .round()
        .max(1.0) as usize
}

pub(crate) fn task_prefix_target_width_px(
    line_content: &str,
    checkbox_size_px: f32,
    checkbox_text_gap_px: f32,
    task_source_indent_step_chars: usize,
) -> Option<f32> {
    if task_source_indent_step_chars == 0 {
        return None;
    }

    let indent_levels = list_marker_body_indent(line_content) / task_source_indent_step_chars;
    Some((indent_levels + 1) as f32 * (checkbox_size_px + checkbox_text_gap_px))
}

/// Byte offset at which a line's list/task marker starts: the blockquote
/// prefix (if any) plus the indentation that follows it.
pub(crate) fn list_marker_indent(line_content: &str) -> usize {
    let quote_len = markdown::blockquote_prefix_len(line_content);
    quote_len + list_marker_body_indent(line_content)
}

/// Indentation after the blockquote prefix, i.e. the real nesting depth.
pub(crate) fn list_marker_body_indent(line_content: &str) -> usize {
    let body = &line_content[markdown::blockquote_prefix_len(line_content)..];
    body.len() - body.trim_start_matches([' ', '\t']).len()
}

fn active_range_intersects_target(active_range: &Range<usize>, target: &Range<usize>) -> bool {
    if active_range.start == active_range.end {
        active_range.start >= target.start && active_range.start < target.end
    } else {
        active_range.start < target.end && active_range.end > target.start
    }
}

pub(crate) fn active_ranges_intersect(
    active_ranges: &[Range<usize>],
    target: &Range<usize>,
) -> bool {
    active_ranges
        .iter()
        .any(|active_range| active_range_intersects_target(active_range, target))
}

pub(crate) fn inline_range_is_collapsed(
    decoration: &LineDecoration,
    visible_range: &Range<usize>,
    active_ranges: &[Range<usize>],
) -> bool {
    decoration
        .syntax_ranges
        .iter()
        .filter(|syntax_range| {
            syntax_range.trigger_range.start <= visible_range.start
                && syntax_range.trigger_range.end >= visible_range.end
        })
        .map(|syntax_range| syntax_range.trigger_range.clone())
        .min_by_key(|range| (range.end.saturating_sub(range.start), range.start))
        .is_none_or(|trigger_range| !active_ranges_intersect(active_ranges, &trigger_range))
}

pub(crate) fn block_preview_is_active(
    preview: &BlockPreview,
    cursor_line: usize,
    selection_ranges: &[Range<usize>],
) -> bool {
    (preview.start_line..=preview.end_line).contains(&cursor_line)
        || selection_ranges.iter().any(|range| {
            range.start < preview.source_range.end && range.end > preview.source_range.start
        })
}

pub(crate) fn active_ranges_have_non_eol_position(
    active_ranges: &[Range<usize>],
    line_len: usize,
) -> bool {
    active_ranges.iter().any(|active_range| {
        active_range.start.min(line_len) < line_len || active_range.end.min(line_len) < line_len
    })
}

fn heading_syntax_is_active(decoration: &LineDecoration, active_ranges: &[Range<usize>]) -> bool {
    matches!(decoration.block_type, BlockType::Heading(_)) && !active_ranges.is_empty()
}

fn scaled_inline_placeholder(text: &str, scale: f32) -> String {
    let target_columns = ((display_width(text) as f32) * scale).round() as usize;
    " ".repeat(target_columns.max(1))
}

/// Scalar properties are row-local. Value groups supply a synthetic active
/// range on every member row before reaching the display mapper.
pub(crate) fn front_matter_row_is_active(active_ranges: &[Range<usize>], line_len: usize) -> bool {
    active_ranges.iter().any(|range| {
        if range.is_empty() {
            range.start <= line_len
        } else {
            range.start <= line_len && range.end > 0
        }
    })
}

/// Delimiters share block focus; individual property syntax remains row-local.
#[cfg(test)]
pub(crate) fn front_matter_block_is_active(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    ranges: &[Range<usize>],
    expansion_allowed: bool,
) -> bool {
    front_matter_block_is_active_with_search(buffer, decorations, ranges, expansion_allowed, None)
}

pub(crate) fn front_matter_block_is_active_with_search(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    ranges: &[Range<usize>],
    expansion_allowed: bool,
    search_reveal_range: Option<&Range<usize>>,
) -> bool {
    let Some(table) = decorations.first().and_then(|d| d.table_info.as_ref()) else {
        return false;
    };
    if !table.is_front_matter
        || table.table_end_line == 0
        || table.table_end_line > buffer.line_count()
    {
        return false;
    }
    front_matter_source_rows_are_revealed(
        buffer,
        table.table_start_line..table.table_end_line,
        ranges,
        expansion_allowed,
        search_reveal_range,
    )
}

fn front_matter_source_rows_are_revealed(
    buffer: &Buffer,
    rows: Range<usize>,
    ranges: &[Range<usize>],
    expansion_allowed: bool,
    search_reveal_range: Option<&Range<usize>>,
) -> bool {
    (expansion_allowed && front_matter_source_rows_are_active(buffer, rows.clone(), ranges))
        || search_reveal_range.is_some_and(|range| {
            range.start < range.end
                && front_matter_source_rows_are_active(buffer, rows, std::slice::from_ref(range))
        })
}

fn front_matter_source_rows_are_active(
    buffer: &Buffer,
    rows: Range<usize>,
    ranges: &[Range<usize>],
) -> bool {
    if rows.is_empty() || rows.end > buffer.line_count() {
        return false;
    }
    let start = buffer.line_start(rows.start);
    let caret_end = buffer.line_end(rows.end - 1);
    let selection_end = buffer.line_start(rows.end);
    ranges.iter().any(|range| {
        if range.is_empty() {
            start <= range.start && range.start <= caret_end
        } else {
            range.start < selection_end && range.end > start
        }
    })
}

pub(crate) struct FrontMatterGroupPlan {
    pub group: super::front_matter_groups::ValueGroup,
    pub active: bool,
    pub collapse_header: bool,
    pub source_visible: bool,
    body_indent_columns: usize,
}

impl FrontMatterGroupPlan {
    pub fn hides_line(&self, line: usize) -> bool {
        use super::front_matter_groups::GroupKind;
        !self.active
            && !self.source_visible
            && self.group.source_lines.contains(&line)
            && match self.group.kind {
                GroupKind::Multiline => self.collapse_header && line == self.group.header_line,
                GroupKind::List => line != self.group.header_line,
            }
    }

    pub fn label_line(&self) -> usize {
        use super::front_matter_groups::GroupKind;
        match self.group.kind {
            GroupKind::Multiline
                if !self.active && !self.source_visible && self.collapse_header =>
            {
                self.group.header_line + 1
            }
            _ => self.group.header_line,
        }
    }
}

impl EditorView {
    pub(super) fn front_matter_delimiters_are_visible(&self) -> bool {
        let ranges: Vec<_> = self
            .all_cursor_ranges()
            .into_iter()
            .map(|(head, anchor)| head.min(anchor)..head.max(anchor))
            .chain(self.marked_ranges().cloned())
            .collect();
        front_matter_block_is_active_with_search(
            &self.buffer,
            &self.decorations,
            &ranges,
            self.markdown_syntax_expansion_allowed(),
            self.current_search_source_range().as_ref(),
        )
    }

    pub(super) fn front_matter_presentation_line_is_hidden(&self, line: usize) -> bool {
        self.front_matter_line_is_hidden(line)
            || (self.front_matter_summary_line() != Some(line)
                && self
                    .decorations
                    .get(line)
                    .and_then(|d| d.table_info.as_ref())
                    .is_some_and(|table| table.is_front_matter)
                && self
                    .front_matter_group_plan()
                    .iter()
                    .any(|plan| plan.hides_line(line)))
    }

    /// Caret movement skips interior rows and unfocused delimiters. The summary
    /// is excluded so the empty-metadata fallback on the opener stays a stop.
    pub(super) fn front_matter_navigation_skips_line(&self, line: usize) -> bool {
        self.front_matter_presentation_line_is_hidden(line)
            || (self.front_matter_summary_line() != Some(line)
                && !self.front_matter_delimiters_are_visible()
                && self
                    .decorations
                    .get(line)
                    .and_then(|d| d.table_info.as_ref())
                    .is_some_and(|table| table.is_front_matter && table.is_separator))
    }

    pub(super) fn front_matter_group_plan(&self) -> Vec<FrontMatterGroupPlan> {
        self.front_matter_group_plan_for_width(self.last_viewport_width)
    }

    pub(super) fn front_matter_group_plan_for_width(
        &self,
        viewport_width: f32,
    ) -> Vec<FrontMatterGroupPlan> {
        if self.collapsed_front_matter_range().is_some() {
            return Vec::new();
        }
        let mut ranges: Vec<_> = self
            .all_cursor_ranges()
            .into_iter()
            .map(|(head, anchor)| head.min(anchor)..head.max(anchor))
            .collect();
        ranges.extend(self.marked_ranges().cloned());
        let mut plans = front_matter_group_plan_with_search(
            &self.buffer,
            &self.decorations,
            &ranges,
            self.markdown_syntax_expansion_allowed(),
            self.current_search_source_range().as_ref(),
        );
        let editor_width = if self.editor_uses_expanded_layout() {
            viewport_width
        } else {
            viewport_width.min(self.max_editor_width())
        };
        let text_width =
            (editor_width - self.padding_left() - self.padding_right()).max(self.scaled(120.0));
        let (_, available_width) =
            super::element::front_matter_content_geometry(true, 0.0, text_width, self.scaled(1.0));
        if self.front_matter_columns.is_some() {
            super::front_matter_layout::constrain_groups(
                &mut plans,
                self.front_matter_columns,
                available_width,
                &self.decorations,
            );
            return plans;
        }
        // Before the first font measurement, use a conservative column budget.
        let column_width = self.font_size_for_block(&BlockType::Table)
            + self.styles.editor.editor_letter_spacing_px.max(0.0);
        apply_front_matter_group_width_limit(
            &mut plans,
            &self.decorations,
            (available_width - self.scaled(64.0)).max(0.0) / column_width.max(1.0),
        );
        plans
    }

    pub(super) fn sync_front_matter_group_focus(&mut self) -> bool {
        self.sync_front_matter_group_focus_for_width(self.last_viewport_width)
    }

    pub(super) fn sync_front_matter_group_focus_for_width(&mut self, width: f32) -> bool {
        let mut signature: Vec<_> = self
            .front_matter_group_plan_for_width(width)
            .iter()
            .map(|plan| {
                (
                    plan.group.source_lines.clone(),
                    plan.active,
                    plan.source_visible,
                )
            })
            .collect();
        if let Some(table) = self
            .decorations
            .first()
            .and_then(|d| d.table_info.as_ref())
            .filter(|table| table.is_front_matter)
        {
            // The existing pane-local signature also owns block delimiter geometry.
            signature.insert(
                0,
                (
                    table.table_start_line..table.table_end_line,
                    self.front_matter_delimiters_are_visible(),
                    self.collapsed_front_matter_range().is_some(),
                ),
            );
            // Scalar-row focus also changes source mapping before prepaint,
            // even when block delimiter visibility itself does not change.
            let ranges: Vec<_> = self
                .all_cursor_ranges()
                .into_iter()
                .map(|(head, anchor)| head.min(anchor)..head.max(anchor))
                .chain(self.marked_ranges().cloned())
                .collect();
            let search_reveal_range = self.current_search_source_range();
            for line in table.table_start_line + 1..table.table_end_line - 1 {
                signature.push((
                    line..line + 1,
                    front_matter_source_rows_are_revealed(
                        &self.buffer,
                        line..line + 1,
                        &ranges,
                        self.markdown_syntax_expansion_allowed(),
                        search_reveal_range.as_ref(),
                    ),
                    false,
                ));
            }
        }
        if signature == self.front_matter_group_focus {
            return false;
        }
        self.front_matter_group_focus = signature;
        self.front_matter_group_geometry_dirty = true;
        self.last_stable_input_bounds = None;
        self.pending_input_bounds = None;
        // Row visibility changes block height, including offscreen members.
        if let Some(table) = self
            .decorations
            .first()
            .and_then(|d| d.table_info.as_ref())
            .filter(|t| t.is_front_matter)
        {
            for line in
                table.table_start_line..table.table_end_line.min(self.line_display_info.len())
            {
                self.line_display_info[line] = None;
            }
            self.clear_sparse_line_geometry_range(table.table_start_line..table.table_end_line);
        }
        self.invalidate_line_geometry_cache();
        self.visible_line_geometry.clear();
        self.front_matter_type_areas.clear();
        self.gutter_fold_areas.clear();
        self.front_matter_text_areas.clear();
        self.front_matter_delete_areas.clear();
        self.front_matter_list_scroll_areas.clear();
        true
    }
}

pub(crate) fn apply_front_matter_group_width_limit(
    plans: &mut [FrontMatterGroupPlan],
    decorations: &[LineDecoration],
    available_columns: f32,
) {
    for plan in plans {
        if matches!(plan.group.kind, super::front_matter_groups::GroupKind::List) {
            let columns = decorations
                .get(plan.group.header_line)
                .and_then(|d| d.table_info.as_ref())
                .and_then(|t| t.column_widths.first())
                .copied()
                .unwrap_or(3)
                + plan.body_indent_columns
                + 2;
            plan.source_visible |= columns as f32 > available_columns;
        }
    }
}

fn front_matter_list_has_comments(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    group: &super::front_matter_groups::ValueGroup,
) -> bool {
    let header = group.header_line;
    let header_comment = decorations
        .get(header)
        .and_then(|d| d.front_matter.as_ref())
        .and_then(|info| buffer.line_content(header).get(info.value_range.end..))
        .is_some_and(|suffix| suffix.trim_start().starts_with('#'));
    header_comment
        || group
            .source_lines
            .clone()
            .any(|line| buffer.line_content(line).trim_start().starts_with('#'))
        || group.items.iter().any(|item| {
            buffer
                .line_content(item.source_line)
                .get(item.source_range.end - buffer.line_start(item.source_line)..)
                .is_some_and(|suffix| suffix.trim_start().starts_with('#'))
        })
}

#[cfg(test)]
pub(crate) fn front_matter_group_plan(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    ranges: &[Range<usize>],
    expansion_allowed: bool,
) -> Vec<FrontMatterGroupPlan> {
    front_matter_group_plan_with_search(buffer, decorations, ranges, expansion_allowed, None)
}

pub(crate) fn front_matter_group_plan_with_search(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    ranges: &[Range<usize>],
    expansion_allowed: bool,
    search_reveal_range: Option<&Range<usize>>,
) -> Vec<FrontMatterGroupPlan> {
    super::front_matter_groups::groups(buffer, decorations)
        .into_iter()
        .map(|group| {
            let active = front_matter_source_rows_are_revealed(
                buffer,
                group.source_lines.clone(),
                ranges,
                expansion_allowed,
                search_reveal_range,
            );
            let collapse_header = group.source_lines.end > group.header_line + 1
                && decorations
                    .get(group.header_line)
                    .and_then(|d| d.front_matter.as_ref())
                    .is_some_and(|info| matches!(info.value, FrontMatterValue::BlockHeader));
            let body_indent_columns = decorations
                .get(group.header_line)
                .and_then(|d| d.front_matter.as_ref())
                .map_or(0, |info| info.indent_columns);
            let source_visible = matches!(group.kind, super::front_matter_groups::GroupKind::List)
                && front_matter_list_has_comments(buffer, decorations, &group);
            FrontMatterGroupPlan {
                group,
                active,
                collapse_header,
                body_indent_columns,
                source_visible,
            }
        })
        .collect()
}

/// Collapsed list syntax is painted as separate, unwrapped item layouts. The
/// key retains its ordinary source mapping in both prepaint and instant shape.
pub(crate) fn apply_front_matter_group_display(
    decoration: &mut LineDecoration,
    line: usize,
    plan: Option<&FrontMatterGroupPlan>,
    source: &str,
) {
    if plan.is_some_and(|plan| plan.source_visible) {
        if let Some(info) = decoration.front_matter.as_mut() {
            info.key_range = None;
            info.value_range = 0..source.len();
            info.value = FrontMatterValue::Raw;
        }
        return;
    }
    if let Some(plan) = plan
        && line != plan.group.header_line
        && matches!(
            plan.group.kind,
            super::front_matter_groups::GroupKind::Multiline
        )
        && source.trim().is_empty()
        && let Some(info) = decoration.front_matter.as_mut()
    {
        info.value_range = source.len()..source.len();
        info.indent_columns = plan.body_indent_columns;
        info.value = FrontMatterValue::Text(String::new());
    }
    if let Some(plan) = plan
        && !plan.active
        && line == plan.group.header_line
        && matches!(plan.group.kind, super::front_matter_groups::GroupKind::List)
        && let Some(info) = decoration.front_matter.as_mut()
    {
        info.value_range.end = source.len();
        info.value = FrontMatterValue::Text(String::new());
    }
}

/// Byte maps use decoded character source spans, never proportional byte ratios.
pub(crate) fn front_matter_item_source_map(
    source: &str,
    display: &str,
    start: usize,
) -> Vec<usize> {
    let ranges = front_matter::decoded_char_ranges(source, display).or_else(|| {
        if !source.starts_with(['\'', '"']) {
            return None;
        }
        let decoded = serde_yaml::from_str::<String>(source).ok()?;
        let ranges = front_matter::decoded_char_ranges(source, &decoded)?;
        let mut chars = decoded.chars().zip(ranges).peekable();
        let mut normalized = String::new();
        let mut spans = Vec::new();
        while let Some((ch, mut span)) = chars.next() {
            if ch == '\r' && chars.peek().is_some_and(|(next, _)| *next == '\n') {
                span.end = chars.next()?.1.end;
            }
            normalized.push(
                if matches!(ch, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}') {
                    ' '
                } else {
                    ch
                },
            );
            spans.push(span);
        }
        (normalized == display).then_some(spans)
    });
    let mut map = Vec::with_capacity(display.len() + 1);
    if let Some(ranges) = ranges {
        for (ch, range) in display.chars().zip(ranges) {
            map.extend(std::iter::repeat_n(start + range.start, ch.len_utf8()));
        }
    } else {
        map.resize(display.len(), start);
    }
    map.push(start + source.len());
    map
}

pub(crate) fn front_matter_display_selection_ranges(
    display: &str,
    source_map: &[usize],
    selections: &[Range<usize>],
) -> Vec<Range<usize>> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    for (index, ch) in display.char_indices() {
        let end = index + ch.len_utf8();
        let (Some(&source_start), Some(&source_end)) = (source_map.get(index), source_map.get(end))
        else {
            continue;
        };
        if selections.iter().any(|selection| {
            !selection.is_empty() && selection.start < source_end && selection.end > source_start
        }) {
            if let Some(last) = spans.last_mut()
                && last.end == index
            {
                last.end = end;
            } else {
                spans.push(index..end);
            }
        }
    }
    spans
}

struct FrontMatterDisplayMapping<'a> {
    source: &'a str,
    text: String,
    display_to_buffer: Vec<usize>,
    buffer_to_display: Vec<usize>,
}

impl<'a> FrontMatterDisplayMapping<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            text: String::new(),
            display_to_buffer: Vec::new(),
            buffer_to_display: vec![0; source.len() + 1],
        }
    }

    fn copy(&mut self, range: Range<usize>) {
        for (offset, ch) in self.source[range.clone()].char_indices() {
            let source_start = range.start + offset;
            let display_start = self.text.len();
            self.text.push(ch);
            // Interior UTF-8 bytes point to their character's start, never to
            // an invalid boundary. Unchanged characters retain exact identity.
            self.display_to_buffer
                .extend(std::iter::repeat_n(source_start, ch.len_utf8()));
            self.buffer_to_display[source_start..source_start + ch.len_utf8()].fill(display_start);
        }
        self.buffer_to_display[range.end] = self.text.len();
    }

    fn replace(&mut self, range: Range<usize>, replacement: &str) {
        let display_start = self.text.len();
        self.buffer_to_display[range.clone()].fill(display_start);
        let token = &self.source[range.clone()];
        if let Some(char_ranges) = front_matter::decoded_char_ranges(token, replacement) {
            let mut source_cursor = range.start;
            for (ch, char_range) in replacement.chars().zip(char_ranges) {
                let char_start = range.start + char_range.start;
                let char_end = range.start + char_range.end;
                self.buffer_to_display[source_cursor..char_end].fill(self.text.len());
                self.display_to_buffer
                    .extend(std::iter::repeat_n(char_start, ch.len_utf8()));
                self.text.push(ch);
                source_cursor = char_end;
            }
            self.buffer_to_display[source_cursor..range.end].fill(self.text.len());
        } else {
            // Synthetic padding, checkboxes and formatted dates have no
            // per-character source identity. Anchor them at the token start.
            self.display_to_buffer
                .extend(std::iter::repeat_n(range.start, replacement.len()));
            self.text.push_str(replacement);
        }
        self.buffer_to_display[range.end] = self.text.len();
    }

    fn finish(mut self, prefix_collapsed: bool) -> (String, Vec<usize>, Vec<usize>, bool) {
        self.buffer_to_display[self.source.len()] = self.text.len();
        (
            self.text,
            self.display_to_buffer,
            self.buffer_to_display,
            prefix_collapsed,
        )
    }
}

/// Synthetic summary bytes all anchor to the delimiter, never to proportional
/// source offsets (which could land inside UTF-8 or hidden YAML).
pub(crate) fn build_front_matter_fold_display(
    source: &str,
    summary: &str,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    let mut mapping = FrontMatterDisplayMapping::new(source);
    mapping.replace(0..source.len(), summary);
    let (text, mut display_to_buffer, buffer_to_display, collapsed) = mapping.finish(false);
    display_to_buffer.push(source.len());
    (text, display_to_buffer, buffer_to_display, collapsed)
}

fn build_front_matter_display_text(
    source: &str,
    decoration: &LineDecoration,
    active_ranges: &[Range<usize>],
    key_padding: usize,
    boolean_columns: usize,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    let mut mapping = FrontMatterDisplayMapping::new(source);
    let Some(info) = decoration.front_matter.as_ref() else {
        mapping.copy(0..source.len());
        return mapping.finish(false);
    };
    let active = front_matter_row_is_active(active_ranges, source.len());
    let value_range = info.value_range.clone();
    let valid_ranges = value_range.start <= value_range.end
        && source.get(value_range.clone()).is_some()
        && info.key_range.as_ref().is_none_or(|key| {
            key.start <= key.end
                && key.end <= value_range.start
                && source.get(key.clone()).is_some()
        });
    // A truly empty key-less row has its key slot at column zero, where typed
    // text is inserted. Padding it would move only the caret to the value column.
    if !valid_ranges
        || (info.key_range.is_none()
            && (matches!(info.value, FrontMatterValue::Raw) || source.is_empty()))
    {
        mapping.copy(0..source.len());
        return mapping.finish(false);
    }
    let max_key_width = decoration
        .table_info
        .as_ref()
        .and_then(|table| table.column_widths.first())
        .copied()
        .unwrap_or(3);
    let key_padding = key_padding.max(1);
    if let Some(key) = info.key_range.as_ref() {
        let Some(colon_offset) = source[key.end..value_range.start].find(':') else {
            mapping.copy(0..source.len());
            return mapping.finish(false);
        };
        let colon_end = key.end + colon_offset + 1;
        if !active
            && !info.display_prefix.is_empty()
            && let Some(marker) = source[..key.start].find('-')
        {
            mapping.copy(0..marker);
            mapping.replace(marker..key.start, &info.display_prefix);
            mapping.copy(key.start..colon_end);
        } else {
            mapping.copy(0..colon_end);
        }
        let padding =
            max_key_width.saturating_sub(display_width(&source[key.clone()])) + key_padding;
        mapping.replace(colon_end..value_range.start, &" ".repeat(padding));
    } else {
        let padding = max_key_width + 1 + key_padding + info.indent_columns;
        let marker = source[..value_range.start].find('-');
        if active && let Some(marker) = marker {
            let prefix = &source[marker..value_range.start];
            if prefix[1..].chars().all(char::is_whitespace) {
                mapping.replace(0..marker, &" ".repeat(padding.saturating_sub(2)));
                mapping.copy(marker..marker + 1);
                mapping.replace(marker + 1..value_range.start, " ");
            } else {
                let marker_width = display_width(prefix);
                mapping.replace(0..marker, &" ".repeat(padding.saturating_sub(marker_width)));
                mapping.copy(marker..value_range.start);
            }
        } else {
            mapping.replace(
                0..value_range.start,
                &" ".repeat(padding.saturating_sub(display_width(&info.display_prefix))),
            );
            if !info.display_prefix.is_empty() {
                // Keep the bullet mapped to the sequence marker, not its scalar.
                mapping.text.push_str(&info.display_prefix);
                mapping.display_to_buffer.extend(std::iter::repeat_n(
                    marker.unwrap_or(0),
                    info.display_prefix.len(),
                ));
            }
        }
    }
    if active {
        mapping.copy(value_range.clone());
    } else {
        match &info.value {
            FrontMatterValue::Text(text)
                if !text.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) =>
            {
                mapping.replace(value_range.clone(), text);
            }
            FrontMatterValue::Boolean(_) => {
                mapping.replace(
                    value_range.clone(),
                    &"\u{a0}".repeat(boolean_columns.max(3)),
                );
            }
            FrontMatterValue::BlockHeader => mapping.replace(value_range.clone(), ""),
            // Date formatting belongs to the editor settings helper. Until it
            // supplies Text(formatted), leave the exact ISO source token alone.
            _ => mapping.copy(value_range.clone()),
        }
    }
    mapping.copy(value_range.end..source.len());
    mapping.finish(!active && !info.display_prefix.is_empty())
}

// Retain math-preview provenance after removing rich metadata. A canonical fence
// language is only its first info-string word, so it cannot produce this tag.
// The leading `latex` also keeps grammar lookup valid for the projected row.
pub(super) const SOURCE_MATH_HIGHLIGHT_LANGUAGE: &str = "latex (source math)";

/// Source is expanded Markdown, not a code editor. Remove rich block presentation
/// but retain its syntax language; ordinary Markdown keeps its roles and spans.
pub(super) fn source_markdown_decoration(decoration: &mut LineDecoration) {
    if matches!(
        decoration.block_type,
        BlockType::CodeBlock | BlockType::CodeBlockFence | BlockType::Table
    ) || decoration.block_preview.is_some()
        || decoration.front_matter.is_some()
    {
        let preview_language =
            decoration
                .block_preview
                .as_ref()
                .map(|preview| match preview.kind {
                    BlockPreviewKind::Math => SOURCE_MATH_HIGHLIGHT_LANGUAGE,
                    BlockPreviewKind::Mermaid => "mermaid",
                });
        let is_table = decoration.front_matter.is_none()
            && decoration.table_info.as_ref().map_or(
                decoration.block_type == BlockType::Table
                    || decoration.code_language.as_deref() == Some("table"),
                |table| !table.is_front_matter,
            );
        let language = if is_table {
            // Keep ink, not table ownership: native text shaping must see raw
            // pipes/cells without invoking rich layout or hiding separator rows.
            Some("markdown".to_string())
        } else if let Some(language) = preview_language {
            Some(language.to_string())
        } else if decoration.table_info.is_none()
            && decoration.front_matter.is_none()
            && matches!(
                decoration.block_type,
                BlockType::CodeBlock | BlockType::CodeBlockFence
            )
        {
            // Closing fences may lack a parser language. Markdown is sufficient
            // for their literal markers; never infer a language for code bodies.
            decoration.code_language.take().or_else(|| {
                (decoration.block_type == BlockType::CodeBlockFence).then(|| "markdown".to_string())
            })
        } else {
            None
        };
        let added = std::mem::take(&mut decoration.diff_added_spans);
        let removed = std::mem::take(&mut decoration.diff_removed_spans);
        *decoration = LineDecoration::default();
        if language.is_some() || !added.is_empty() || !removed.is_empty() {
            decoration.code_language = language;
            decoration.diff_added_spans = added;
            decoration.diff_removed_spans = removed;
        }
    } else if let Some(payload) = decoration.payload_mut() {
        payload.image_path = None;
        // Some replacements (notably HTML breaks) survive even a full active
        // range. Source must keep their exact bytes and identity source maps.
        payload.display_replacements.clear();
        payload.inline_math_spans.clear();
    }
}

/// Language grammars do not consistently color Markdown fences or standalone
/// math delimiters. This is ink only, on projected source rows, never rich UI.
fn apply_source_block_marker_colors(
    line: &str,
    language: &str,
    theme: &Theme,
    colors: &mut [Hsla],
) {
    let trimmed = line.trim();
    let start = line.len() - line.trim_start().len();
    if let Some(marker @ (b'`' | b'~')) = trimmed.bytes().next() {
        let count = trimmed.bytes().take_while(|byte| *byte == marker).count();
        if count >= 3 {
            colors[start..start + count].fill(theme.syntax_marker);
            return;
        }
    }
    if language == SOURCE_MATH_HIGHLIGHT_LANGUAGE {
        for marker in ["$$", "\\[", "\\]"] {
            if trimmed.starts_with(marker) {
                colors[start..start + marker.len()].fill(theme.syntax_marker);
            }
            if trimmed.ends_with(marker) {
                let end = start + trimmed.len();
                colors[end - marker.len()..end].fill(theme.syntax_marker);
            }
        }
    }
}

/// Expansion state is presentation-only: never synthesize a cursor selection.
pub(super) fn expand_source_syntax(
    source_mode: bool,
    line_len: usize,
    active_ranges: &mut Vec<Range<usize>>,
) {
    if source_mode {
        active_ranges.clear();
        active_ranges.push(0..line_len);
    }
}

/// Build display text by selectively hiding syntax ranges.
pub(crate) fn build_display_text(
    line_content: &str,
    decoration: &LineDecoration,
    active_ranges: &[Range<usize>],
    code_block_syntax_active: bool,
    table_width_chars: Option<usize>,
    table_cell_padding_chars: usize,
    collapsed_task_prefix_chars: usize,
    task_indent_step_display_columns: usize,
    task_source_indent_step_chars: usize,
    front_matter_key_padding_chars: usize,
    superscript_scale_factor: f32,
    subscript_scale_factor: f32,
    _footnote_scale_factor: f32,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    fn append_mapped_replacement(
        display_text: &mut String,
        display_to_buffer: &mut Vec<usize>,
        buffer_to_display: &mut [usize],
        display_pos: &mut usize,
        source: &str,
        source_range: Range<usize>,
        replacement: &str,
        anchor_source_start: bool,
    ) {
        let replacement_display_start = *display_pos;
        display_text.push_str(replacement);

        let source_start = source_range.start.min(buffer_to_display.len());
        let source_end = source_range.end.min(buffer_to_display.len());
        let source_len = source_end.saturating_sub(source_start);

        if replacement.is_empty() || source_len == 0 {
            for _ in 0..replacement.len() {
                display_to_buffer.push(source_end);
            }
            for buf_i in source_start..source_end {
                buffer_to_display[buf_i] = replacement_display_start;
            }
            *display_pos += replacement.len();
            return;
        }

        let replacement_char_starts: Vec<usize> = replacement
            .char_indices()
            .map(|(char_start, _)| char_start)
            .collect();
        let mut replacement_segments = Vec::with_capacity(replacement_char_starts.len());

        for (index, char_start) in replacement_char_starts.iter().copied().enumerate() {
            let char_end = replacement_char_starts
                .get(index + 1)
                .copied()
                .unwrap_or(replacement.len());
            let mut source_offset =
                (char_start * source_len / replacement.len()).min(source_len - 1);
            while source_offset > 0 && !source.is_char_boundary(source_start + source_offset) {
                source_offset -= 1;
            }
            replacement_segments.push((source_offset, char_start));
            for _ in char_start..char_end {
                display_to_buffer.push(source_start + source_offset);
            }
        }

        let mut segment_index = 0usize;
        for source_offset in 0..source_len {
            while segment_index + 1 < replacement_segments.len()
                && replacement_segments[segment_index + 1].0 <= source_offset
            {
                segment_index += 1;
            }
            buffer_to_display[source_start + source_offset] =
                replacement_display_start + replacement_segments[segment_index].1;
        }
        // Expansion can map several display characters to the source start.
        // Anchor that endpoint to the first one so the full replacement is reserved.
        if anchor_source_start {
            buffer_to_display[source_start] = replacement_display_start;
        }

        *display_pos += replacement.len();
    }

    let len = line_content.len();
    let leading_ws = list_marker_indent(line_content);
    let body_indent = list_marker_body_indent(line_content);
    let trimmed = &line_content[leading_ws..];
    let task_core_len = task_core_syntax_len(trimmed);
    let task_fold_len = task_prefix_len(trimmed);

    if decoration
        .table_info
        .as_ref()
        .is_some_and(|table| table.is_front_matter)
    {
        return build_front_matter_display_text(
            line_content,
            decoration,
            active_ranges,
            front_matter_key_padding_chars,
            collapsed_task_prefix_chars,
        );
    }

    if decoration.block_type == BlockType::Table {
        if let Some(ref table_info) = decoration.table_info {
            return build_table_display_text(
                line_content,
                table_info,
                active_ranges,
                code_block_syntax_active,
                table_width_chars,
                table_cell_padding_chars,
                collapsed_task_prefix_chars,
                task_indent_step_display_columns,
                task_source_indent_step_chars,
                front_matter_key_padding_chars,
            );
        }
    }

    if decoration.block_type == BlockType::CodeBlockFence {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let mut buffer_to_display: Vec<usize> = (0..len).collect();
        buffer_to_display.push(len);
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    let mut display_replacements: Vec<DisplayReplacement> =
        decoration.display_replacements.iter().cloned().collect();

    for fragment in &decoration.table_cell_fragments {
        if !table_cell_fragment_prefix_is_folded(line_content, fragment, active_ranges) {
            continue;
        }
        let source = &line_content[fragment.range.clone()];
        let display_columns = if matches!(fragment.block_type, BlockType::TaskList(_)) {
            task_visual_indent_display_columns(
                list_marker_body_indent(source),
                task_indent_step_display_columns,
                task_source_indent_step_chars,
            ) + collapsed_task_prefix_chars.max(1)
        } else {
            fragment.prefix_len
        };
        display_replacements.push(DisplayReplacement {
            source_range: fragment.range.start..fragment.range.start + fragment.prefix_len,
            display_text: " ".repeat(display_columns),
            trigger_range: fragment.range.start..fragment.range.start + fragment.prefix_len,
        });
    }

    for superscript_range in &decoration.superscript_spans {
        if !inline_range_is_collapsed(decoration, superscript_range, active_ranges) {
            continue;
        }
        let Some(text) = line_content.get(superscript_range.clone()) else {
            continue;
        };
        display_replacements.push(DisplayReplacement {
            source_range: superscript_range.clone(),
            display_text: scaled_inline_placeholder(text, superscript_scale_factor),
            trigger_range: superscript_range.clone(),
        });
    }

    for subscript_range in &decoration.subscript_spans {
        if !inline_range_is_collapsed(decoration, subscript_range, active_ranges) {
            continue;
        }
        let Some(text) = line_content.get(subscript_range.clone()) else {
            continue;
        };
        display_replacements.push(DisplayReplacement {
            source_range: subscript_range.clone(),
            display_text: scaled_inline_placeholder(text, subscript_scale_factor),
            trigger_range: subscript_range.clone(),
        });
    }

    for footnote_reference in &decoration.footnote_references {
        if active_ranges_intersect(active_ranges, &footnote_reference.full_range) {
            continue;
        }
        let Some(text) = line_content.get(footnote_reference.collapsed_range.clone()) else {
            continue;
        };
        let display_text = if decoration.footnote_definition_label.is_some() {
            text.to_string()
        } else {
            scaled_inline_placeholder(text, _footnote_scale_factor)
        };
        display_replacements.push(DisplayReplacement {
            source_range: footnote_reference.collapsed_range.clone(),
            display_text,
            trigger_range: footnote_reference.full_range.clone(),
        });
    }

    display_replacements.retain(|replacement| {
        replacement.display_text == "\n"
            || decoration.table_cell_fragments.iter().any(|fragment| {
                fragment.prefix_len > 0
                    && replacement.source_range
                        == (fragment.range.start..fragment.range.start + fragment.prefix_len)
            })
            || !active_ranges_intersect(active_ranges, &replacement.trigger_range)
    });
    display_replacements.sort_by_key(|replacement| replacement.source_range.start);

    if decoration.syntax_ranges.is_empty() && display_replacements.is_empty() {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let mut buffer_to_display: Vec<usize> = (0..len).collect();
        buffer_to_display.push(len);
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    let hidden: Vec<bool> = decoration
        .syntax_ranges
        .iter()
        .map(|sr| {
            if decoration.block_type == BlockType::CodeBlockFence && code_block_syntax_active {
                return false;
            }
            if matches!(decoration.block_type, BlockType::TaskList(_))
                && task_core_len > 0
                && sr.trigger_range.start == leading_ws
                && sr.trigger_range.end <= leading_ws + task_fold_len.max(task_core_len)
            {
                return task_prefix_is_folded(
                    line_content,
                    leading_ws,
                    task_core_len,
                    active_ranges,
                );
            }
            if active_ranges.is_empty() {
                return true;
            }
            if matches!(decoration.block_type, BlockType::Heading(_)) {
                return !heading_syntax_is_active(decoration, active_ranges);
            }
            if active_ranges_intersect(active_ranges, &sr.trigger_range) {
                return false;
            }
            if active_ranges_have_non_eol_position(active_ranges, len) {
                return true;
            }
            !matches!(
                decoration.block_type,
                BlockType::HorizontalRule | BlockType::CodeBlockFence
            )
        })
        .collect();

    if !hidden.iter().any(|&h| h) && display_replacements.is_empty() {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let mut buffer_to_display: Vec<usize> = (0..len).collect();
        buffer_to_display.push(len);
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    let mut is_hidden = vec![false; len];
    for (i, sr) in decoration.syntax_ranges.iter().enumerate() {
        if hidden[i] {
            let start = sr.range.start.min(len);
            let end = sr.range.end.min(len);
            for j in start..end {
                is_hidden[j] = true;
            }
        }
    }

    let prefix_range_and_replacement = match decoration.block_type {
        _ if !decoration.table_cell_fragments.is_empty() => None,
        BlockType::UnorderedList => {
            let fold_len = if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                2
            } else {
                0
            };
            (fold_len > 0 && prefix_is_folded(leading_ws, fold_len, active_ranges))
                .then_some((leading_ws..leading_ws + fold_len, "  ".to_string()))
        }
        BlockType::OrderedList => {
            let prefix_len = markdown::ordered_list_prefix_len(trimmed);
            (prefix_len > 0
                && !decoration.display_replacements.iter().any(|replacement| {
                    replacement.source_range == (leading_ws..leading_ws + prefix_len)
                })
                && prefix_is_folded(leading_ws, prefix_len, active_ranges))
            .then_some((
                leading_ws..leading_ws + prefix_len,
                trimmed[..prefix_len].to_string(),
            ))
        }
        BlockType::TaskList(_) => {
            let fold_len = task_fold_len;
            let visual_indent = task_visual_indent_display_columns(
                body_indent,
                task_indent_step_display_columns,
                task_source_indent_step_chars,
            );
            let replacement = " ".repeat(visual_indent + collapsed_task_prefix_chars.max(1));
            (fold_len > 0
                && task_prefix_is_folded(line_content, leading_ws, task_core_len, active_ranges))
            .then_some((0..leading_ws + fold_len, replacement))
        }
        _ => None,
    };

    let prefix_collapsed = prefix_range_and_replacement.is_some()
        || decoration
            .table_cell_fragments
            .first()
            .is_some_and(|fragment| {
                table_cell_fragment_prefix_is_folded(line_content, fragment, active_ranges)
            });

    let mut display_text = String::with_capacity(len);
    let mut display_to_buffer = Vec::with_capacity(len);
    let mut buffer_to_display = vec![0usize; len];
    let mut display_pos = 0;

    let mut replacement_index = 0usize;
    let mut buffer_pos = 0usize;
    while buffer_pos < len {
        if let Some((prefix_range, replacement)) = &prefix_range_and_replacement
            && buffer_pos == prefix_range.start
        {
            append_mapped_replacement(
                &mut display_text,
                &mut display_to_buffer,
                &mut buffer_to_display,
                &mut display_pos,
                line_content,
                prefix_range.clone(),
                replacement,
                true,
            );
            buffer_pos = prefix_range.end.min(len);
            continue;
        }

        if let Some(replacement) = display_replacements.get(replacement_index)
            && buffer_pos == replacement.source_range.start
        {
            append_mapped_replacement(
                &mut display_text,
                &mut display_to_buffer,
                &mut buffer_to_display,
                &mut display_pos,
                line_content,
                replacement.source_range.clone(),
                &replacement.display_text,
                // Math retains its interior hit mapping and derives the full span from its end.
                !decoration
                    .inline_math_spans
                    .contains(&replacement.source_range),
            );
            buffer_pos = replacement.source_range.end.min(len);
            replacement_index += 1;
            continue;
        }

        let next_buffer_pos = line_content[buffer_pos..]
            .chars()
            .next()
            .map(|ch| buffer_pos + ch.len_utf8())
            .unwrap_or(len);
        if is_hidden[buffer_pos] {
            for offset in buffer_pos..next_buffer_pos.min(len) {
                buffer_to_display[offset] = display_pos;
            }
        } else {
            display_text.push_str(&line_content[buffer_pos..next_buffer_pos]);
            for offset in buffer_pos..next_buffer_pos.min(len) {
                display_to_buffer.push(offset);
                buffer_to_display[offset] = display_pos + (offset - buffer_pos);
            }
            display_pos += next_buffer_pos - buffer_pos;
        }
        buffer_pos = next_buffer_pos;
    }

    buffer_to_display.push(display_text.len());

    (
        display_text,
        display_to_buffer,
        buffer_to_display,
        prefix_collapsed,
    )
}

/// Build display text for a folded table row.
pub(crate) fn build_table_display_text(
    line_content: &str,
    info: &TableLineInfo,
    _active_ranges: &[Range<usize>],
    _code_block_syntax_active: bool,
    available_chars: Option<usize>,
    table_cell_padding_chars: usize,
    collapsed_task_prefix_chars: usize,
    task_indent_step_display_columns: usize,
    task_source_indent_step_chars: usize,
    front_matter_key_padding_chars: usize,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    let len = line_content.len();
    let cell_padding = table_cell_padding_chars;
    let balanced_col_widths = compute_table_render_widths(info, available_chars, cell_padding);
    let target_row_chars = available_chars.unwrap_or_else(|| {
        balanced_col_widths
            .iter()
            .map(|width| width + cell_padding * 2)
            .sum::<usize>()
            .max(1)
    });

    if info.is_separator {
        let display = " ".repeat(target_row_chars.max(1));
        let d2b = vec![0; display.len()];
        let mut b2d = vec![0; len];
        b2d.push(display.len());
        return (display, d2b, b2d, false);
    }

    let mut display = String::new();
    let mut d2b: Vec<usize> = Vec::new();
    let mut b2d = vec![0usize; len];
    let mut consumed = 0usize;
    let mut display_pos = 0usize;

    for (ci, cell) in info.cells.iter().enumerate() {
        let cell_range = info
            .cell_ranges
            .get(ci)
            .cloned()
            .unwrap_or(consumed..consumed);

        let gap_start = if ci == 0 {
            consumed
        } else {
            consumed.saturating_add(1)
        };
        for buf_i in gap_start..cell_range.start.min(len) {
            b2d[buf_i] = display_pos;
        }

        let cell_decoration = markdown::parse_decorations(cell)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (folded_cell_raw, cell_d2b_raw, cell_b2d_raw, _) = build_display_text(
            cell,
            &cell_decoration,
            &[],
            false,
            None,
            0,
            collapsed_task_prefix_chars,
            task_indent_step_display_columns,
            task_source_indent_step_chars,
            front_matter_key_padding_chars,
            1.0,
            1.0,
            1.0,
        );
        let trimmed_range = table_cell_trimmed_display_range(&folded_cell_raw);
        let trimmed_start = trimmed_range.start;
        let folded_cell = folded_cell_raw[trimmed_range.clone()].to_string();
        let cell_d2b: Vec<usize> = cell_d2b_raw[trimmed_range].to_vec();
        let cell_b2d: Vec<usize> = cell_b2d_raw
            .iter()
            .map(|&d| d.saturating_sub(trimmed_start))
            .collect();
        let cell_content_chars = display_width(&folded_cell);
        let width = balanced_col_widths
            .get(ci)
            .copied()
            .unwrap_or(cell_content_chars)
            .max(cell_content_chars);
        let left_padding = " ".repeat(cell_padding);
        let right_padding_len = width.saturating_sub(cell_content_chars) + cell_padding;
        let right_padding = " ".repeat(right_padding_len);
        let cell_anchor = cell_range.start.min(len.saturating_sub(1));

        display.push_str(&left_padding);
        for _ in 0..left_padding.len() {
            d2b.push(cell_anchor);
        }
        display_pos += left_padding.len();

        display.push_str(&folded_cell);
        for &local_buf in &cell_d2b {
            d2b.push((cell_range.start + local_buf).min(len.saturating_sub(1)));
        }
        for (local_buf, &local_display) in cell_b2d.iter().enumerate() {
            let global = cell_range.start + local_buf;
            if global < len {
                b2d[global] = display_pos + local_display;
            }
        }
        display_pos += folded_cell.len();

        display.push_str(&right_padding);
        let right_anchor = cell_range.end.min(len.saturating_sub(1));
        for _ in 0..right_padding.len() {
            d2b.push(right_anchor);
        }
        display_pos += right_padding.len();

        consumed = cell_range.end.min(len);
    }

    let trailing_start = if info.cells.is_empty() {
        consumed
    } else {
        consumed.saturating_add(1)
    };
    for buf_i in trailing_start..len {
        b2d[buf_i] = display_pos;
    }

    b2d.push(display.len());

    (display, d2b, b2d, false)
}

#[derive(Default)]
pub(crate) struct TableCellSyntaxFocus {
    pub selections: Vec<Range<usize>>,
    pub marked_ranges: Vec<Range<usize>>,
    pub search_reveal_range: Option<Range<usize>>,
    pub dragging: bool,
    pub suppress_empty_caret: bool,
}

impl EditorView {
    pub(super) fn table_cell_syntax_focus(&self) -> TableCellSyntaxFocus {
        let search_reveal_range = self.current_search_source_range();
        if !self.markdown_syntax_expansion_allowed() {
            return TableCellSyntaxFocus {
                search_reveal_range,
                ..Default::default()
            };
        }
        TableCellSyntaxFocus {
            // Drag activation follows the primary cursor, not source-sorted extras.
            selections: self
                .all_cursors_snapshot()
                .into_iter()
                .map(|cursor| cursor.selection_range())
                .collect(),
            marked_ranges: self.marked_ranges().cloned().collect(),
            search_reveal_range,
            dragging: self.is_selecting && self.selection_drag_has_moved,
            suppress_empty_caret: self.suppress_fold_for_empty_selection_drag,
        }
    }
}

/// Mirror ordinary text selection activation, projected into one source-owned
/// cell. During drag, only a selection inside one token reveals that token;
/// crossing unrelated content stays folded until the selection is committed.
pub(crate) fn table_cell_active_ranges(
    line_start_offset: usize,
    cell_range: &Range<usize>,
    focus: &TableCellSyntaxFocus,
    decoration: &LineDecoration,
) -> Vec<Range<usize>> {
    let start = line_start_offset + cell_range.start;
    let end = line_start_offset + cell_range.end;
    let project = |range: &Range<usize>| {
        range.start.saturating_sub(start).min(cell_range.len())
            ..range.end.saturating_sub(start).min(cell_range.len())
    };
    let mut active = Vec::new();
    if !focus.dragging {
        active.extend(
            focus
                .selections
                .iter()
                .filter(|range| !range.is_empty() && range.start < end && range.end > start)
                .map(project),
        );
    } else if let Some(selection) = focus.selections.first()
        && !selection.is_empty()
        && selection.start >= start
        && selection.end <= end
    {
        let local = project(selection);
        let trigger = decoration
            .syntax_ranges
            .iter()
            .map(|syntax| syntax.trigger_range.clone())
            .chain(
                decoration
                    .table_cell_fragments
                    .iter()
                    .filter(|fragment| fragment.prefix_len > 0)
                    .map(|fragment| {
                        fragment.range.start..fragment.range.start + fragment.prefix_len
                    }),
            )
            .find(|trigger| local.start >= trigger.start && local.end <= trigger.end);
        if let Some(trigger) = trigger {
            active.push(trigger);
        }
    }
    active.extend(
        focus
            .marked_ranges
            .iter()
            .filter(|range| range.start < end && range.end > start)
            .map(project),
    );
    if active.is_empty() && !focus.dragging && !focus.suppress_empty_caret {
        active.extend(
            focus
                .selections
                .iter()
                .filter(|range| range.is_empty() && range.start >= start && range.start <= end)
                .map(project),
        );
    }
    // Search is a presentation reason, independent of drag/caret suppression.
    if let Some(range) = focus.search_reveal_range.as_ref()
        && range.start.max(start) < range.end.min(end)
    {
        active.push(project(range));
    }
    active
}

#[cfg(test)]
pub(crate) fn active_table_cell_ranges(
    line_start_offset: usize,
    cell_range: &Range<usize>,
    cursor_offsets: &[usize],
    marked_range: Option<&Range<usize>>,
) -> Vec<Range<usize>> {
    table_cell_active_ranges(
        line_start_offset,
        cell_range,
        &TableCellSyntaxFocus {
            selections: cursor_offsets
                .iter()
                .map(|&offset| offset..offset)
                .collect(),
            marked_ranges: marked_range.into_iter().cloned().collect(),
            ..Default::default()
        },
        &LineDecoration::default(),
    )
}

/// Structural list syntax at the start of a cell, including incomplete markers
/// whose indentation must survive until completion. This is not display folding.
pub(crate) fn table_cell_list_syntax_len(source: &str) -> usize {
    let leading = list_marker_indent(source);
    let trimmed = &source[leading..];
    let bullet = if ["- ", "* ", "+ "]
        .iter()
        .any(|marker| trimmed.starts_with(marker))
    {
        2
    } else if matches!(trimmed, "-" | "*" | "+") {
        1
    } else {
        0
    };
    let syntax = bullet
        .max(task_core_syntax_len(trimmed))
        .max(markdown::ordered_list_prefix_len(trimmed));
    if syntax == 0 { 0 } else { leading + syntax }
}

pub(crate) fn table_cell_decoration(
    row_decoration: &markdown::LineDecoration,
    cell_content: &str,
    _cell_range: &Range<usize>,
) -> markdown::LineDecoration {
    let mut cell_decoration = markdown::parse_table_cell_decorations(cell_content, row_decoration)
        .into_iter()
        .next()
        .unwrap_or_default();
    let first_list_syntax = if row_decoration
        .table_info
        .as_ref()
        .is_some_and(|info| info.is_front_matter)
    {
        0
    } else {
        table_cell_list_syntax_len(cell_content)
    };
    let indented_list = cell_decoration.block_type == BlockType::CodeBlock && first_list_syntax > 0;
    if indented_list {
        // Source indentation is a list depth inside a cell, not an indented
        // code block. The paragraph context below restores its inline syntax.
        cell_decoration.block_type = BlockType::Normal;
        cell_decoration.block_preview = None;
        cell_decoration.code_language = None;
    }
    // Cell breaks are inline HTML, even at the start of a cell. Reuse the
    // paragraph parse for both protection and presentation, without borrowing
    // block-level syntax from the synthetic `x ` prefix.
    let break_context =
        markdown::parse_table_cell_decorations(&format!("x {cell_content}"), row_decoration)
            .into_iter()
            .next()
            .unwrap_or_default();
    if matches!(
        cell_decoration.block_type,
        BlockType::Normal
            | BlockType::UnorderedList
            | BlockType::OrderedList
            | BlockType::TaskList(_)
    ) && cell_decoration.block_preview.is_none()
        && cell_decoration.footnote_definition_label.is_none()
    {
        let shift = |range: &Range<usize>| {
            let start = range.start.checked_sub(2)?;
            let end = range.end.checked_sub(2)?;
            (start <= end && end <= cell_content.len()).then_some(start..end)
        };
        // Inline triggers include the preceding byte for caret expansion. At
        // cell start that byte belongs to `x `, so clip it rather than dropping
        // the cell's opening/closing syntax ranges.
        let shift_trigger = |range: &Range<usize>| shift(&(range.start.max(2)..range.end));
        let shift_ranges =
            |ranges: &[Range<usize>]| ranges.iter().filter_map(&shift).collect::<Vec<_>>();
        cell_decoration.inline_spans = break_context
            .inline_spans
            .iter()
            .filter_map(|span| {
                Some(markdown::InlineSpan {
                    range: shift(&span.range)?,
                    ..span.clone()
                })
            })
            .collect();
        cell_decoration.inline_math_spans = shift_ranges(&break_context.inline_math_spans);
        cell_decoration.protected_ranges = shift_ranges(&break_context.protected_ranges);
        cell_decoration.syntax_ranges = break_context
            .syntax_ranges
            .iter()
            .filter_map(|syntax| {
                Some(markdown::SyntaxRange {
                    range: shift(&syntax.range)?,
                    trigger_range: shift_trigger(&syntax.trigger_range)?,
                })
            })
            .collect();
        cell_decoration.display_replacements = break_context
            .display_replacements
            .iter()
            .filter_map(|replacement| {
                Some(markdown::DisplayReplacement {
                    source_range: shift(&replacement.source_range)?,
                    trigger_range: shift_trigger(&replacement.trigger_range)?,
                    ..replacement.clone()
                })
            })
            .collect();
        cell_decoration.highlight_spans = shift_ranges(&break_context.highlight_spans);
        cell_decoration.superscript_spans = shift_ranges(&break_context.superscript_spans);
        cell_decoration.subscript_spans = shift_ranges(&break_context.subscript_spans);
        cell_decoration.wiki_links = break_context
            .wiki_links
            .iter()
            .filter_map(|wiki| {
                Some(markdown::WikiLinkSpan {
                    full_range: shift(&wiki.full_range)?,
                    collapsed_range: shift(&wiki.collapsed_range)?,
                    ..wiki.clone()
                })
            })
            .collect();
        cell_decoration.footnote_references = break_context
            .footnote_references
            .iter()
            .filter_map(|footnote| {
                Some(markdown::FootnoteReferenceSpan {
                    full_range: shift(&footnote.full_range)?,
                    collapsed_range: shift(&footnote.collapsed_range)?,
                    ..footnote.clone()
                })
            })
            .collect();
        cell_decoration.image_path = break_context.image_path.clone();
    }

    let breaks = table_cell_break_ranges(cell_content)
        .into_iter()
        .filter(|source_range| {
            let inner = &cell_content[source_range.start + 1..source_range.end - 1];
            if !inner
                .get(..2)
                .is_some_and(|name| name.eq_ignore_ascii_case("br"))
                || !inner[2..]
                    .strip_suffix('/')
                    .unwrap_or(&inner[2..])
                    .bytes()
                    .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c'))
            {
                return false;
            }
            let target = source_range.start + 2..source_range.end + 2;
            let overlaps =
                |range: &Range<usize>| range.start < target.end && range.end > target.start;
            !break_context.protected_ranges.iter().any(overlaps)
                && !break_context.inline_spans.iter().any(|span| {
                    span.kind == markdown::InlineKind::InlineCode && overlaps(&span.range)
                })
                && !break_context
                    .wiki_links
                    .iter()
                    .any(|link| overlaps(&link.full_range))
                && !break_context.inline_math_spans.iter().any(overlaps)
                && !break_context.superscript_spans.iter().any(overlaps)
                && !break_context.subscript_spans.iter().any(overlaps)
                && !break_context
                    .footnote_references
                    .iter()
                    .any(|reference| overlaps(&reference.full_range))
                && !break_context.inline_spans.iter().any(|span| {
                    matches!(
                        span.kind,
                        markdown::InlineKind::Link | markdown::InlineKind::Image
                    ) && markdown::inline_span_source_range(&break_context, span).is_some_and(
                        |source| {
                            break_context.syntax_ranges.iter().any(|syntax| {
                                syntax.trigger_range == source && overlaps(&syntax.range)
                            })
                        },
                    )
                })
        })
        .collect::<Vec<_>>();
    for (start, end) in std::iter::once(0)
        .chain(breaks.iter().map(|range| range.end))
        .zip(
            breaks
                .iter()
                .map(|range| range.start)
                .chain([cell_content.len()]),
        )
    {
        let fragment = &cell_content[start..end];
        let leading = list_marker_indent(fragment);
        let trimmed = &fragment[leading..];
        let task_core = task_core_syntax_len(trimmed);
        let task_len = task_prefix_len(trimmed);
        let ordered_len = complete_ordered_list_prefix_len(trimmed);
        let (block_type, marker_len) = if task_core > 0 {
            if task_len > 0 {
                (
                    BlockType::TaskList(matches!(trimmed.as_bytes()[3], b'x' | b'X')),
                    task_len,
                )
            } else {
                (BlockType::Normal, 0)
            }
        } else if ordered_len > 0 {
            (BlockType::OrderedList, ordered_len)
        } else if ["- ", "* ", "+ "]
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            (BlockType::UnorderedList, 2)
        } else {
            (BlockType::Normal, 0)
        };
        // Structural prefix folding is owned by each fragment, not the parser's
        // first block (which otherwise extends across all source <br> tags).
        if start == 0 {
            let structural_end = leading
                + marker_len
                    .max(task_core)
                    .max(markdown::ordered_list_prefix_len(trimmed));
            cell_decoration.syntax_ranges.retain(|syntax| {
                !(syntax.range.start < structural_end && syntax.trigger_range.end <= structural_end)
            });
            if indented_list
                || matches!(
                    cell_decoration.block_type,
                    BlockType::TaskList(_) | BlockType::OrderedList | BlockType::UnorderedList
                )
            {
                cell_decoration.block_type = block_type.clone();
            }
        }
        cell_decoration
            .table_cell_fragments
            .push(markdown::TableCellFragment {
                range: start..end,
                block_type,
                prefix_len: if marker_len > 0 {
                    leading + marker_len
                } else {
                    0
                },
            });
    }

    for source_range in breaks {
        cell_decoration
            .display_replacements
            .push(markdown::DisplayReplacement {
                source_range: source_range.clone(),
                display_text: "\n".to_string(),
                trigger_range: source_range,
            });
    }

    cell_decoration
}

pub(crate) fn table_cell_break_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    while let Some(relative_start) = text[offset..].find('<') {
        let start = offset + relative_start;
        let Some(relative_end) = text[start..].find('>') else {
            break;
        };
        let end = start + relative_end + 1;
        let inner = text[start + 1..end - 1].trim();
        let tag = inner.strip_suffix('/').unwrap_or(inner).trim();
        if tag.eq_ignore_ascii_case("br") && !markdown::is_escaped_at(text, start) {
            ranges.push(start..end);
        }
        offset = end;
    }
    ranges
}

pub(crate) fn compute_table_render_widths(
    info: &TableLineInfo,
    available_chars: Option<usize>,
    table_cell_padding_chars: usize,
) -> Vec<usize> {
    let mut widths = info.column_widths.clone();
    let Some(available_chars) = available_chars else {
        return widths;
    };

    if widths.is_empty() {
        return widths;
    }

    let padding_budget = table_cell_padding_chars
        .saturating_mul(2)
        .saturating_mul(widths.len());
    let min_width = 3usize;
    let min_required = min_width
        .saturating_mul(widths.len())
        .saturating_add(padding_budget);
    if available_chars <= padding_budget {
        return vec![1; widths.len()];
    }
    if available_chars <= min_required {
        let content_budget = available_chars
            .saturating_sub(padding_budget)
            .max(widths.len());
        let base = (content_budget / widths.len()).max(1);
        let mut remainder = content_budget.saturating_sub(base * widths.len());
        for width in &mut widths {
            *width = base;
            if remainder > 0 {
                *width += 1;
                remainder -= 1;
            }
        }
        return widths;
    }

    let content_budget = available_chars.saturating_sub(padding_budget);
    let current_total: usize = widths.iter().sum();
    if current_total <= content_budget {
        return widths;
    }

    widths.fill(min_width);
    let mut remaining = content_budget.saturating_sub(min_width * widths.len());
    let desired_extras: Vec<usize> = info
        .column_widths
        .iter()
        .map(|width| width.saturating_sub(min_width))
        .collect();

    while remaining > 0 {
        let mut made_progress = false;
        for (width, desired_extra) in widths.iter_mut().zip(desired_extras.iter()) {
            if width.saturating_sub(min_width) < *desired_extra {
                *width += 1;
                remaining -= 1;
                made_progress = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if !made_progress {
            let mut index = 0usize;
            let width_count = widths.len();
            while remaining > 0 {
                widths[index % width_count] += 1;
                remaining -= 1;
                index += 1;
            }
        }
    }

    widths
}

/// Layout information for a table rendered as a block element.
#[derive(Debug, Clone)]
pub(crate) struct TableCellLayout {
    /// Pixel bounds of the cell (relative to table origin).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Column index.
    pub col: usize,
    pub alignment: crate::markdown::TableAlign,
    /// The display text for this cell (with inline markdown folded).
    pub display_text: String,
    pub prefix_collapsed: bool,
    /// Mapping from display offset → buffer offset within the cell.
    pub display_to_buffer: Vec<usize>,
    /// Mapping from buffer offset → display offset within the cell.
    pub buffer_to_display: Vec<usize>,
    /// The original cell byte range within its source line.
    pub cell_range: Range<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct TableLayout {
    /// Total pixel width of the table.
    pub total_width: f32,
    /// Total pixel height of the table (excluding handles).
    pub total_height: f32,
    /// Pixel heights for each visual row.
    pub row_heights: Vec<f32>,
    /// Cell layouts indexed as [visual_row][col].
    pub cells: Vec<Vec<TableCellLayout>>,
    /// Number of visual rows (header + data rows, separator excluded).
    pub visual_row_count: usize,
    /// Number of columns.
    pub col_count: usize,
    /// The handle area size in pixels.
    pub handle_size: f32,
    /// Cell padding in pixels (horizontal, each side).
    pub cell_padding_x: f32,
    /// Cell padding in pixels (vertical, each side).
    pub cell_padding_y: f32,
    /// Maps visual_row → source line index in the buffer.
    pub visual_row_to_line: Vec<usize>,
    /// The cell text color for styling.
    pub is_header: Vec<bool>,
    pub is_front_matter: bool,
}

/// Compute the full table layout from decoration data of all lines in the table block.
/// - `buffer`: raw source for restoring trailing cell whitespace during rendering
/// - `decorations`: all parsed line decorations
/// - `table_start`, `table_end`: line range of the table
/// - `available_width`: pixel width available for the table
/// - `cell_padding_x`, `cell_padding_y`: pixel padding per cell side
/// - `base_char_width`: width of a monospace character in pixels
/// - `line_height`: text line height in pixels
/// - `handle_size`: drag-handle area size in pixels
pub(crate) fn compute_table_layout(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    table_start: usize,
    table_end: usize,
    available_width: f32,
    cell_padding_x: f32,
    cell_padding_y: f32,
    base_char_width: f32,
    line_height: f32,
    handle_size: f32,
) -> TableLayout {
    // Collect non-separator rows with their line indices
    let mut visual_rows: Vec<(usize, &TableLineInfo)> = Vec::new();
    for line_idx in table_start..table_end {
        if let Some(ref ti) = decorations
            .get(line_idx)
            .and_then(|d| d.table_info.as_ref())
        {
            if !ti.is_separator {
                visual_rows.push((line_idx, ti));
            }
        }
    }

    let col_count = visual_rows
        .first()
        .map(|(_, ti)| ti.cells.len())
        .unwrap_or(1);
    let is_front_matter = visual_rows
        .first()
        .is_some_and(|(_, ti)| ti.is_front_matter);

    // Measure desired column widths based on content
    let mut desired_col_widths_chars = vec![if is_front_matter { 3usize } else { 1 }; col_count];
    for (line, ti) in &visual_rows {
        for ci in 0..ti.cells.len().min(col_count) {
            let (cell, cell_range) =
                super::table_cell_render::content_with_range(ti, buffer.line_content(*line), ci);
            let cell = cell.as_str();
            let width = if is_front_matter {
                display_width(cell.trim())
            } else {
                let decoration = table_cell_decoration(&decorations[*line], cell, &cell_range);
                let (display, _, b2d, _) = build_display_text(
                    cell,
                    &decoration,
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
                hard_line_byte_ranges(&display)
                    .iter()
                    .zip(&decoration.table_cell_fragments)
                    .map(|(range, fragment)| {
                        let end = super::table_cell_render::intrinsic_line_end(
                            cell, fragment, &b2d, range,
                        );
                        display_width(&display[range.start..range.start + end])
                    })
                    .max()
                    .unwrap_or(0)
            };
            desired_col_widths_chars[ci] = desired_col_widths_chars[ci].max(width);
        }
    }

    // Keep every table inside the editor viewport. On very narrow panes or
    // tables with many columns, shrink padding before compressing content;
    // wrapped cell text then increases row height instead of overflowing.
    let available_width = available_width.max(col_count as f32);
    let cell_padding_x = cell_padding_x.min(available_width / (col_count as f32 * 4.0));
    let total_padding = cell_padding_x * 2.0 * col_count as f32;
    let content_width = (available_width - total_padding).max(col_count as f32);
    let desired_total: f32 = desired_col_widths_chars
        .iter()
        .map(|&w| w as f32 * base_char_width)
        .sum();

    let col_widths: Vec<f32> = if !is_front_matter {
        let desired = desired_col_widths_chars
            .iter()
            .map(|&width| width as f32 * base_char_width)
            .collect::<Vec<_>>();
        super::table_column_widths::allocate(&desired, content_width, base_char_width.max(1.0))
    } else if desired_total <= content_width {
        let mut widths: Vec<f32> = desired_col_widths_chars
            .iter()
            .map(|&w| w as f32 * base_char_width)
            .collect();
        let extra = content_width - desired_total;
        if is_front_matter && widths.len() == 2 {
            widths[1] += extra;
        } else {
            let extra_per_col = extra / col_count as f32;
            for width in &mut widths {
                *width += extra_per_col;
            }
        }
        widths
    } else {
        let min_col = (content_width / col_count as f32).min(28.0).max(1.0);
        let mut widths: Vec<f32> = desired_col_widths_chars
            .iter()
            .map(|&w| (w as f32 * base_char_width).max(min_col))
            .collect();

        let mut unresolved: Vec<usize> = (0..col_count).collect();
        let mut remaining_width = content_width;
        loop {
            if unresolved.is_empty() {
                break;
            }

            let avg_width = (remaining_width / unresolved.len() as f32).max(min_col);
            let mut changed = false;
            let mut next_unresolved = Vec::new();

            for &index in &unresolved {
                if widths[index] <= avg_width {
                    remaining_width -= widths[index];
                    changed = true;
                } else {
                    next_unresolved.push(index);
                }
            }

            if !changed {
                for &index in &unresolved {
                    widths[index] = avg_width;
                }
                break;
            }

            unresolved = next_unresolved;
        }

        let actual_total: f32 = widths.iter().sum();
        if actual_total > content_width {
            let excess = actual_total - content_width;
            let shrinkable: Vec<usize> = widths
                .iter()
                .enumerate()
                .filter_map(|(index, width)| (*width > min_col + 0.1).then_some(index))
                .collect();
            let shrinkable_total: f32 = shrinkable
                .iter()
                .map(|&index| widths[index] - min_col)
                .sum();
            if shrinkable_total > 0.0 {
                for index in shrinkable {
                    let share = (widths[index] - min_col) / shrinkable_total;
                    widths[index] = (widths[index] - excess * share).max(min_col);
                }
            }
        }

        widths
    };

    let row_height = line_height + cell_padding_y * 2.0;
    let row_heights = vec![row_height; visual_rows.len()];
    let total_width = col_widths.iter().sum::<f32>() + total_padding;
    let total_height = row_height * visual_rows.len() as f32;

    // Build cell layouts
    let mut all_cells: Vec<Vec<TableCellLayout>> = Vec::new();
    let mut visual_row_to_line = Vec::new();
    let mut is_header_vec = Vec::new();

    let mut current_y = 0.0_f32;
    for (line_idx, ti) in visual_rows.iter() {
        visual_row_to_line.push(*line_idx);
        is_header_vec.push(ti.is_header);
        let mut row_cells = Vec::new();
        let mut current_x = 0.0_f32;
        for ci in 0..col_count {
            let (cell_content, cell_range) = super::table_cell_render::content_with_range(
                ti,
                buffer.line_content(*line_idx),
                ci,
            );
            let alignment = ti
                .column_alignments
                .get(ci)
                .copied()
                .unwrap_or(crate::markdown::TableAlign::Left);

            // Build display text for cell with inline markdown folding
            let cell_decoration =
                table_cell_decoration(&decorations[*line_idx], &cell_content, &cell_range);
            let (folded_raw, d2b_raw, b2d_raw, prefix_collapsed) = build_display_text(
                &cell_content,
                &cell_decoration,
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
            let trimmed_range =
                table_cell_display_range(&folded_raw, &cell_content, &cell_decoration, &b2d_raw);
            let trim_start = trimmed_range.start;
            let display_text = folded_raw[trimmed_range.clone()].to_string();
            let d2b = d2b_raw[trimmed_range].to_vec();
            let b2d = b2d_raw
                .iter()
                .map(|&d| d.saturating_sub(trim_start))
                .collect();

            let col_w = col_widths.get(ci).copied().unwrap_or(28.0);
            let cell_w = col_w + cell_padding_x * 2.0;
            row_cells.push(TableCellLayout {
                x: current_x,
                y: current_y,
                width: cell_w,
                height: row_height,
                col: ci,
                alignment,
                display_text,
                prefix_collapsed,
                display_to_buffer: d2b,
                buffer_to_display: b2d,
                cell_range,
            });
            current_x += cell_w;
        }
        all_cells.push(row_cells);
        current_y += row_height;
    }

    TableLayout {
        total_width,
        total_height,
        row_heights,
        cells: all_cells,
        visual_row_count: visual_rows.len(),
        col_count,
        handle_size,
        cell_padding_x,
        cell_padding_y,
        visual_row_to_line,
        is_header: is_header_vec,
        is_front_matter,
    }
}

pub(crate) fn build_highlight_bounds(
    display_text: &str,
    runs: &[TextRun],
    wrapped: &WrappedLine,
    disp_start: usize,
    disp_end: usize,
    content_x: f32,
    line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    leading_padding_px: f32,
    letter_spacing: f32,
    window: &mut Window,
) -> Vec<Bounds<Pixels>> {
    if disp_start >= disp_end {
        return Vec::new();
    }

    let mut bounds_list = Vec::new();
    let mut segment_start = 0usize;
    for (segment_index, segment_end) in wrap_boundary_byte_indices(wrapped, display_text)
        .into_iter()
        .chain(std::iter::once(display_text.len()))
        .enumerate()
    {
        let overlap_start = disp_start.max(segment_start);
        let overlap_end = disp_end.min(segment_end);
        if overlap_start < overlap_end {
            let start_x = if overlap_start == segment_start {
                0.0
            } else {
                f32::from(
                    wrapped
                        .position_for_index(overlap_start, px(line_height))
                        .unwrap_or_default()
                        .x,
                )
            };

            let end_x = if display_text[segment_start..segment_end].contains('\u{a0}') {
                f32::from(
                    wrapped.unwrapped_layout.x_for_index(overlap_end)
                        - wrapped.unwrapped_layout.x_for_index(segment_start),
                )
            } else if overlap_end == segment_end {
                let segment_text = &display_text[segment_start..segment_end];
                let segment_runs = extract_runs_for_range(runs, segment_start, segment_end);
                let shared: SharedString = if segment_text.is_empty() {
                    " ".into()
                } else {
                    segment_text.to_string().into()
                };
                if let Ok(mut shaped) = window.text_system().shape_text(
                    shared,
                    px(font_size),
                    &segment_runs,
                    None,
                    None,
                ) {
                    if letter_spacing != 0.0 {
                        for w in shaped.iter_mut() {
                            apply_letter_spacing(w, letter_spacing);
                        }
                    }
                    shaped.first().map_or(0.0, |w| f32::from(w.width()))
                } else {
                    0.0
                }
            } else {
                f32::from(
                    wrapped
                        .position_for_index(overlap_end, px(line_height))
                        .unwrap_or_default()
                        .x,
                )
            };

            let indent = if segment_index > 0 { indent_px } else { 0.0 };
            let segment_y = line_top + f32::from(wrapped.row_top(segment_index, px(line_height)));
            let segment_height = f32::from(wrapped.row_height(segment_index, px(line_height)));
            bounds_list.push(Bounds::from_corners(
                point(
                    px(if overlap_start == segment_start {
                        content_x + indent - leading_padding_px
                    } else {
                        content_x + indent + start_x
                    }),
                    px(segment_y),
                ),
                point(
                    px(content_x + indent + end_x),
                    px(segment_y + segment_height),
                ),
            ));
        }
        segment_start = segment_end;
    }

    bounds_list
}

pub(crate) fn build_selection_quads(
    display_text: &str,
    runs: &[TextRun],
    wrapped: &WrappedLine,
    disp_start: usize,
    disp_end: usize,
    content_x: f32,
    line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    leading_padding_px: f32,
    selection_color: Hsla,
    letter_spacing: f32,
    window: &mut Window,
) -> Vec<PaintQuad> {
    build_highlight_bounds(
        display_text,
        runs,
        wrapped,
        disp_start,
        disp_end,
        content_x,
        line_top,
        line_height,
        font_size,
        indent_px,
        leading_padding_px,
        letter_spacing,
        window,
    )
    .into_iter()
    .map(|bounds| fill(bounds, selection_color))
    .collect()
}

pub(crate) fn compute_image_preview_size(
    image_width: f32,
    image_height: f32,
    max_width: f32,
    raster_scale: f32,
) -> (f32, f32) {
    if image_width <= 0.0 || image_height <= 0.0 || max_width <= 0.0 {
        return (0.0, 0.0);
    }

    let logical_width = image_width / raster_scale.max(1.0);
    let logical_height = image_height / raster_scale.max(1.0);
    let scale = (max_width / logical_width).min(1.0);
    (logical_width * scale, logical_height * scale)
}

#[cfg(test)]
pub(crate) fn compute_generated_preview_size(
    image_width: f32,
    image_height: f32,
    max_width: f32,
) -> (f32, f32) {
    if image_width <= 0.0 || image_height <= 0.0 || max_width <= 0.0 {
        return (0.0, 0.0);
    }

    let logical_width = image_width / GENERATED_PREVIEW_RASTER_SCALE;
    let logical_height = image_height / GENERATED_PREVIEW_RASTER_SCALE;
    let scale = (max_width / logical_width).min(1.0);
    (logical_width * scale, logical_height * scale)
}

pub(crate) fn intrinsic_generated_preview_size_for_kind(
    image_width: f32,
    image_height: f32,
    kind: BlockPreviewKind,
) -> (f32, f32) {
    if image_width <= 0.0 || image_height <= 0.0 {
        return (0.0, 0.0);
    }

    let logical_width = image_width / generated_preview_raster_scale(kind);
    let logical_height = image_height / generated_preview_raster_scale(kind);
    (logical_width, logical_height)
}

/// Computes the on-screen size of a generated preview and the background that
/// hugs it (content size + padding on every side, no letterbox). Returns
/// `(fitted_w, fitted_h, background_width, background_height)` in the same
/// (scaled) pixel space. Used by the layout (extra-height), the background, and
/// the draw paths so they always agree.
///
/// Both kinds retain their rendered logical size unless the available width is narrower.
/// Height and the snug background follow the same fitted aspect ratio.
pub(crate) fn fit_generated_preview_size(
    image_width: f32,
    image_height: f32,
    kind: BlockPreviewKind,
    avail_width: f32,
    min_height: f32,
    _max_height: f32,
    pad_x: f32,
    pad_y: f32,
) -> (f32, f32, f32, f32) {
    let (w0, h0) = intrinsic_generated_preview_size_for_kind(image_width, image_height, kind);
    if w0 <= 0.0 || h0 <= 0.0 {
        return (0.0, 0.0, (2.0 * pad_x).max(1.0), min_height.max(1.0));
    }
    let inner_w = (avail_width - 2.0 * pad_x).max(1.0);
    let scale = (inner_w / w0).min(1.0).max(0.0);
    let (fitted_w, fitted_h) = (w0 * scale, h0 * scale);
    let background_width = (fitted_w + 2.0 * pad_x).min(avail_width).max(1.0);
    let background_height = (fitted_h + 2.0 * pad_y).max(1.0);
    (fitted_w, fitted_h, background_width, background_height)
}

pub(crate) fn image_syntax_range(line_content: &str) -> Option<Range<usize>> {
    let start = line_content.find("![")?;
    let suffix = &line_content[start..];
    let end = suffix.find(')')? + start + 1;
    Some(start..end)
}

pub(crate) fn is_image_syntax_active(
    decoration: &LineDecoration,
    line_content: &str,
    line_start_offset: usize,
    cursor_offset: usize,
    selection: &Range<usize>,
) -> bool {
    if decoration.image_path.is_none() {
        return false;
    }

    let Some(local_range) = image_syntax_range(line_content) else {
        return false;
    };
    let absolute_range =
        (line_start_offset + local_range.start)..(line_start_offset + local_range.end);

    (cursor_offset >= absolute_range.start && cursor_offset <= absolute_range.end)
        || (selection.start < absolute_range.end && selection.end > absolute_range.start)
}

/// Extract TextRuns covering the byte range [seg_start..seg_end) from the original runs.
pub(crate) fn extract_runs_for_range(
    runs: &[TextRun],
    seg_start: usize,
    seg_end: usize,
) -> Vec<TextRun> {
    let mut result = Vec::new();
    let mut pos = 0;
    for run in runs {
        let run_end = pos + run.len;
        let overlap_start = seg_start.max(pos);
        let overlap_end = seg_end.min(run_end);
        if overlap_start < overlap_end {
            result.push(TextRun {
                len: overlap_end - overlap_start,
                ..run.clone()
            });
        }
        pos = run_end;
    }
    result
}

pub(crate) fn hard_line_byte_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            ranges.push(start..index);
            start = index + 1;
        }
    }
    ranges.push(start..text.len());
    ranges
}

pub(crate) fn table_cell_display_range(
    display_text: &str,
    source: &str,
    decoration: &LineDecoration,
    buffer_to_display: &[usize],
) -> Range<usize> {
    // Only source padding is disposable. Spaces produced by scripts, footnotes
    // and folded prefixes reserve real ink and must survive at cell edges.
    let source_start = if decoration
        .table_cell_fragments
        .first()
        .is_some_and(|fragment| fragment.prefix_len > 0)
    {
        0
    } else {
        source.len() - source.trim_start_matches([' ', '\t']).len()
    };
    let prefix_end = decoration
        .table_cell_fragments
        .last()
        .map_or(0, |fragment| fragment.range.start + fragment.prefix_len);
    let source_end = source
        .trim_end_matches([' ', '\t'])
        .len()
        .max(source_start)
        .max(prefix_end);
    let start = buffer_to_display
        .get(source_start)
        .copied()
        .unwrap_or(0)
        .min(display_text.len());
    let end = buffer_to_display
        .get(source_end)
        .copied()
        .unwrap_or(display_text.len())
        .clamp(start, display_text.len());
    start..end
}

pub(crate) fn table_cell_trimmed_display_range(display_text: &str) -> Range<usize> {
    let trimmed_start = display_text.trim_start_matches([' ', '\t']);
    let start = display_text.len() - trimmed_start.len();
    let trimmed = trimmed_start.trim_end_matches([' ', '\t']);
    start..start + trimmed.len()
}

/// Get wrap boundary byte indices from the WrappedLine's shaped runs.
pub(crate) fn wrap_boundary_byte_indices(wrapped: &WrappedLine, text: &str) -> Vec<usize> {
    let runs = wrapped.runs();
    let mut previous = 0usize;
    wrapped
        .wrap_boundaries()
        .iter()
        .filter_map(|boundary| {
            let index = runs
                .get(boundary.run_ix)?
                .glyphs
                .get(boundary.glyph_ix)?
                .index;
            if index <= previous || index > text.len() || !text.is_char_boundary(index) {
                return None;
            }
            previous = index;
            Some(index)
        })
        .collect()
}

pub(crate) fn wrap_segment_byte_ranges(text_len: usize, boundaries: &[usize]) -> Vec<Range<usize>> {
    let mut ranges = Vec::with_capacity(boundaries.len() + 1);
    let mut start = 0usize;
    for end in boundaries.iter().copied() {
        let end = end.min(text_len);
        if end > start {
            ranges.push(start..end);
            start = end;
        }
    }
    if start < text_len || ranges.is_empty() {
        ranges.push(start..text_len);
    }
    ranges
}

/// Return indent_px if the position y indicates a continuation line (y > 0).
pub(crate) fn indent_for_y(indent_px: f32, y: Pixels, line_height: f32) -> f32 {
    if indent_px > 0.0 && f32::from(y) > line_height * 0.5 {
        indent_px
    } else {
        0.0
    }
}

/// Compute the display prefix byte count for list indent.
pub(crate) fn compute_display_prefix_bytes(
    display_text: &str,
    block_type: &BlockType,
    prefix_collapsed: bool,
    _collapsed_task_prefix_chars: usize,
) -> usize {
    fn char_boundary_after_n_chars(text: &str, char_count: usize) -> usize {
        text.char_indices()
            .nth(char_count)
            .map(|(offset, _)| offset)
            .unwrap_or(text.len())
    }

    let leading_ws = display_text.len() - display_text.trim_start_matches([' ', '\t']).len();
    let trimmed = &display_text[leading_ws..];
    match block_type {
        BlockType::UnorderedList => {
            if prefix_collapsed {
                leading_ws
            } else if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                leading_ws + 2
            } else {
                0
            }
        }
        BlockType::TaskList(_) => {
            if prefix_collapsed {
                leading_ws
            } else {
                trimmed.find("] ").map(|p| leading_ws + p + 2).unwrap_or(0)
            }
        }
        BlockType::OrderedList => {
            // Handles both `1. ` and `1) ` marker styles (plus trailing
            // whitespace) so the marker highlight never bleeds into content.
            let prefix_len = markdown::ordered_list_prefix_len(trimmed);
            if prefix_len > 0 {
                leading_ws + prefix_len
            } else if prefix_collapsed {
                // Without a real marker only a digit run can have been folded;
                // anything else (e.g. `- 1.`) must keep its text visible.
                if trimmed.starts_with(|ch: char| ch.is_ascii_digit()) {
                    leading_ws + char_boundary_after_n_chars(trimmed, 4)
                } else {
                    leading_ws
                }
            } else {
                0
            }
        }
        _ => 0,
    }
}

pub(crate) fn prefix_is_folded(
    leading_ws: usize,
    prefix_len: usize,
    active_ranges: &[Range<usize>],
) -> bool {
    active_ranges.is_empty()
        || !active_ranges_intersect(active_ranges, &(leading_ws..leading_ws + prefix_len))
}

fn complete_ordered_list_prefix_len(trimmed: &str) -> usize {
    let prefix_len = markdown::ordered_list_prefix_len(trimmed);
    if prefix_len > 0 && matches!(trimmed.as_bytes()[prefix_len - 1], b' ' | b'\t') {
        prefix_len
    } else {
        0
    }
}

pub(crate) fn table_cell_fragment_prefix_is_folded(
    content: &str,
    fragment: &markdown::TableCellFragment,
    active_ranges: &[Range<usize>],
) -> bool {
    if fragment.prefix_len == 0 {
        return false;
    }
    let source = &content[fragment.range.clone()];
    let leading = list_marker_indent(source);
    if matches!(fragment.block_type, BlockType::TaskList(_)) {
        let local_active = active_ranges
            .iter()
            .filter_map(|range| {
                (range.end >= fragment.range.start && range.start <= fragment.range.end).then(
                    || {
                        range
                            .start
                            .saturating_sub(fragment.range.start)
                            .min(source.len())
                            ..range
                                .end
                                .saturating_sub(fragment.range.start)
                                .min(source.len())
                    },
                )
            })
            .collect::<Vec<_>>();
        task_prefix_is_folded(
            source,
            leading,
            task_core_syntax_len(&source[leading..]),
            &local_active,
        )
    } else {
        prefix_is_folded(
            fragment.range.start + leading,
            fragment.prefix_len - leading,
            active_ranges,
        )
    }
}

fn task_core_syntax_len(trimmed: &str) -> usize {
    for marker in [
        "- [ ]", "- [x]", "- [X]", "* [ ]", "* [x]", "* [X]", "+ [ ]", "+ [x]", "+ [X]",
    ] {
        if trimmed.starts_with(marker) {
            return marker.len();
        }
    }
    0
}

fn task_prefix_is_folded(
    line_content: &str,
    leading_ws: usize,
    task_core_len: usize,
    active_ranges: &[Range<usize>],
) -> bool {
    if task_prefix_len(&line_content[leading_ws..]) == 0 {
        return false;
    }
    if active_ranges.is_empty() {
        return true;
    }

    active_ranges.iter().all(|active_range| {
        if active_range.start != active_range.end {
            return !active_range_intersects_target(
                active_range,
                &(leading_ws..leading_ws + task_core_len),
            );
        }

        let col = active_range.start;
        let core_end = leading_ws + task_core_len;
        if col < leading_ws {
            return true;
        }
        if col < core_end {
            return false;
        }
        if col == core_end {
            return line_content
                .as_bytes()
                .get(core_end)
                .is_some_and(|byte| *byte == b' ');
        }
        true
    })
}

pub(crate) fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn push_syntax_candidate(candidates: &mut Vec<String>, candidate: impl Into<String>) {
    let candidate = candidate.into();
    if candidate.is_empty() || candidates.iter().any(|existing| existing == &candidate) {
        return;
    }
    candidates.push(candidate);
}

fn syntax_lookup_candidates(language: &str) -> Vec<String> {
    let token = language
        .trim()
        .trim_matches('`')
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, ',' | '{' | '}' | '[' | ']' | '(' | ')')
        })
        .next()
        .unwrap_or_default()
        .trim_start_matches('.');
    let lowered = token.to_ascii_lowercase();
    let mut candidates = Vec::new();
    push_syntax_candidate(&mut candidates, token);
    push_syntax_candidate(&mut candidates, lowered.clone());
    push_syntax_candidate(&mut candidates, token.to_ascii_uppercase());

    match lowered.as_str() {
        "bash" | "sh" | "shell" | "zsh" => {
            push_syntax_candidate(&mut candidates, "bash");
            push_syntax_candidate(&mut candidates, "shell");
            push_syntax_candidate(&mut candidates, "Shell-Unix-Generic");
        }
        "c" => {
            push_syntax_candidate(&mut candidates, "C");
        }
        "cpp" | "cxx" | "cc" | "c++" => {
            push_syntax_candidate(&mut candidates, "cpp");
            push_syntax_candidate(&mut candidates, "C++");
        }
        "cs" | "csharp" | "c#" => {
            push_syntax_candidate(&mut candidates, "cs");
            push_syntax_candidate(&mut candidates, "C#");
        }
        "dockerfile" => {
            push_syntax_candidate(&mut candidates, "Dockerfile");
        }
        "html" | "htm" => {
            push_syntax_candidate(&mut candidates, "html");
            push_syntax_candidate(&mut candidates, "HTML");
        }
        "ini" | "cfg" | "conf" => {
            push_syntax_candidate(&mut candidates, "ini");
            push_syntax_candidate(&mut candidates, "INI");
        }
        "java" => {
            push_syntax_candidate(&mut candidates, "Java");
        }
        "js" | "javascript" => {
            push_syntax_candidate(&mut candidates, "javascript");
            push_syntax_candidate(&mut candidates, "JavaScript");
        }
        "json" => {
            push_syntax_candidate(&mut candidates, "JSON");
        }
        "kt" | "kts" | "kotlin" => {
            push_syntax_candidate(&mut candidates, "kotlin");
            push_syntax_candidate(&mut candidates, "Kotlin");
        }
        "lua" => {
            push_syntax_candidate(&mut candidates, "Lua");
        }
        "md" | "markdown" | "mdx" => {
            push_syntax_candidate(&mut candidates, "markdown");
            push_syntax_candidate(&mut candidates, "Markdown");
        }
        "mermaid" | "mmd" => {
            push_syntax_candidate(&mut candidates, "mermaid");
            push_syntax_candidate(&mut candidates, "Mermaid");
        }
        "objc" | "objective-c" => {
            push_syntax_candidate(&mut candidates, "objective-c");
            push_syntax_candidate(&mut candidates, "Objective-C");
        }
        "php" => {
            push_syntax_candidate(&mut candidates, "PHP");
        }
        "py" | "python" => {
            push_syntax_candidate(&mut candidates, "python");
            push_syntax_candidate(&mut candidates, "Python");
        }
        "rb" | "ruby" => {
            push_syntax_candidate(&mut candidates, "ruby");
            push_syntax_candidate(&mut candidates, "Ruby");
        }
        "rs" | "rust" => {
            push_syntax_candidate(&mut candidates, "rust");
            push_syntax_candidate(&mut candidates, "Rust");
        }
        "scss" => {
            push_syntax_candidate(&mut candidates, "SCSS");
        }
        "sql" => {
            push_syntax_candidate(&mut candidates, "SQL");
        }
        "swift" => {
            push_syntax_candidate(&mut candidates, "Swift");
        }
        "toml" => {
            push_syntax_candidate(&mut candidates, "TOML");
        }
        "ts" | "typescript" => {
            push_syntax_candidate(&mut candidates, "typescript");
            push_syntax_candidate(&mut candidates, "TypeScript");
        }
        "tsx" => {
            push_syntax_candidate(&mut candidates, "tsx");
            push_syntax_candidate(&mut candidates, "TypeScriptReact");
            push_syntax_candidate(&mut candidates, "TypeScript React");
        }
        "vue" => {
            push_syntax_candidate(&mut candidates, "Vue");
        }
        "xml" => {
            push_syntax_candidate(&mut candidates, "XML");
        }
        "yaml" | "yml" => {
            push_syntax_candidate(&mut candidates, "yaml");
            push_syntax_candidate(&mut candidates, "YAML");
        }
        _ => {}
    }

    candidates
}

/// Convert a packed `0xRRGGBB` palette color to `Hsla`.
fn code_palette_hsla(rgb: u32) -> Hsla {
    syntect_color_to_hsla(code_palette_syntect_color(rgb))
}

fn apply_highlight_range(
    range: Range<usize>,
    color: Hsla,
    weight: FontWeight,
    style: FontStyle,
    colors: &mut [Hsla],
    weights: &mut [FontWeight],
    styles: &mut [FontStyle],
) {
    let start = range.start.min(colors.len());
    let end = range.end.min(colors.len());
    for index in start..end {
        colors[index] = color;
        weights[index] = weight;
        styles[index] = style;
    }
}

fn is_mermaid_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

/// Arrow runs are recognized before identifiers, including adjacent endpoints.
/// A single hyphen/dot still belongs to an ID such as `my-node.v2`.
fn mermaid_link_len(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut index = 0;
    while bytes.get(index) == Some(&b'<') {
        index += 1;
    }
    if index == 0
        && ["o--o", "o--x", "x--o", "x--x", "o---", "x---"]
            .iter()
            .any(|prefix| source.starts_with(prefix))
    {
        index += 1;
    }
    let body = &source[index..];
    if !["--", "-.", "->", "-x", "-)", "==", "~~~"]
        .iter()
        .any(|prefix| body.starts_with(prefix))
    {
        return 0;
    }
    while matches!(bytes.get(index), Some(b'-' | b'.' | b'=' | b'~')) {
        index += 1;
    }
    if matches!(bytes.get(index), Some(b'>' | b'<')) {
        while matches!(bytes.get(index), Some(b'>' | b'<')) {
            index += 1;
        }
    } else if matches!(bytes.get(index), Some(b'o' | b'x' | b')')) {
        index += 1;
    }
    while matches!(bytes.get(index), Some(b'+' | b'-')) {
        index += 1;
    }
    index
}

/// Quoted labels may contain escaped quotes and syntax-looking Unicode text.
fn mermaid_quoted_end(source: &str, start: usize) -> usize {
    let quote = source.as_bytes()[start];
    let mut chars = source[start + 1..].char_indices();
    while let Some((offset, ch)) = chars.next() {
        if ch == '\\' {
            chars.next();
        } else if ch as u32 == quote as u32 {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    source.len()
}

fn is_mermaid_language(language: &str) -> bool {
    let token = language
        .trim()
        .trim_matches('`')
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, ',' | '{' | '}' | '[' | ']' | '(' | ')')
        })
        .next()
        .unwrap_or_default()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    matches!(token.as_str(), "mermaid" | "mmd")
}

fn apply_mermaid_block_highlighting(
    line_content: &str,
    theme: &Theme,
    colors: &mut [Hsla],
    weights: &mut [FontWeight],
    styles: &mut [FontStyle],
) {
    if line_content.is_empty() {
        return;
    }

    let palette = theme.code_palette;
    let comment_color = code_palette_hsla(palette.comment);
    let keyword_color = code_palette_hsla(palette.keyword);
    let direction_color = code_palette_hsla(palette.number);
    let string_color = code_palette_hsla(palette.string);
    let operator_color = code_palette_hsla(palette.operator);
    let number_color = code_palette_hsla(palette.number);
    let identifier_color = code_palette_hsla(palette.variable);

    let trimmed = line_content.trim_start();
    if trimmed.starts_with("%%") && !trimmed.starts_with("%%{") {
        apply_highlight_range(
            0..line_content.len(),
            comment_color,
            FontWeight::NORMAL,
            FontStyle::Italic,
            colors,
            weights,
            styles,
        );
        return;
    }

    // Rows are independent so viewport/lazy/incremental shaping never depends
    // on which neighbouring rows happened to be painted first.
    let first_word = trimmed.split_whitespace().next().unwrap_or_default();
    let colon_label = matches!(
        first_word.to_ascii_lowercase().as_str(),
        "note" | "title" | "acctitle" | "accdescr"
    );
    let participant_alias = trimmed.to_ascii_lowercase().contains("participant ")
        || trimmed.to_ascii_lowercase().starts_with("actor ");
    let mut saw_link = false;
    let mut index = 0usize;
    while index < line_content.len() {
        let mut chars = line_content[index..].char_indices();
        let Some((_, ch)) = chars.next() else {
            break;
        };
        let ch_len = ch.len_utf8();

        if ch.is_ascii_whitespace() {
            index += ch_len;
            continue;
        }

        if line_content[index..].starts_with("%%") {
            apply_highlight_range(
                index..line_content.len(),
                comment_color,
                FontWeight::NORMAL,
                FontStyle::Italic,
                colors,
                weights,
                styles,
            );
            break;
        }

        if matches!(ch, '"' | '\'' | '`') {
            let end = mermaid_quoted_end(line_content, index);
            apply_highlight_range(
                index..end,
                string_color,
                FontWeight::NORMAL,
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            index = end;
            continue;
        }

        // Node shapes and edge labels protect their contents from keyword,
        // number, arrow and comment recognition (e.g. A["end --> 中"]).
        if matches!(ch, '[' | '{' | '(' | '|') {
            let start = index;
            let closing = match ch {
                '[' => ']',
                '{' => '}',
                '(' => ')',
                _ => '|',
            };
            let slanted =
                ch == '[' && matches!(line_content.as_bytes().get(index + 1), Some(b'/' | b'\\'));
            let opening_len = ch_len + usize::from(slanted);
            let mut stack = vec![(closing, slanted)];
            let mut delimiters = vec![index..index + opening_len];
            index += opening_len;
            while index < line_content.len() {
                let inner = line_content[index..].chars().next().unwrap();
                // In [/...\\] and [\\...\\], the backslash is part of the
                // shape closer, not an escape of the closing bracket.
                if stack.last() == Some(&(']', true))
                    && matches!(inner, '/' | '\\')
                    && line_content.as_bytes().get(index + 1) == Some(&b']')
                {
                    delimiters.push(index..index + 2);
                    stack.pop();
                    index += 2;
                    if stack.is_empty() {
                        break;
                    }
                    continue;
                }
                if matches!(inner, '"' | '`')
                    || inner == '\''
                        && !line_content[..index].ends_with(|ch: char| ch.is_alphanumeric())
                {
                    index = mermaid_quoted_end(line_content, index);
                    continue;
                }
                if inner == '\\' {
                    index += 1;
                    if let Some(escaped) = line_content[index..].chars().next() {
                        index += escaped.len_utf8();
                    }
                    continue;
                }
                let mut inner_len = inner.len_utf8();
                if stack.last().is_some_and(|(closing, _)| inner == *closing) {
                    delimiters.push(index..index + inner_len);
                    stack.pop();
                } else if ch != '|' && matches!(inner, '[' | '{' | '(') {
                    let slanted = inner == '['
                        && matches!(line_content.as_bytes().get(index + 1), Some(b'/' | b'\\'));
                    stack.push((
                        match inner {
                            '[' => ']',
                            '{' => '}',
                            _ => ')',
                        },
                        slanted,
                    ));
                    inner_len += usize::from(slanted);
                    delimiters.push(index..index + inner_len);
                }
                index += inner_len;
                if stack.is_empty() {
                    break;
                }
            }
            apply_highlight_range(
                start..index,
                string_color,
                FontWeight::NORMAL,
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            for delimiter in delimiters {
                apply_highlight_range(
                    delimiter,
                    operator_color,
                    FontWeight::NORMAL,
                    FontStyle::Normal,
                    colors,
                    weights,
                    styles,
                );
            }
            continue;
        }

        let link_len = mermaid_link_len(&line_content[index..]);
        if link_len > 0 {
            let start = index;
            index += link_len;
            saw_link = true;
            apply_highlight_range(
                start..index,
                operator_color,
                FontWeight::NORMAL,
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            // Flowchart's alternative `-- text -->` / `-. text .->` /
            // `== text ==>` label spellings have no brackets or pipe pair.
            if matches!(&line_content[start..index], "--" | "-." | "==") {
                let label_start = index;
                let label_end = line_content[index..]
                    .char_indices()
                    .find_map(|(offset, _)| {
                        let rest = &line_content[index + offset..];
                        (mermaid_link_len(rest) > 0 || rest.starts_with(".->"))
                            .then_some(index + offset)
                    });
                if let Some(end) = label_end {
                    apply_highlight_range(
                        label_start..end,
                        string_color,
                        FontWeight::NORMAL,
                        FontStyle::Normal,
                        colors,
                        weights,
                        styles,
                    );
                    index = end;
                }
            }
            continue;
        }

        if ch == ':'
            && (saw_link || colon_label)
            && !line_content[index..].starts_with("::")
            && !line_content[..index].ends_with(':')
        {
            apply_highlight_range(
                index..index + 1,
                operator_color,
                FontWeight::NORMAL,
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            apply_highlight_range(
                index + 1..line_content.len(),
                string_color,
                FontWeight::NORMAL,
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            break;
        }

        if is_mermaid_word_char(ch) && !matches!(ch, '-' | '.') {
            let start = index;
            index += ch_len;
            while index < line_content.len() {
                let mut inner_chars = line_content[index..].char_indices();
                let Some((_, next_ch)) = inner_chars.next() else {
                    break;
                };
                if is_mermaid_word_char(next_ch) && mermaid_link_len(&line_content[index..]) == 0 {
                    index += next_ch.len_utf8();
                } else {
                    break;
                }
            }

            let token = &line_content[start..index];
            let lowered = token.to_ascii_lowercase();
            let is_keyword = matches!(
                lowered.as_str(),
                "graph"
                    | "flowchart"
                    | "sequencediagram"
                    | "classdiagram"
                    | "statediagram"
                    | "statediagram-v2"
                    | "erdiagram"
                    | "journey"
                    | "gantt"
                    | "mindmap"
                    | "timeline"
                    | "pie"
                    | "requirementdiagram"
                    | "gitgraph"
                    | "c4context"
                    | "c4container"
                    | "c4component"
                    | "c4dynamic"
                    | "c4deployment"
                    | "xychart"
                    | "xychart-beta"
                    | "block-beta"
                    | "packet-beta"
                    | "architecture-beta"
                    | "quadrantchart"
                    | "sankey-beta"
                    | "subgraph"
                    | "end"
                    | "direction"
                    | "participant"
                    | "actor"
                    | "note"
                    | "autonumber"
                    | "loop"
                    | "alt"
                    | "else"
                    | "opt"
                    | "par"
                    | "and"
                    | "break"
                    | "critical"
                    | "option"
                    | "rect"
                    | "destroy"
                    | "activate"
                    | "deactivate"
                    | "create"
                    | "box"
                    | "title"
                    | "section"
                    | "style"
                    | "linkstyle"
                    | "click"
                    | "classdef"
                    | "class"
                    | "namespace"
                    | "state"
                    | "choice"
                    | "fork"
                    | "join"
                    | "as"
                    | "right"
                    | "left"
                    | "of"
                    | "over"
                    | "accdescr"
                    | "acctitle"
            );
            let is_direction = matches!(lowered.as_str(), "tb" | "td" | "bt" | "lr" | "rl");
            let is_number = token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.');
            apply_highlight_range(
                start..index,
                if is_direction {
                    direction_color
                } else if is_keyword {
                    keyword_color
                } else if is_number {
                    number_color
                } else {
                    identifier_color
                },
                if is_keyword || is_direction {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            if participant_alias && lowered == "as"
                || start == line_content.len() - trimmed.len()
                    && matches!(
                        lowered.as_str(),
                        "loop" | "alt" | "else" | "opt" | "section" | "title"
                    )
            {
                apply_highlight_range(
                    index..line_content.len(),
                    string_color,
                    FontWeight::NORMAL,
                    FontStyle::Normal,
                    colors,
                    weights,
                    styles,
                );
                break;
            }
            continue;
        }

        if matches!(
            ch,
            '[' | ']' | '{' | '}' | '(' | ')' | '|' | ':' | ';' | ',' | '&'
        ) {
            apply_highlight_range(
                index..(index + ch_len),
                operator_color,
                FontWeight::NORMAL,
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            index += ch_len;
            continue;
        }

        if matches!(ch, '-' | '=' | '>' | '<' | '.' | '+' | '~' | '/') {
            let start = index;
            index += ch_len;
            while index < line_content.len() {
                let mut inner_chars = line_content[index..].char_indices();
                let Some((_, next_ch)) = inner_chars.next() else {
                    break;
                };
                if matches!(next_ch, '-' | '=' | '>' | '<' | '.' | '+' | '~' | '/') {
                    index += next_ch.len_utf8();
                } else {
                    break;
                }
            }
            apply_highlight_range(
                start..index,
                operator_color,
                FontWeight::NORMAL,
                FontStyle::Normal,
                colors,
                weights,
                styles,
            );
            continue;
        }

        index += ch_len;
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ops::Range, sync::Arc};

    use gpui::{
        Bounds, FontId, FontStyle, FontWeight, GlyphId, LineLayout, RenderImage, ShapedGlyph,
        ShapedRun, TextRun, WrapBoundary, WrappedLine, WrappedLineLayout, font, point, px, size,
    };

    use crate::buffer::Buffer;
    use crate::editor::MermaidTheme;
    use crate::front_matter::FrontMatterValue;
    use crate::markdown::{self, LineDecoration};
    use crate::theme::{EditorThemeMode, Theme, ThemeKind, apply_editor_theme_mode};

    use super::{
        AnchoredPopupVertical, GeneratedPreviewFontFamilies, anchored_popup_placement,
        apply_front_matter_group_display, apply_search_match_text_styles, build_display_text,
        build_generated_block_preview_svg, build_table_display_text, build_text_runs_for_display,
        compute_display_prefix_bytes, compute_generated_preview_size, compute_table_layout,
        display_width, fit_generated_preview_size, front_matter_group_plan,
        front_matter_item_source_map, load_generated_preview_image_from_svg,
        load_generated_preview_image_from_svg_for_editor, prefix_is_folded,
        table_cell_content_offset, table_cell_decoration, task_prefix_is_folded,
        wrap_segment_byte_ranges,
    };

    mod search_source_reveal_tests {
        use super::super as helpers;
        use super::*;

        type Display = (String, Vec<usize>, Vec<usize>, Vec<TextRun>);

        fn display(
            source: &str,
            decoration: &LineDecoration,
            active: &[Range<usize>],
            cell: bool,
        ) -> Display {
            let (raw, d2b, b2d, collapsed) = build_display_text(
                source, decoration, active, false, None, 0, 3, 3, 2, 0, 0.7, 0.7, 0.7,
            );
            let range = if cell {
                helpers::table_cell_display_range(&raw, source, decoration, &b2d)
            } else {
                0..raw.len()
            };
            let text = raw[range.clone()].to_owned();
            let d2b = d2b[range.clone()].to_vec();
            let b2d = b2d
                .iter()
                .map(|index| index.saturating_sub(range.start).min(range.len()))
                .collect();
            let theme = Theme::one_dark();
            let body = font("Helvetica");
            let mono = font("Menlo");
            let runs = build_text_runs_for_display(
                &text,
                source,
                &d2b,
                decoration,
                active,
                false,
                collapsed,
                &body,
                &body,
                &mono,
                &body,
                &body,
                &body,
                &body,
                3,
                16.0,
                &theme,
                theme.git_status_added,
                theme.text_danger,
                theme.link,
                FontWeight::NORMAL,
                theme.list_marker,
                FontWeight::BOLD,
                theme.syntax_marker,
                None,
                false,
            );
            (text, d2b, b2d, runs)
        }

        fn assert_keyword_mapping_and_highlight(
            source: &str,
            mapped: &mut Display,
            base: usize,
            keyword: Range<usize>,
        ) {
            let (text, d2b, b2d, runs) = mapped;
            let visible = b2d[keyword.start]..b2d[keyword.end];
            assert_eq!(&text[visible.clone()], &source[keyword.clone()]);
            for (index, _) in source[keyword.clone()].char_indices() {
                let source_index = keyword.start + index;
                assert_eq!(d2b[b2d[source_index]], source_index);
                assert!(text.is_char_boundary(b2d[source_index]));
            }
            let active_color = gpui::hsla(0.13, 1.0, 0.52, 1.0);
            let inactive_color = gpui::hsla(0.73, 1.0, 0.52, 1.0);
            let matches = [0..1, base + keyword.start..base + keyword.end];
            apply_search_match_text_styles(
                runs,
                text,
                d2b,
                base,
                &matches,
                Some(1),
                inactive_color,
                active_color,
            );
            let colors: Vec<_> = runs
                .iter()
                .flat_map(|run| std::iter::repeat_n(run.color, run.len))
                .collect();
            assert_eq!(colors.len(), text.len());
            for (index, _) in text.char_indices() {
                assert_eq!(
                    colors[index] == active_color,
                    visible.contains(&index),
                    "{text:?}, byte {index}"
                );
            }
        }

        #[test]
        fn search_source_reveal_ordinary_link_math_cjk_navigation_and_dismiss() {
            let source = "前 [名称](https://host/秘密) 中 $x+y$ 后";
            let mut decoration = markdown::parse_decorations(source).remove(0);
            let cache = inline_math_image_cache(&[("x+y", 183, 120)]);
            assert!(helpers::add_inline_math_display_replacements(
                &mut decoration,
                source,
                &cache,
                6.0
            ));
            let target = source.find("秘密").unwrap();
            let math = source.find("x+y").unwrap();
            let folded = display(source, &decoration, &[], false);
            assert!(!folded.0.contains("https://"));
            assert!(!folded.0.contains("x+y"));
            for keyword in [target..target + "秘密".len(), math..math + 3] {
                let mut mapped =
                    display(source, &decoration, std::slice::from_ref(&keyword), false);
                assert_keyword_mapping_and_highlight(source, &mut mapped, 100, keyword.clone());
                if keyword.start == target {
                    assert!(mapped.0.contains("[名称](https://host/秘密)"));
                    assert!(!mapped.0.contains("x+y"));
                } else {
                    assert!(mapped.0.contains("$x+y$"));
                    assert!(!mapped.0.contains("https://"));
                }
            }
            let spanning = 0..math + 3;
            let mut mapped = display(source, &decoration, std::slice::from_ref(&spanning), false);
            assert_eq!(mapped.0, source);
            assert_keyword_mapping_and_highlight(source, &mut mapped, 100, spanning);
            let dismissed = display(source, &decoration, &[], false);
            assert_eq!(
                (dismissed.0, dismissed.1, dismissed.2),
                (folded.0, folded.1, folded.2)
            );
        }

        #[test]
        fn search_source_reveal_table_is_independent_of_readonly_drag_and_caret_suppression() {
            let document = "| Head |\n| --- |\n| 前 [名称](https://host/秘密) 中 $x+y$ 后 |";
            let buffer = Buffer::from_text(document);
            let decorations = markdown::parse_decorations(document);
            let row = &decorations[2];
            let (source, cell_range) = crate::editor::table_cell_render::content_with_range(
                row.table_info.as_ref().unwrap(),
                buffer.line_content(2),
                0,
            );
            let mut decoration = table_cell_decoration(row, &source, &cell_range);
            let cache = inline_math_image_cache(&[("x+y", 183, 120)]);
            helpers::add_inline_math_display_replacements(&mut decoration, &source, &cache, 6.0);
            let base = buffer.line_start(2) + cell_range.start;
            let target = source.find("秘密").unwrap();
            let math = source.find("x+y").unwrap();
            for readonly in [false, true] {
                for dragging in [false, true] {
                    for suppress_empty_caret in [false, true] {
                        for keyword in [target..target + "秘密".len(), math..math + 3] {
                            let focus = helpers::TableCellSyntaxFocus {
                                selections: if readonly { vec![] } else { vec![base..base] },
                                search_reveal_range: Some(base + keyword.start..base + keyword.end),
                                dragging,
                                suppress_empty_caret,
                                ..Default::default()
                            };
                            let active = helpers::table_cell_active_ranges(
                                buffer.line_start(2),
                                &cell_range,
                                &focus,
                                &decoration,
                            );
                            assert!(active.contains(&keyword));
                            let mut mapped = display(&source, &decoration, &active, true);
                            assert_keyword_mapping_and_highlight(
                                &source,
                                &mut mapped,
                                base,
                                keyword.clone(),
                            );
                            assert_eq!(mapped.0.contains("https://"), keyword.start == target);
                            assert_eq!(mapped.0.contains("x+y"), keyword.start == math);
                        }
                    }
                }
            }
            let dismissed = helpers::TableCellSyntaxFocus::default();
            let active = helpers::table_cell_active_ranges(
                buffer.line_start(2),
                &cell_range,
                &dismissed,
                &decoration,
            );
            assert!(active.is_empty());
            let mapped = display(&source, &decoration, &active, true);
            assert!(!mapped.0.contains("https://"));
            assert!(!mapped.0.contains("x+y"));
        }

        #[test]
        fn search_source_reveal_table_clips_cross_cell_ranges_and_excludes_touching_boundaries() {
            let document = "| One | Two |\n| --- | --- |\n| [名](https://host/秘密) | $x+y$ |";
            let buffer = Buffer::from_text(document);
            let decorations = markdown::parse_decorations(document);
            let row = &decorations[2];
            let line_start = buffer.line_start(2);
            let first = document.find("秘密").unwrap();
            let last = document.find("x+y").unwrap() + 3;
            for col in 0..2 {
                let (source, cell_range) = crate::editor::table_cell_render::content_with_range(
                    row.table_info.as_ref().unwrap(),
                    buffer.line_content(2),
                    col,
                );
                let decoration = table_cell_decoration(row, &source, &cell_range);
                let start = line_start + cell_range.start;
                let end = line_start + cell_range.end;
                let focus = helpers::TableCellSyntaxFocus {
                    search_reveal_range: Some(first..last),
                    dragging: true,
                    suppress_empty_caret: true,
                    ..Default::default()
                };
                let active =
                    helpers::table_cell_active_ranges(line_start, &cell_range, &focus, &decoration);
                assert_eq!(
                    active,
                    vec![first.max(start) - start..last.min(end) - start]
                );
                let mut mapped = display(&source, &decoration, &active, true);
                let keyword = if col == 0 { "秘密" } else { "x+y" };
                let local = source.find(keyword).unwrap();
                assert_keyword_mapping_and_highlight(
                    &source,
                    &mut mapped,
                    start,
                    local..local + keyword.len(),
                );
                for range in [start - 1..start, end..end + 1, start..start] {
                    let focus = helpers::TableCellSyntaxFocus {
                        search_reveal_range: Some(range),
                        ..Default::default()
                    };
                    assert!(
                        helpers::table_cell_active_ranges(
                            line_start,
                            &cell_range,
                            &focus,
                            &decoration
                        )
                        .is_empty()
                    );
                }
            }
        }

        #[test]
        fn search_source_reveal_front_matter_readonly_groups_and_lifecycle() {
            let source = "---\ntitle: 'scalar'\nbody: |-\n  中文🌸\n  second\ntags:\n  - 'one'\n  - \"二\\u263a\"\nother: true\n---\nBody";
            let buffer = Buffer::from_text(source);
            let decorations = markdown::parse_decorations(source);
            let selections = [0..buffer.len()];
            let signature = |search: Option<&Range<usize>>| {
                let plans = helpers::front_matter_group_plan_with_search(
                    &buffer,
                    &decorations,
                    &selections,
                    false,
                    search,
                );
                let delimiter = helpers::front_matter_block_is_active_with_search(
                    &buffer,
                    &decorations,
                    &selections,
                    false,
                    search,
                );
                let scalar_rows: Vec<_> = (1..9)
                    .map(|line| {
                        helpers::front_matter_source_rows_are_revealed(
                            &buffer,
                            line..line + 1,
                            &selections,
                            false,
                            search,
                        )
                    })
                    .collect();
                (plans, delimiter, scalar_rows)
            };
            let (plans, delimiter, rows) = signature(None);
            assert!(!delimiter);
            assert!(rows.iter().all(|active| !active));
            assert!(plans.iter().all(|plan| !plan.active));
            assert!(plans[0].hides_line(2));
            assert!(plans[1].hides_line(7));
            for (line, keyword, group) in [
                (3, "中文🌸", Some(0)),
                (7, "\\u263a", Some(1)),
                (1, "scalar", None),
                (8, "true", None),
            ] {
                let local = buffer.line_content(line).find(keyword).unwrap();
                let base = buffer.line_start(line);
                let search = base + local..base + local + keyword.len();
                let (plans, delimiter, rows) = signature(Some(&search));
                assert!(delimiter);
                assert_eq!(rows.iter().filter(|active| **active).count(), 1);
                assert!(rows[line - 1]);
                for (index, plan) in plans.iter().enumerate() {
                    assert_eq!(plan.active, group == Some(index));
                    if plan.active {
                        assert!(
                            plan.group
                                .source_lines
                                .clone()
                                .all(|member| !plan.hides_line(member))
                        );
                    }
                }
                let mut decoration = decorations[line].clone();
                let plan = plans
                    .iter()
                    .find(|plan| plan.group.source_lines.contains(&line));
                helpers::apply_front_matter_group_display(
                    &mut decoration,
                    line,
                    plan,
                    buffer.line_content(line),
                );
                let active = if plan.is_some_and(|plan| plan.active) {
                    0..buffer.line_content(line).len()
                } else {
                    local..local + keyword.len()
                };
                let mut mapped = display(buffer.line_content(line), &decoration, &[active], false);
                assert_keyword_mapping_and_highlight(
                    buffer.line_content(line),
                    &mut mapped,
                    base,
                    local..local + keyword.len(),
                );
            }
            let (dismissed, delimiter, rows) = signature(None);
            assert!(!delimiter);
            assert!(rows.iter().all(|active| !active));
            assert_eq!(
                dismissed
                    .iter()
                    .map(|plan| (plan.active, plan.source_visible))
                    .collect::<Vec<_>>(),
                plans
                    .iter()
                    .map(|plan| (plan.active, plan.source_visible))
                    .collect::<Vec<_>>()
            );
            assert_eq!(buffer.text(), source);
        }

        #[test]
        fn search_source_reveal_front_matter_delimiters_and_half_open_boundaries() {
            let source = "---\ntitle: '中'\n---\nBody";
            let buffer = Buffer::from_text(source);
            let decorations = markdown::parse_decorations(source);
            let body = buffer.line_start(3);
            for range in [0..3, buffer.line_start(2)..buffer.line_end(2)] {
                assert!(helpers::front_matter_block_is_active_with_search(
                    &buffer,
                    &decorations,
                    &[],
                    false,
                    Some(&range)
                ));
            }
            for range in [body..buffer.len(), body..body] {
                assert!(!helpers::front_matter_block_is_active_with_search(
                    &buffer,
                    &decorations,
                    &[0..body],
                    false,
                    Some(&range)
                ));
            }
            let rows = 1..2;
            let start = buffer.line_start(1);
            let end = buffer.line_start(2);
            for range in [0..start, end..body, start..start] {
                assert!(!helpers::front_matter_source_rows_are_revealed(
                    &buffer,
                    rows.clone(),
                    &[start..end],
                    false,
                    Some(&range)
                ));
            }
        }

        #[test]
        fn search_source_reveal_front_matter_raw_scalar_values_keep_exact_mapping() {
            let source = "---\nquoted: \"中文\\u263a\"\nflag: true\nwhen: 2026-10-16\n---";
            let buffer = Buffer::from_text(source);
            let decorations = markdown::parse_decorations(source);
            for (line, keyword) in [(1, "\\u263a"), (2, "true"), (3, "2026-10-16")] {
                let text = buffer.line_content(line);
                let mut decoration = decorations[line].clone();
                if line == 3 {
                    decoration.front_matter.as_mut().unwrap().value =
                        FrontMatterValue::Text("16 October 2026".into());
                }
                let folded = display(text, &decoration, &[], false);
                assert!(!folded.0.contains(keyword));
                let start = text.find(keyword).unwrap();
                let mut mapped = display(text, &decoration, &[start..start + keyword.len()], false);
                assert_keyword_mapping_and_highlight(
                    text,
                    &mut mapped,
                    buffer.line_start(line),
                    start..start + keyword.len(),
                );
                let dismissed = display(text, &decoration, &[], false);
                assert_eq!(
                    (dismissed.0, dismissed.1, dismissed.2),
                    (folded.0, folded.1, folded.2)
                );
            }
        }
    }

    fn inline_math_image_cache(
        entries: &[(&str, u32, u32)],
    ) -> std::collections::HashMap<String, Option<std::sync::Arc<gpui::RenderImage>>> {
        entries
            .iter()
            .map(|&(source, width, height)| {
                (
                    super::inline_math_cache_key(source),
                    Some(std::sync::Arc::new(gpui::RenderImage::new([
                        image::Frame::new(image::RgbaImage::new(width, height)),
                    ]))),
                )
            })
            .collect()
    }

    fn inline_math_test_wrapped_line(display: &str, boundaries: &[usize]) -> WrappedLine {
        // GlyphId is a repr(C) u32 without a public constructor; these glyphs are never painted.
        let glyph_id = unsafe { std::mem::zeroed::<GlyphId>() };
        let glyphs = display
            .char_indices()
            .enumerate()
            .map(|(column, (index, _))| ShapedGlyph {
                id: glyph_id,
                position: point(px(column as f32 * 6.0), px(0.0)),
                index,
                is_emoji: false,
            })
            .collect::<Vec<_>>();
        let wrap_boundaries = boundaries
            .iter()
            .map(|index| WrapBoundary {
                run_ix: 0,
                glyph_ix: glyphs
                    .iter()
                    .position(|glyph| glyph.index == *index)
                    .unwrap(),
            })
            .collect();
        let mut wrapped = WrappedLine::default();
        wrapped.text = display.to_string().into();
        let layout: &mut Arc<WrappedLineLayout> = &mut wrapped;
        *layout = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: px(16.0),
                width: px(glyphs.len() as f32 * 6.0),
                ascent: px(16.0),
                descent: px(4.0),
                runs: vec![ShapedRun {
                    font_id: FontId(0),
                    glyphs,
                }],
                len: display.len(),
            }),
            wrap_boundaries,
            wrap_width: None,
            row_heights: Vec::new(),
        });
        wrapped
    }

    fn inline_math_test_display(
        source: &str,
        active_ranges: &[Range<usize>],
        cache: &HashMap<String, Option<Arc<RenderImage>>>,
        boundaries: &[usize],
    ) -> (markdown::LineDecoration, Vec<usize>, WrappedLine) {
        let mut decoration = markdown::parse_decorations(source).remove(0);
        super::add_inline_math_display_replacements(&mut decoration, source, cache, 6.0);
        let (display, _, buffer_to_display, _) = build_display_text(
            source,
            &decoration,
            active_ranges,
            false,
            None,
            3,
            3,
            2,
            0,
            0,
            1.0,
            1.0,
            1.0,
        );
        let wrapped = inline_math_test_wrapped_line(&display, boundaries);
        (decoration, buffer_to_display, wrapped)
    }

    #[test]
    fn inline_math_placeholder_reserves_intrinsic_width_without_upscaling() {
        let source = "energy $E=mc^2$";
        let mut decoration = markdown::parse_decorations(source).remove(0);
        let cache = inline_math_image_cache(&[("E=mc^2", 183, 120)]);
        assert!(super::add_inline_math_display_replacements(
            &mut decoration,
            source,
            &cache,
            6.0
        ));
        let replacement = decoration.display_replacements.last().unwrap();
        // 183 raster pixels / 3 = 61 logical pixels: round 61 / 6 up, not down.
        assert_eq!(replacement.display_text, "\u{a0}".repeat(11));
        assert_eq!(replacement.trigger_range, 7..15);
        let (decoration, buffer_to_display, mut wrapped) =
            inline_math_test_display(source, &[], &cache, &[]);
        super::apply_inline_math_row_heights(
            &mut wrapped,
            &decoration,
            source,
            &buffer_to_display,
            &[],
            &cache,
            24.0,
            0,
        );
        assert_eq!(wrapped.row_heights, vec![px(40.0)]);
    }

    #[test]
    fn inline_math_placeholder_advance_matches_image_without_horizontal_padding() {
        // Eleven NBSPs occupy 66px before letter spacing, while the formula is 61px.
        let range = 4..26;
        for shaped_width in [44.0, 61.0, 66.0, 88.0] {
            let spans = [(&range, px(20.0), 61.0, 61.0 - shaped_width)];
            assert_eq!(
                super::inline_math_placeholder_glyph_x(2, px(14.0), &spans),
                px(14.0)
            );
            assert_eq!(
                super::inline_math_placeholder_glyph_x(4, px(20.0), &spans),
                px(20.0)
            );
            assert_eq!(
                super::inline_math_placeholder_glyph_x(26, px(20.0 + shaped_width), &spans),
                px(81.0)
            );
            // A real space after the formula keeps its own width.
            assert_eq!(
                super::inline_math_placeholder_glyph_x(28, px(26.0 + shaped_width), &spans),
                px(87.0)
            );
        }
    }

    #[test]
    fn adjacent_inline_math_placeholders_do_not_accumulate_rounding_gaps() {
        let first = 0..22;
        let second = 22..30;
        let spans = [
            (&first, px(10.0), 61.0, -5.0),
            (&second, px(76.0), 20.0, -4.0),
        ];
        assert_eq!(
            super::inline_math_placeholder_glyph_x(22, px(76.0), &spans),
            px(71.0)
        );
        assert_eq!(
            super::inline_math_placeholder_glyph_x(30, px(100.0), &spans),
            px(91.0)
        );
    }

    #[test]
    fn inline_math_display_range_includes_all_placeholder_characters() {
        let source = "before $x$ after";
        let mut decoration = markdown::parse_decorations(source).remove(0);
        let cache = inline_math_image_cache(&[("x", 183, 120)]);
        super::add_inline_math_display_replacements(&mut decoration, source, &cache, 6.0);
        let (display, _, buffer_to_display, _) = build_display_text(
            source,
            &decoration,
            &[],
            false,
            None,
            3,
            3,
            2,
            0,
            0,
            1.0,
            1.0,
            1.0,
        );
        let range = super::inline_math_display_range(
            &decoration,
            &decoration.inline_math_spans[0],
            &buffer_to_display,
        )
        .unwrap();
        assert_eq!(range, 7..29);
        assert_eq!(&display[range], "\u{a0}".repeat(11));
        assert!(super::buffer_to_display_offset(&buffer_to_display, 8) > 7);
    }

    #[test]
    fn inline_math_row_height_fits_only_the_tallest_collapsed_formula() {
        let source = "$x$ and $y$ and $z$";
        let cache = inline_math_image_cache(&[("x", 30, 30), ("y", 60, 180), ("z", 60, 120)]);
        for (active_ranges, base_height, expected_height) in [
            (vec![], 24.0, 60.0),
            (vec![9..9], 24.0, 40.0),
            (vec![0..source.len()], 24.0, 24.0),
            (vec![], 80.0, 80.0),
        ] {
            let (decoration, buffer_to_display, mut wrapped) =
                inline_math_test_display(source, &active_ranges, &cache, &[]);
            super::apply_inline_math_row_heights(
                &mut wrapped,
                &decoration,
                source,
                &buffer_to_display,
                &active_ranges,
                &cache,
                base_height,
                0,
            );
            assert_eq!(wrapped.row_height(0, px(base_height)), px(expected_height));
            assert_eq!(wrapped.size(px(base_height)).height, px(expected_height));
            if expected_height == base_height {
                assert!(wrapped.row_heights.is_empty());
            }
        }
    }

    #[test]
    fn inline_math_row_heights_expand_only_the_formula_row_at_wrap_boundaries() {
        let cache = inline_math_image_cache(&[("y", 60, 180)]);
        for (source, boundaries, expected) in [
            ("text $y$ after", [5, 14], [24.0, 60.0, 24.0]),
            ("$y$ text after", [9, 14], [60.0, 24.0, 24.0]),
        ] {
            let (decoration, buffer_to_display, mut wrapped) =
                inline_math_test_display(source, &[], &cache, &boundaries);
            super::apply_inline_math_row_heights(
                &mut wrapped,
                &decoration,
                source,
                &buffer_to_display,
                &[],
                &cache,
                24.0,
                0,
            );
            assert_eq!(wrapped.row_heights, expected.map(px).to_vec());
            assert_eq!(wrapped.size(px(24.0)).height, px(108.0));
            assert_eq!(wrapped.row_top(2, px(24.0)), px(84.0));
        }
    }

    #[test]
    fn inline_math_row_heights_clear_when_formulas_expand() {
        let source = "text $y$ after";
        let cache = inline_math_image_cache(&[("y", 60, 180)]);
        let (decoration, buffer_to_display, mut wrapped) =
            inline_math_test_display(source, &[], &cache, &[5, 14]);
        wrapped.set_row_heights(vec![px(24.0), px(60.0), px(24.0)]);
        super::apply_inline_math_row_heights(
            &mut wrapped,
            &decoration,
            source,
            &buffer_to_display,
            &[7..7],
            &cache,
            24.0,
            0,
        );
        assert!(wrapped.row_heights.is_empty());
        assert_eq!(wrapped.size(px(24.0)).height, px(72.0));
    }

    #[test]
    fn inline_math_row_heights_use_local_display_offsets() {
        let source = "before $x$ and $y$ after";
        let cache = inline_math_image_cache(&[("x", 30, 120), ("y", 60, 180)]);
        let (decoration, buffer_to_display, wrapped) =
            inline_math_test_display(source, &[], &cache, &[]);
        let range = super::inline_math_display_range(
            &decoration,
            &decoration.inline_math_spans[1],
            &buffer_to_display,
        )
        .unwrap();
        let mut segment = inline_math_test_wrapped_line(&wrapped.text[range.start..], &[]);
        super::apply_inline_math_row_heights(
            &mut segment,
            &decoration,
            source,
            &buffer_to_display,
            &[],
            &cache,
            24.0,
            range.start,
        );
        assert_eq!(segment.row_heights, vec![px(60.0)]);
        // Include "and " before the formula so global byte offsets would assign it to the wrong row.
        let display_offset = range.start - 4;
        let formula_end = range.end - display_offset;
        let mut segment =
            inline_math_test_wrapped_line(&wrapped.text[display_offset..], &[4, formula_end + 1]);
        super::apply_inline_math_row_heights(
            &mut segment,
            &decoration,
            source,
            &buffer_to_display,
            &[],
            &cache,
            24.0,
            display_offset,
        );
        assert_eq!(segment.row_heights, vec![px(24.0), px(60.0), px(24.0)]);
        assert_eq!(segment.row_top(1, px(24.0)), px(24.0));
        assert_eq!(segment.row_top(2, px(24.0)), px(84.0));
        let mut segment = inline_math_test_wrapped_line(&wrapped.text[..range.start], &[]);
        super::apply_inline_math_row_heights(
            &mut segment,
            &decoration,
            source,
            &buffer_to_display,
            &[],
            &cache,
            24.0,
            0,
        );
        assert_eq!(segment.row_heights, vec![px(40.0)]);
    }

    #[test]
    fn inline_math_uncached_images_keep_source_geometry() {
        let source = "$x$";
        let mut decoration = markdown::parse_decorations(source).remove(0);
        let cache = std::collections::HashMap::new();
        assert!(!super::add_inline_math_display_replacements(
            &mut decoration,
            source,
            &cache,
            6.0
        ));
        assert!(decoration.display_replacements.is_empty());
        let (mut decoration, buffer_to_display, mut wrapped) =
            inline_math_test_display(source, &[], &cache, &[]);
        wrapped.set_row_heights(vec![px(60.0)]);
        super::apply_inline_math_row_heights(
            &mut wrapped,
            &decoration,
            source,
            &buffer_to_display,
            &[],
            &cache,
            24.0,
            0,
        );
        assert!(wrapped.row_heights.is_empty());
        assert_eq!(wrapped.size(px(24.0)).height, px(24.0));
        let cache = inline_math_image_cache(&[("x", 30, 120)]);
        assert!(!super::add_inline_math_display_replacements(
            &mut decoration,
            source,
            &cache,
            0.0
        ));
    }

    #[test]
    fn baseline_font_tracks_block_role_not_inline_content() {
        let body = font("Test Body");
        let heading = font("Test Heading");
        let mono = font("Test Mono");
        for (block_type, expected) in [
            (markdown::BlockType::Normal, body.clone()),
            (markdown::BlockType::UnorderedList, body.clone()),
            (markdown::BlockType::Heading(2), heading.clone().bold()),
            (markdown::BlockType::CodeBlock, mono.clone()),
            (markdown::BlockType::CodeBlockFence, mono.clone()),
        ] {
            let mut decoration = markdown::LineDecoration::default();
            decoration.block_type = block_type;
            assert_eq!(
                super::line_baseline_font(&decoration, false, &body, &heading, &mono),
                expected,
            );
            assert_eq!(
                super::line_baseline_font(&decoration, true, &body, &heading, &mono),
                mono,
            );
        }
        for text in ["English", "English中", "**English中**", "`code` English中"] {
            let decorations = markdown::parse_decorations(text);
            assert_eq!(
                super::line_baseline_font(&decorations[0], false, &body, &heading, &mono),
                body,
            );
        }
    }

    #[test]
    fn table_layout_never_exceeds_available_width() {
        let text = "| A | B | C | D | E | F | G | H |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| long content | long content | long content | long content | long content | long content | long content | long content |";
        let decorations = markdown::parse_decorations(text);
        for available_width in [80.0, 160.0, 320.0] {
            let layout = compute_table_layout(
                &Buffer::from_text(text),
                &decorations,
                0,
                3,
                available_width,
                8.0,
                4.0,
                8.0,
                20.0,
                16.0,
            );
            assert!(layout.total_width <= available_width + 0.01);
            assert!(layout.cells.iter().flatten().all(|cell| cell.width > 0.0));
        }
    }

    fn table_layout_fallback(text: &str, available_width: f32, scale: f32) -> super::TableLayout {
        let decorations = markdown::parse_decorations(text);
        compute_table_layout(
            &Buffer::from_text(text),
            &decorations,
            0,
            decorations.len(),
            available_width,
            8.0 * scale,
            4.0 * scale,
            8.0 * scale,
            20.0 * scale,
            16.0 * scale,
        )
    }

    fn assert_table_layout_fallback_widths(
        layout: &super::TableLayout,
        expected_content_widths: &[f32],
        available_width: f32,
    ) {
        let tolerance = (available_width * f32::EPSILON * 8.0).max(0.001);
        assert!(!layout.is_front_matter);
        assert_eq!(layout.col_count, expected_content_widths.len());
        assert_eq!(layout.cells.len(), layout.visual_row_count);
        assert!(!layout.cells.is_empty());
        assert!(layout.total_width <= available_width + tolerance);
        assert!((layout.total_width - available_width).abs() <= tolerance);
        for row in &layout.cells {
            assert_eq!(row.len(), expected_content_widths.len());
            let mut x = 0.0;
            for (cell, &expected) in row.iter().zip(expected_content_widths) {
                let content_width = cell.width - layout.cell_padding_x * 2.0;
                assert!(content_width.is_finite() && content_width > 0.0);
                assert!(
                    (content_width - expected).abs() <= tolerance,
                    "column {}: expected {expected}, got {content_width}",
                    cell.col,
                );
                assert!((cell.x - x).abs() <= tolerance);
                x += cell.width;
            }
            assert!((x - layout.total_width).abs() <= tolerance);
        }
    }

    #[test]
    fn table_layout_fallback_short_column_fits_and_long_column_gets_remainder() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        let text = format!("| A | B |\n| --- | --- |\n| ok | {paragraph} |");
        let layout = table_layout_fallback(&text, 300.0, 1.0);
        assert_eq!(layout.cells[1][0].display_text, "ok");
        // Two characters at eight pixels, with 32 pixels of table padding.
        assert_table_layout_fallback_widths(&layout, &[16.0, 252.0], 300.0);
    }

    #[test]
    fn table_layout_fallback_long_url_targets_do_not_widen_short_aliases() {
        let target = format!("https://example.test/{}", "very-long-path/".repeat(120));
        let paragraph = "a long paragraph with many words ".repeat(20);
        assert!(target.len() > paragraph.len());
        for link in [format!("[[{target}|Go]]"), format!("[Go]({target})")] {
            let text = format!("| A | B |\n| --- | --- |\n| {link} | {paragraph} |");
            let layout = table_layout_fallback(&text, 300.0, 1.0);
            assert_eq!(layout.col_count, 2);
            assert_eq!(layout.cells[1][0].display_text, "Go");
            assert_table_layout_fallback_widths(&layout, &[16.0, 252.0], 300.0);
        }
    }

    #[test]
    fn table_layout_fallback_break_tags_measure_only_the_longest_hard_line() {
        let paragraph = "a longer column with many words ".repeat(20);
        for (content, longest_line, breaks) in [
            ("<br>a".repeat(80), 1, 80),
            ("a<br>bbbb<BR/>cc<br />d".to_string(), 4, 3),
        ] {
            let text = format!("| A | B |\n| --- | --- |\n| {content} | {paragraph} |");
            let layout = table_layout_fallback(&text, 300.0, 1.0);
            let display = &layout.cells[1][0].display_text;
            assert!(!display.contains('<'));
            assert_eq!(display.matches('\n').count(), breaks);
            assert_eq!(
                display.split('\n').map(display_width).max(),
                Some(longest_line)
            );
            let short_width = longest_line as f32 * 8.0;
            assert_table_layout_fallback_widths(
                &layout,
                &[short_width, 268.0 - short_width],
                300.0,
            );
        }
    }

    #[test]
    fn table_layout_fallback_hidden_bold_markers_do_not_consume_width() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        let text = format!("| A | B |\n| --- | --- |\n| **ok** | {paragraph} |");
        let layout = table_layout_fallback(&text, 300.0, 1.0);
        assert_eq!(layout.cells[1][0].display_text, "ok");
        assert_table_layout_fallback_widths(&layout, &[16.0, 252.0], 300.0);
    }

    #[test]
    fn table_layout_fallback_header_and_all_body_rows_contribute_maxima() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        let text = format!(
            "| **Heading** | B | C |\n| --- | --- | --- |\n| x | ok | {paragraph} |\n| y | longest | z |"
        );
        let layout = table_layout_fallback(&text, 500.0, 1.0);
        assert_eq!(layout.cells[0][0].display_text, "Heading");
        assert_eq!(layout.cells[2][1].display_text, "longest");
        assert_eq!(layout.is_header, vec![true, false, false]);
        assert_table_layout_fallback_widths(&layout, &[56.0, 56.0, 340.0], 500.0);
    }

    #[test]
    fn table_layout_fallback_surplus_does_not_expand_short_columns() {
        let text = "| A | B | C |\n| --- | --- | --- |\n| a | bbb | abcdefghij |";
        let layout = table_layout_fallback(text, 600.0, 1.0);
        assert_table_layout_fallback_widths(&layout, &[8.0, 24.0, 520.0], 600.0);

        let tied = "| A | B | C |\n| --- | --- | --- |\n| a | abcdefghij | klmnopqrst |";
        let layout = table_layout_fallback(tied, 600.0, 1.0);
        assert_table_layout_fallback_widths(&layout, &[8.0, 272.0, 272.0], 600.0);
    }

    #[test]
    fn table_layout_fallback_multiple_long_columns_share_remaining_space() {
        let text = format!(
            "| A | B | C | D |\n| --- | --- | --- | --- |\n| ok | four | {} | {} |",
            "x".repeat(100),
            "y".repeat(200),
        );
        let layout = table_layout_fallback(&text, 500.0, 1.0);
        assert_table_layout_fallback_widths(&layout, &[16.0, 32.0, 194.0, 194.0], 500.0);
    }

    #[test]
    fn table_layout_fallback_unicode_uses_display_columns_not_utf8_bytes() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        for (content, display, columns) in [
            ("東京", "東京", 4),
            ("**東京**", "東京", 4),
            ("[[notes/a-very-long-target.md|東京]]", "東京", 4),
            ("é", "é", 1),
        ] {
            let text = format!("| A | B |\n| --- | --- |\n| {content} | {paragraph} |");
            let layout = table_layout_fallback(&text, 300.0, 1.0);
            assert_eq!(layout.cells[1][0].display_text, display);
            assert_eq!(display_width(display), columns);
            let short_width = columns as f32 * 8.0;
            assert_table_layout_fallback_widths(
                &layout,
                &[short_width, 268.0 - short_width],
                300.0,
            );
        }
    }

    #[test]
    fn table_layout_fallback_empty_column_keeps_one_character_minimum() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        let text = format!("| | B |\n| --- | --- |\n| | {paragraph} |");
        let layout = table_layout_fallback(&text, 300.0, 1.0);
        assert_eq!(layout.cells[0][0].display_text, "");
        assert_eq!(layout.cells[1][0].display_text, "");
        assert_table_layout_fallback_widths(&layout, &[8.0, 260.0], 300.0);
    }

    #[test]
    fn table_layout_intrinsic_nonfinal_overlays_keep_all_reserved_advance() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        for overlay in ["x^abcdefghij^", "x~abcdefghij~", "x[^abcdefghij]"] {
            for padding in ["", "   \t"] {
                let content = format!("{overlay}{padding}<br>z");
                let decoration = table_cell_decoration(
                    &LineDecoration::default(),
                    &content,
                    &(0..content.len()),
                );
                let (display, _, b2d, _) =
                    table_cell_inline_display_and_runs(&content, &decoration);
                let ranges = super::hard_line_byte_ranges(&display);
                assert_eq!(ranges.len(), 2);
                let end = super::super::table_cell_render::intrinsic_line_end(
                    &content,
                    &decoration.table_cell_fragments[0],
                    &b2d,
                    &ranges[0],
                );
                assert_eq!(end, 11, "{content:?}");
                assert_eq!(
                    &display[ranges[0].start..ranges[0].start + end],
                    "x          "
                );
                assert_eq!(ranges[0].len() - end, padding.len());

                let text = format!("| A | B |\n| --- | --- |\n| {content} | {paragraph} |");
                let layout = table_layout_fallback(&text, 300.0, 1.0);
                assert_table_layout_fallback_widths(&layout, &[88.0, 180.0], 300.0);
            }
        }
    }

    #[test]
    fn table_layout_intrinsic_real_source_trailing_padding_is_excluded() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        for content in ["ab   \t<br>z", "ab<br>z   \t", "ab   \t<br>z   \t"] {
            let decoration =
                table_cell_decoration(&LineDecoration::default(), content, &(0..content.len()));
            let (display, _, b2d, _) = table_cell_inline_display_and_runs(content, &decoration);
            for ((range, fragment), expected) in super::hard_line_byte_ranges(&display)
                .iter()
                .zip(&decoration.table_cell_fragments)
                .zip(["ab", "z"])
            {
                let end = super::super::table_cell_render::intrinsic_line_end(
                    content, fragment, &b2d, range,
                );
                assert_eq!(&display[range.start..range.start + end], expected);
            }
            let text = format!("| A | B |\n| --- | --- |\n| {content} | {paragraph} |");
            let layout = table_layout_fallback(&text, 300.0, 1.0);
            assert_table_layout_fallback_widths(&layout, &[16.0, 252.0], 300.0);
        }
    }

    #[test]
    fn table_layout_intrinsic_empty_complete_prefixes_retain_reserved_advance() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        for (prefix, columns) in [("- [ ] ", 3), ("- [x] ", 3), ("123. ", 5), ("12) ", 4)] {
            for leading in ["", "q<br>"] {
                let content = format!("{leading}{prefix}<br>z");
                let decoration = table_cell_decoration(
                    &LineDecoration::default(),
                    &content,
                    &(0..content.len()),
                );
                let (display, _, b2d, _) =
                    table_cell_inline_display_and_runs(&content, &decoration);
                let ranges = super::hard_line_byte_ranges(&display);
                let index = usize::from(!leading.is_empty());
                let fragment = &decoration.table_cell_fragments[index];
                assert_eq!(fragment.prefix_len, prefix.len());
                let end = super::super::table_cell_render::intrinsic_line_end(
                    &content,
                    fragment,
                    &b2d,
                    &ranges[index],
                );
                assert_eq!(end, columns, "{content:?}");
                assert_eq!(&display[ranges[index].clone()], " ".repeat(columns));

                let text = format!("| A | B |\n| --- | --- |\n| {content} | {paragraph} |");
                let layout = table_layout_fallback(&text, 300.0, 1.0);
                let width = columns as f32 * 8.0;
                assert_table_layout_fallback_widths(&layout, &[width, 268.0 - width], 300.0);
            }
        }
    }

    #[test]
    fn table_layout_intrinsic_cached_math_matches_exact_rendered_advance() {
        let cache = inline_math_image_cache(&[("x", 183, 120), ("y", 60, 120)]);
        for (content, formula_line, has_space) in [
            ("$x$$y$<br>z", 0, false),
            ("z<br>$x$$y$<br>z", 1, false),
            ("z<br>$x$ $y$  \t<br>z", 1, true),
        ] {
            let mut decoration =
                table_cell_decoration(&LineDecoration::default(), content, &(0..content.len()));
            assert_eq!(decoration.inline_math_spans.len(), 2);
            assert!(super::add_inline_math_display_replacements(
                &mut decoration,
                content,
                &cache,
                6.0,
            ));
            let (display, _, b2d, _) = table_cell_inline_display_and_runs(content, &decoration);
            let ranges = super::hard_line_byte_ranges(&display);
            let formula_ranges = decoration
                .inline_math_spans
                .iter()
                .map(|source_range| {
                    super::inline_math_display_range(&decoration, source_range, &b2d).unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                display[ranges[formula_line].clone()]
                    .chars()
                    .filter(|&ch| ch == '\u{a0}')
                    .count(),
                15,
            );
            for tracking in [0.5, 2.0] {
                let gap = if has_space { 6.0 + tracking } else { 0.0 };
                let expected = 61.0 + gap + 20.0;
                let mut longest = 0.0_f32;
                for (index, (range, fragment)) in ranges
                    .iter()
                    .zip(&decoration.table_cell_fragments)
                    .enumerate()
                {
                    let mut line = inline_math_test_wrapped_line(&display[range.clone()], &[]);
                    super::apply_letter_spacing(&mut line, tracking);
                    super::keep_inline_math_placeholders_unwrapped(
                        &mut line,
                        &decoration,
                        content,
                        &b2d,
                        &[],
                        &cache,
                        range.start,
                    );
                    let end = super::super::table_cell_render::intrinsic_line_end(
                        content, fragment, &b2d, range,
                    );
                    let measured = f32::from(line.unwrapped_layout.x_for_index(end));
                    longest = longest.max(measured);
                    if index != formula_line {
                        continue;
                    }
                    assert_eq!(end, formula_ranges[1].end - range.start);
                    for (formula, width) in formula_ranges.iter().zip([61.0, 20.0]) {
                        let left = line
                            .unwrapped_layout
                            .x_for_index(formula.start - range.start);
                        let right = line.unwrapped_layout.x_for_index(formula.end - range.start);
                        assert!((f32::from(right - left) - width).abs() <= 0.001);
                    }
                    let first_end = line
                        .unwrapped_layout
                        .x_for_index(formula_ranges[0].end - range.start);
                    let second_start = line
                        .unwrapped_layout
                        .x_for_index(formula_ranges[1].start - range.start);
                    assert!((f32::from(second_start - first_end) - gap).abs() <= 0.001);
                    assert!(
                        (measured - expected).abs() <= 0.001,
                        "{content:?}, tracking {tracking}: expected {expected}, got {measured}",
                    );
                }
                assert!((longest - expected).abs() <= 0.001);
                let widths =
                    super::super::table_column_widths::allocate(&[longest, 400.0], 268.0, 6.0);
                assert!((widths[0] - expected).abs() <= 0.001);
                assert!((widths[1] - (268.0 - expected)).abs() <= 0.001);
            }
        }
    }

    #[test]
    fn table_layout_fallback_view_width_and_scale_preserve_content_budget() {
        let paragraph = "a long paragraph with many words ".repeat(20);
        let text = format!("| A | B |\n| --- | --- |\n| Go | {paragraph} |");
        for view_width in [
            8.0_f32, 16.0, 32.0, 64.0, 80.0, 160.0, 300.0, 1200.0, 8192.0,
        ] {
            for scale in [0.5, 1.0, 1.25, 2.0, 3.0] {
                let available = view_width * scale;
                let layout = table_layout_fallback(&text, available, scale);
                let padding = (8.0 * scale).min(available / 8.0);
                assert!((layout.cell_padding_x - padding).abs() <= 0.001);
                let content_budget = available - padding * 4.0;
                let short_width = (16.0 * scale).min(content_budget / 2.0);
                assert_table_layout_fallback_widths(
                    &layout,
                    &[short_width, content_budget - short_width],
                    available,
                );
            }
        }
    }

    #[test]
    fn baseline_font_distinguishes_table_headers_and_front_matter() {
        let body = font("Test Body");
        let heading = font("Test Heading");
        let mono = font("Test Mono");
        let table = markdown::parse_decorations("| Key | Value |\n| --- | --- |\n| A | 中 |");
        assert_eq!(
            super::line_baseline_font(&table[0], false, &body, &heading, &mono),
            body.clone().bold(),
        );
        assert_eq!(
            super::line_baseline_font(&table[2], false, &body, &heading, &mono),
            body,
        );
        let front_matter = markdown::parse_decorations("---\ntitle: 中\n---");
        assert_eq!(
            super::line_baseline_font(&front_matter[1], false, &body, &heading, &mono),
            mono,
        );
    }

    #[test]
    fn search_highlight_only_considers_matches_that_touch_the_line() {
        fn plain_run(len: usize) -> TextRun {
            TextRun {
                len,
                font: font("Helvetica"),
                color: gpui::hsla(0.0, 0.0, 0.0, 1.0),
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        }

        let display_text = "abcdef";
        let display_to_buffer: Vec<usize> = (0..display_text.len()).collect();
        let inactive = gpui::hsla(0.5, 1.0, 0.5, 1.0);
        let active = gpui::hsla(0.1, 1.0, 0.5, 1.0);

        // The line covers buffer offsets 100..106; the surrounding matches must
        // not colour it, and the active index still refers to the global list.
        let matches: Vec<Range<usize>> = vec![0..3, 50..55, 102..104, 500..504, 900..903];
        let mut runs = vec![plain_run(display_text.len())];
        apply_search_match_text_styles(
            &mut runs,
            display_text,
            &display_to_buffer,
            100,
            &matches,
            Some(2),
            inactive,
            active,
        );

        let colors: Vec<_> = runs.iter().map(|run| (run.len, run.color)).collect();
        assert_eq!(
            colors,
            vec![
                (2, gpui::hsla(0.0, 0.0, 0.0, 1.0)),
                (2, active),
                (2, gpui::hsla(0.0, 0.0, 0.0, 1.0)),
            ]
        );

        // A line with no overlapping match keeps its runs untouched.
        let mut untouched = vec![plain_run(display_text.len())];
        apply_search_match_text_styles(
            &mut untouched,
            display_text,
            &display_to_buffer,
            200,
            &matches,
            Some(2),
            inactive,
            active,
        );
        assert_eq!(untouched.len(), 1);
        assert_eq!(untouched[0].color, gpui::hsla(0.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn table_cell_alignment_offsets_content_within_available_width() {
        use markdown::TableAlign;

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
        assert_eq!(
            table_cell_content_offset(TableAlign::Right, 40.0, 100.0),
            0.0
        );
    }

    #[test]
    fn editor_mermaid_raster_stays_within_texture_budget() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10000" height="2000" viewBox="0 0 10000 2000"><rect width="10000" height="2000" fill="black"/></svg>"#;
        let image = load_generated_preview_image_from_svg_for_editor(
            svg,
            markdown::BlockPreviewKind::Mermaid,
        )
        .expect("large Mermaid SVG should rasterize");
        let size = image.size(0);
        let width = size.width.0 as u64;
        let height = size.height.0 as u64;

        assert!(width <= 2048);
        assert!(height <= 2048);
        assert!(width * height <= 2_097_152);
    }

    #[test]
    fn folded_table_mapping_keeps_cell_ends_before_synthetic_padding() {
        let source = "| Header 1 | Header 2 |";
        let decoration =
            markdown::parse_decorations(&format!("{source}\n| --- | --- |\n| A | B |"));
        let info = decoration[0].table_info.as_ref().expect("table header");
        let (_display, _d2b, b2d, _) =
            build_table_display_text(source, info, &[], false, None, 1, 0, 0, 0, 0);

        let first_end = info.cell_ranges[0].end;
        let second_start = info.cell_ranges[1].start;
        assert_eq!(b2d[first_end], 9);
        assert_eq!(b2d[first_end + 1], 10);
        assert_eq!(b2d[second_start], 11);
    }

    #[test]
    fn quote_lines_hide_markers_in_display_text() {
        let text = "> a\n>\n> - item one\n> - item two\n>\n> 1. ordered\n> b";
        let decorations = markdown::parse_decorations(text);
        for (line_content, decoration) in text.lines().zip(decorations.iter()) {
            let (display_text, ..) = build_display_text(
                line_content,
                decoration,
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
            assert!(
                !display_text.contains('>'),
                "line {line_content:?} rendered {display_text:?}"
            );
        }
    }

    #[test]
    fn quoted_lists_render_like_unquoted_ones() {
        for (quoted, plain) in [
            ("> - item", "- item"),
            ("> 1. item", "1. item"),
            ("> - [ ] todo", "- [ ] todo"),
        ] {
            let render = |line: &str| {
                let decorations = markdown::parse_decorations(&format!("{line}\n"));
                let (display_text, _, buffer_to_display, prefix_collapsed) = build_display_text(
                    line,
                    &decorations[0],
                    &[],
                    false,
                    None,
                    1,
                    2,
                    2,
                    2,
                    1,
                    0.7,
                    0.7,
                    0.7,
                );
                let marker_display_index = buffer_to_display
                    .get(super::list_marker_indent(line))
                    .copied()
                    .unwrap_or(0);
                (display_text, prefix_collapsed, marker_display_index)
            };

            assert_eq!(render(quoted), render(plain), "quoted {quoted:?}");
        }
    }

    #[test]
    fn anchored_popup_uses_gap_and_left_alignment_above() {
        let placement = anchored_popup_placement(
            800.0,
            600.0,
            Bounds::new(point(px(520.0), px(560.0)), size(px(100.0), px(24.0))),
            220.0,
            240.0,
            false,
            2.0,
            8.0,
        );

        assert_eq!(placement.left, 520.0);
        assert_eq!(placement.width, 220.0);
        assert_eq!(
            placement.vertical,
            AnchoredPopupVertical::Above { bottom: 42.0 }
        );
    }

    #[test]
    fn anchored_popup_left_aligns_with_flush_edge_anchor() {
        // Trigger buttons flush against the panel's left edge must get a
        // menu whose left edge lines up exactly (no margin inset).
        let placement = anchored_popup_placement(
            360.0,
            600.0,
            Bounds::new(point(px(0.0), px(560.0)), size(px(90.0), px(24.0))),
            220.0,
            240.0,
            false,
            2.0,
            8.0,
        );

        assert_eq!(placement.left, 0.0);
    }

    #[test]
    fn anchored_popup_right_aligns_when_right_space_is_insufficient() {
        let placement = anchored_popup_placement(
            800.0,
            600.0,
            Bounds::new(point(px(700.0), px(40.0)), size(px(80.0), px(24.0))),
            220.0,
            180.0,
            false,
            2.0,
            8.0,
        );

        assert_eq!(placement.left, 560.0);
        assert_eq!(placement.width, 220.0);
        assert_eq!(
            placement.vertical,
            AnchoredPopupVertical::Below { top: 66.0 }
        );
    }

    #[test]
    fn anchored_popup_can_match_anchor_width() {
        let placement = anchored_popup_placement(
            800.0,
            600.0,
            Bounds::new(point(px(300.0), px(200.0)), size(px(180.0), px(30.0))),
            240.0,
            120.0,
            true,
            2.0,
            8.0,
        );

        assert_eq!(placement.left, 300.0);
        assert_eq!(placement.width, 180.0);
    }

    fn generated_preview_fonts() -> GeneratedPreviewFontFamilies<'static> {
        GeneratedPreviewFontFamilies {
            monospace: "Menlo",
            body_cjk: "Menlo",
            mermaid_theme: MermaidTheme::Auto,
        }
    }

    fn folded_display_and_runs(line: &str, theme: &Theme) -> (String, Vec<TextRun>) {
        folded_display_and_runs_with_body_cjk(line, theme, ".SystemUIFont")
    }

    fn folded_display_and_runs_with_body_cjk(
        line: &str,
        theme: &Theme,
        body_cjk: &'static str,
    ) -> (String, Vec<TextRun>) {
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges: [Range<usize>; 0] = [];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(body_cjk),
            &font("Test Mono"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );
        (display_text, runs)
    }

    #[test]
    fn mixed_latin_cjk_text_keeps_script_specific_font_runs() {
        let theme = Theme::one_dark();
        let (display_text, runs) =
            folded_display_and_runs_with_body_cjk("only english中", &theme, "Test CJK");

        assert_eq!(
            run_at_byte(&runs, display_text.find('o').unwrap())
                .font
                .family
                .as_ref(),
            ".SystemUIFont",
        );
        assert_eq!(
            run_at_byte(&runs, display_text.find('中').unwrap())
                .font
                .family
                .as_ref(),
            "Test CJK",
        );
    }

    fn run_at_byte(runs: &[TextRun], byte_offset: usize) -> &TextRun {
        let mut start = 0usize;
        runs.iter()
            .find(|run| {
                let contains = byte_offset >= start && byte_offset < start + run.len;
                start += run.len;
                contains
            })
            .expect("text run at byte offset")
    }

    #[test]
    fn regular_prefix_hides_at_trailing_edge() {
        assert!(prefix_is_folded(0, 2, &[Range { start: 2, end: 2 }]));
    }

    #[test]
    fn wrap_segments_keep_url_slash_at_continuation_start() {
        let text = "http://example.com/path";
        let boundary = text.find('/').expect("first slash");

        let ranges = wrap_segment_byte_ranges(text.len(), &[boundary]);

        assert_eq!(ranges, vec![0..5, 5..text.len()]);
        assert_eq!(&text[ranges[1].clone()][..1], "/");
    }

    #[test]
    fn wrap_segments_ignore_duplicate_and_out_of_bounds_boundaries() {
        assert_eq!(wrap_segment_byte_ranges(8, &[3, 3, 99]), vec![0..3, 3..8]);
    }

    #[test]
    fn task_prefix_stays_visible_without_trailing_space() {
        assert!(!task_prefix_is_folded(
            "- [ ]",
            0,
            5,
            &[Range { start: 5, end: 5 }],
        ));
    }

    #[test]
    fn task_prefix_stays_visible_before_non_space_suffix() {
        assert!(!task_prefix_is_folded(
            "- [ ]a",
            0,
            5,
            &[Range { start: 5, end: 5 }],
        ));
    }

    #[test]
    fn task_prefix_hides_when_trailing_space_exists() {
        assert!(task_prefix_is_folded(
            "- [ ] ",
            0,
            5,
            &[Range { start: 5, end: 5 }],
        ));
    }

    #[test]
    fn task_prefix_hides_after_content_gap() {
        assert!(task_prefix_is_folded(
            "- [ ] text",
            0,
            5,
            &[Range { start: 6, end: 6 }],
        ));
    }

    #[test]
    fn nested_task_display_absorbs_parent_prefix_width() {
        let text = "- [ ] parent\n  - [ ] child";
        let line = "  - [ ] child";
        let decoration = markdown::parse_decorations(text)
            .into_iter()
            .nth(1)
            .unwrap_or_default();
        let collapsed_task_prefix_chars = 3;
        let (display_text, _, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &[Range {
                start: line.len(),
                end: line.len(),
            }],
            false,
            None,
            0,
            collapsed_task_prefix_chars,
            collapsed_task_prefix_chars,
            2,
            0,
            1.0,
            1.0,
            1.0,
        );

        assert!(prefix_collapsed);
        assert_eq!(
            compute_display_prefix_bytes(
                &display_text,
                &decoration.block_type,
                prefix_collapsed,
                collapsed_task_prefix_chars,
            ),
            collapsed_task_prefix_chars * 2,
        );
    }

    #[test]
    fn collapsed_unordered_list_prefix_stops_before_body_text() {
        let line = "- item";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (_, _, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &[Range {
                start: line.len(),
                end: line.len(),
            }],
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

        assert!(prefix_collapsed);
        let display_text = "  item".to_string();
        assert_eq!(display_text, "  item");
        assert_eq!(
            compute_display_prefix_bytes(&display_text, &decoration.block_type, true, 3),
            2,
        );
    }

    #[test]
    fn heading_prefix_expands_when_cursor_is_in_body_text() {
        let line = "# title";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, _, _, _) = build_display_text(
            line,
            &decoration,
            &[Range { start: 3, end: 3 }],
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

        assert_eq!(display_text, line);
    }

    #[test]
    fn heading_prefix_expands_when_cursor_is_in_heading_marker() {
        let line = "# title";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, _, _, _) = build_display_text(
            line,
            &decoration,
            &[Range { start: 1, end: 1 }],
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

        assert_eq!(display_text, line);
    }

    #[test]
    fn heading_prefix_expands_when_selection_is_in_body_text() {
        let line = "# title";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, _, _, _) = build_display_text(
            line,
            &decoration,
            &[Range { start: 3, end: 5 }],
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

        assert_eq!(display_text, line);
    }

    #[test]
    fn heading_prefix_expands_after_marker_space() {
        let line = "# ";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, _, _, _) = build_display_text(
            line,
            &decoration,
            &[Range { start: 2, end: 2 }],
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

        assert_eq!(display_text, line);
    }

    #[test]
    fn active_heading_marker_uses_marker_text_color() {
        let line = "# title";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges = [Range { start: 1, end: 1 }];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let heading_marker_color = crate::hex_hsla!("#A0A7B6");
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            heading_marker_color,
            None,
            false,
        );

        assert_eq!(display_text, line);
        assert_eq!(
            runs.first().map(|run| run.color),
            Some(heading_marker_color)
        );
    }

    #[test]
    fn folded_ordered_list_marker_uses_list_marker_text_color() {
        let line = "1. item";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges: [Range<usize>; 0] = [];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let list_marker_color = crate::hex_hsla!("#E06C75");
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            list_marker_color,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert!(prefix_collapsed);
        assert!(display_text.starts_with("1. "));
        assert_eq!(runs.first().map(|run| run.color), Some(list_marker_color));
    }

    #[test]
    fn paren_ordered_list_marker_highlight_excludes_content() {
        // `1) ABC`: the marker highlight must stop after `1) ` and not bleed
        // into the first content character.
        let line = "1) ABC";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        assert_eq!(decoration.block_type, markdown::BlockType::OrderedList);
        let display_text = line;
        assert_eq!(
            compute_display_prefix_bytes(display_text, &decoration.block_type, true, 0),
            3
        );
        assert_eq!(
            compute_display_prefix_bytes(display_text, &decoration.block_type, false, 0),
            3
        );
    }

    #[test]
    fn folded_link_description_uses_bold_link_text() {
        let theme = Theme::one_dark();
        let (display_text, runs) = folded_display_and_runs("[title](https://example.com)", &theme);

        assert_eq!(display_text, "title");
        assert_eq!(runs.first().map(|run| run.color), Some(theme.link));
        assert_eq!(
            runs.first().map(|run| run.font.weight),
            Some(FontWeight::BOLD)
        );
    }

    #[test]
    fn folded_image_text_uses_plain_text_color() {
        let theme = Theme::one_dark();
        let (display_text, runs) = folded_display_and_runs("![alt](image.png)", &theme);

        assert_eq!(display_text, "alt");
        assert_eq!(runs.first().map(|run| run.color), Some(theme.text_primary));
        assert_eq!(
            runs.first().map(|run| run.font.weight),
            Some(FontWeight::NORMAL)
        );
    }

    #[test]
    fn folded_footnote_source_keeps_superscript_placeholder() {
        let theme = Theme::one_dark();
        let (display_text, runs) = folded_display_and_runs("See [^note].", &theme);

        assert!(!display_text.contains("note"));
        assert!(
            runs.iter()
                .any(|run| run.color == gpui::transparent_black())
        );
    }

    #[test]
    fn folded_footnote_target_keeps_label_on_baseline() {
        let theme = Theme::one_dark();
        let (display_text, runs) = folded_display_and_runs("[^note]: body", &theme);

        assert_eq!(display_text, "note: body");
        assert!(runs.iter().any(|run| run.color == theme.text_primary));
        assert!(
            runs.iter()
                .all(|run| run.color != gpui::transparent_black())
        );
    }

    #[test]
    fn generated_math_block_preview_renders_svg() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Math,
            r"\int_0^1 x^2 \, dx = \frac{1}{3}",
            &theme,
            generated_preview_fonts(),
        );
        let text_color = crate::theme::hsla_to_hex_rgba(theme.text_bright).to_lowercase();

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("LaTeX preview failed"));
        assert!(svg.to_lowercase().contains(&text_color));
    }

    #[test]
    fn inline_math_placeholder_expands_when_cursor_touches_formula() {
        let source = "$E=mc^2$";
        let mut decoration = markdown::parse_decorations(source).remove(0);
        decoration
            .display_replacements
            .push(markdown::DisplayReplacement {
                source_range: 1..7,
                display_text: "[math]".to_string(),
                trigger_range: 0..8,
            });

        let display = |active_ranges: &[Range<usize>]| {
            build_display_text(
                source,
                &decoration,
                active_ranges,
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
            )
            .0
        };

        assert_eq!(display(&[]), "[math]");
        for cursor in [0, 3, 7] {
            assert_eq!(display(&[cursor..cursor]), source);
        }
        assert_eq!(display(&[8..8]), "[math]");
    }

    #[test]
    fn folded_inline_math_has_no_inline_code_background() {
        let theme = Theme::one_dark();
        let (_, runs) = folded_display_and_runs("energy $E=mc^2$", &theme);

        assert!(runs.iter().all(|run| run.background_color.is_none()));
    }

    #[test]
    fn generated_complex_math_block_preview_renders_svg() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Math,
            r"\operatorname*{arg\,min}_{\theta \in \mathbb{R}^n}
\left(
\frac{1}{m}\sum_{i=1}^{m} \ell\bigl(f_{\theta}(x_i), y_i\bigr)
+ \lambda \lVert \theta \rVert_2^2
\right)",
            &theme,
            generated_preview_fonts(),
        );

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("LaTeX preview failed"));
    }

    #[test]
    fn generated_inference_rule_math_preview_renders_svg() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Math,
            r"\frac{t_1\in T}{\texttt{succ }t_1\in T}",
            &theme,
            generated_preview_fonts(),
        );

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("LaTeX preview failed"), "{svg}");
    }

    #[test]
    fn generated_math_preview_supports_basic_text_macro() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Math,
            r"x \in T \text{ when } x > 0",
            &theme,
            generated_preview_fonts(),
        );

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("LaTeX preview failed"), "{svg}");
    }

    #[test]
    fn fullscreen_mermaid_raster_stays_within_texture_budget() {
        for width in [10_000, 100_000] {
            let svg = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"2000\" viewBox=\"0 0 {width} 2000\"><rect width=\"{width}\" height=\"2000\" fill=\"black\"/></svg>"
            );
            let image = super::load_generated_preview_overlay_image_from_svg(
                &svg,
                markdown::BlockPreviewKind::Mermaid,
                2.0,
            )
            .unwrap();
            let size = image.size(0);
            assert!(size.width.0 <= 4096);
            assert!(size.width.0 as u64 * size.height.0 as u64 <= 8_388_608);
            assert!(
                size.width.0 > 2048,
                "fullscreen should be sharper than inline"
            );
        }
    }

    #[test]
    fn generated_math_block_preview_is_one_and_a_half_times_the_previous_size() {
        let svg = super::render_math_block_preview_svg("E=mc^2", &Theme::one_dark()).unwrap();
        let old_svg = super::render_mathjax_preview_svg(
            "E=mc^2",
            &super::MathJaxOptions {
                font_size: 36.0,
                horizontal_align: super::HorizontalAlign::Center,
            },
        )
        .unwrap();
        let options = usvg::Options::default();
        let size = usvg::Tree::from_str(&svg, &options).unwrap().size();
        let old_size = usvg::Tree::from_str(&old_svg, &options).unwrap().size();
        assert!(
            (size.width() / old_size.width() - 1.5).abs() < 0.05,
            "widths: {} vs {}",
            size.width(),
            old_size.width()
        );
        assert!(
            (size.height() / old_size.height() - 1.5).abs() < 0.05,
            "heights: {} vs {}",
            size.height(),
            old_size.height()
        );
    }

    #[test]
    fn generated_mermaid_block_preview_uses_builtin_dark_theme() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            "flowchart LR\n    A[Start] --> B{Decision}\n    B -->|Yes| C[OK]\n    B -->|No| D[Cancel]",
            &theme,
            generated_preview_fonts(),
        );
        let svg = svg.to_lowercase();

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("mermaid preview failed"));
        assert!(!svg.contains("<rect width=\"100%\" height=\"100%\" fill="));
        assert!(svg.contains("#e5e7eb"));
        assert!(svg.contains("#111827"));
        assert!(svg.contains("#94a3b8"));
        assert!(svg.contains("background-color:transparent"));
        assert!(svg.contains("font-size:14px"));
        assert!(!svg.contains("<foreignobject"));
    }

    #[test]
    fn generated_mermaid_block_preview_uses_builtin_light_theme() {
        let theme = Theme::one_light();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            "flowchart LR\n    A[Start] --> B{Decision}\n    B -->|Yes| C[OK]\n    B -->|No| D[Cancel]",
            &theme,
            generated_preview_fonts(),
        );
        let svg = svg.to_lowercase();

        assert!(!svg.contains("mermaid preview failed"));
        assert!(svg.contains("#0f172a"));
        assert!(svg.contains("#f8fafc"));
        assert!(svg.contains("#64748b"));
        assert!(svg.contains("background-color:transparent"));
        assert!(!svg.contains("<foreignobject"));
    }

    #[test]
    fn generated_mermaid_dark_host_theme_wins_over_embedded_light_theme() {
        for source in [
            "%%{init: {'theme': 'default'}}%%\nflowchart LR\nA[Start] --> B[End]",
            "sequenceDiagram\nAlice->>Bob: Hello\nNote right of Bob: Reply",
            "classDiagram\nAnimal <|-- Duck\nAnimal: +name",
        ] {
            let svg = build_generated_block_preview_svg(
                markdown::BlockPreviewKind::Mermaid,
                source,
                &Theme::one_dark(),
                generated_preview_fonts(),
            );
            assert!(!svg.contains("Mermaid preview failed"), "{svg}");
            assert!(svg.contains("#e5e7eb"), "{svg}");
            load_generated_preview_image_from_svg(&svg, markdown::BlockPreviewKind::Mermaid)
                .unwrap();
        }
    }

    #[test]
    fn generated_mermaid_dark_surfaces_are_darker_in_raster_output() {
        let average = |theme: &Theme, source: &str| {
            let svg = build_generated_block_preview_svg(
                markdown::BlockPreviewKind::Mermaid,
                source,
                theme,
                generated_preview_fonts(),
            );
            let mut options = usvg::Options::default();
            options.fontdb = super::generated_preview_fontdb().clone();
            let tree = usvg::Tree::from_str(&svg, &options).unwrap();
            let size = tree.size().to_int_size();
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pixmap.as_mut(),
            );
            let solid = pixmap
                .pixels()
                .iter()
                .filter(|pixel| pixel.alpha() > 240)
                .collect::<Vec<_>>();
            assert!(!solid.is_empty());
            solid
                .iter()
                .map(|pixel| {
                    (pixel.red() as f32 + pixel.green() as f32 + pixel.blue() as f32) / 3.0
                })
                .sum::<f32>()
                / solid.len() as f32
        };
        for source in [
            "flowchart LR\nA[Start] --> B[Finish]",
            "%%{init: {'theme': 'default'}}%%\nflowchart LR\nA[Start] --> B[Finish]",
        ] {
            let dark = average(&Theme::one_dark(), source);
            let light = average(&Theme::one_light(), source);
            assert!(dark < 100.0 && light > 150.0, "dark={dark}, light={light}");
        }
    }

    #[test]
    fn generated_mermaid_all_theme_options_rasterize() {
        let editor_theme = Theme::one_dark();
        for mermaid_theme in MermaidTheme::ALL {
            let svg = build_generated_block_preview_svg(
                markdown::BlockPreviewKind::Mermaid,
                "flowchart LR\n    A[Source] --> B{Valid?}\n    B -->|Yes| C[Render]\n    B -->|No| D[Error]",
                &editor_theme,
                GeneratedPreviewFontFamilies {
                    monospace: "Menlo",
                    body_cjk: "PingFang SC",
                    mermaid_theme,
                },
            );
            assert!(
                !svg.contains("Mermaid preview failed"),
                "{} failed to render",
                mermaid_theme.label()
            );

            load_generated_preview_image_from_svg(&svg, markdown::BlockPreviewKind::Mermaid)
                .unwrap_or_else(|error| {
                    panic!("{} failed to rasterize: {error}", mermaid_theme.label())
                });
        }
    }

    #[test]
    fn generated_mermaid_sequence_text_is_larger_without_changing_other_diagrams() {
        let sequence_source =
            "%% comment\nsequenceDiagram\nAlice->>Bob: Hello\nNote right of Bob: Reply";
        assert!(matches!(
            merman::detect::DetectorRegistry::new()
                .detect_type(sequence_source, &mut merman::MermaidConfig::default()),
            Ok("sequence")
        ));
        for (theme, mermaid_theme) in [
            (Theme::one_dark(), MermaidTheme::Auto),
            (Theme::one_light(), MermaidTheme::Auto),
            (Theme::one_dark(), MermaidTheme::MermaidDefault),
        ] {
            let font_families = GeneratedPreviewFontFamilies {
                mermaid_theme,
                ..generated_preview_fonts()
            };
            let flowchart = super::render_mermaid_block_preview_svg(
                "flowchart LR\nA[Start] --> B[Finish]",
                &theme,
                font_families,
            )
            .unwrap();
            let sequence =
                super::render_mermaid_block_preview_svg(sequence_source, &theme, font_families)
                    .unwrap();
            assert!(
                flowchart.contains(if mermaid_theme == MermaidTheme::Auto {
                    "font-size:14px;fill:"
                } else {
                    "font-size:16px;fill:"
                }),
                "{mermaid_theme:?}"
            );
            assert!(
                sequence.contains("font-size:18px;font-weight:400\">Hello"),
                "{mermaid_theme:?}"
            );
            let with_directive = super::render_mermaid_block_preview_svg(
                &format!("%%{{init: {{'theme': 'default'}}}}%%\n{sequence_source}"),
                &theme,
                font_families,
            )
            .unwrap();
            assert!(
                with_directive.contains("font-size:18px;font-weight:400\">Hello"),
                "{mermaid_theme:?}"
            );
            super::load_generated_preview_image_from_svg(
                &sequence,
                markdown::BlockPreviewKind::Mermaid,
            )
            .unwrap();
        }
    }

    #[test]
    fn generated_math_block_preview_keeps_inline_background_transparent() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Math,
            r"\int_0^1 x^2 \, dx = \frac{1}{3}",
            &theme,
            generated_preview_fonts(),
        );

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("LaTeX preview failed"));
        assert!(svg.contains("background-color:transparent"));
        assert!(
            !svg.to_lowercase()
                .contains("<rect width=\"100%\" height=\"100%\" fill=")
        );
    }

    #[test]
    fn generated_mermaid_block_preview_preserves_cjk_text_without_panicking() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            "sequenceDiagram\n    participant U as 用户\n    participant E as 编辑器\n    U->>E: 编辑 Mermaid 或公式块\n    E-->>U: 立即更新预览",
            &theme,
            generated_preview_fonts(),
        );

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("Mermaid preview failed"));
        assert!(svg.contains("编辑 Mermaid 或公式块"));
        assert!(svg.contains("立即更新预览"));
    }

    #[test]
    fn generated_mermaid_block_preview_does_not_need_ascii_token_substitutions() {
        let theme = Theme::one_dark();
        let mut occupied_tokens = String::new();

        for first in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789".chars() {
            for second in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789".chars() {
                occupied_tokens.push(first);
                occupied_tokens.push(second);
                occupied_tokens.push(' ');
            }
        }

        let source = format!(
            "%% {occupied_tokens}\nsequenceDiagram\n    participant U as 用户\n    U->>U: 不再需要 ASCII token 替换"
        );
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            &source,
            &theme,
            generated_preview_fonts(),
        );

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("Mermaid preview failed"));
        assert!(svg.contains("不再需要 ASCII token 替换"));
    }

    #[test]
    fn generated_mermaid_block_preview_handles_complex_utf8_flowchart() {
        let theme = Theme::one_dark();
        let source = r#"flowchart TD
  A["商品页加载<br/>content script document_start 注入"] --> B["登录态检查<br/>chrome.storage.local.pluginAuth"]
  B -->|未登录| B0["不进入商品分析<br/>只等登录态变化"]
  B -->|已登录| C["白名单门控<br/>focus + host 白名单 + Shopify 自动发现"]
  C -->|未命中| C0["静默退出<br/>页面完全不显示插件"]
  C -->|命中| D["挂载 Shadow DOM React Root<br/>但 pluginActive 仍可为 false"]
  D --> E["商品主管线准备<br/>XPath / Shopify JSON / URL PDP / 截图 OCR"]
  E -->|非 PDP| E0["不显示插件"]
  E -->|确认 PDP + 拿到商品| F["暂存 preparedAnalysis<br/>小女孩进入正常态，等待点击"]
  F --> G["用户点击小女孩"]
  G --> H["POST /api/plugin-ai/analyze<br/>带 pageData + extractedProduct + engine"]
  H --> I{"engine?"}
  I -->|server| J["服务端研究 + Tavily + 平台模型最终成稿"]
  J --> K["SSE: analysis_report_start/component/done"]
  I -->|user_ai| L["服务端只做品类 + 模板 + web prompt 备料"]
  L --> M["SSE: bridge_prompt + bridgeHandoff done"]
  M --> N["插件 Bridge Client 接管"]
  N --> O["Bridge Tab 驱动 ChatGPT/Gemini iframe"]
  O --> P["用户 AI 输出 JSONL 原文"]
  P --> Q["POST /analyze/bridge-complete"]
  Q --> R{"服务端验收通过?"}
  R -->|通过| S["返回同构 events 数组"]
  R -->|未 done 且有额度| O2["BRIDGE_CONTINUE 续写"]
  O2 --> P
  R -->|失败/额度尽| T["POST /analyze/bridge-fallback<br/>服务端兜底成稿"]
  T --> K
  S --> U["逐条喂 mapAnalyzeSseEvent"]
  K --> U
  U --> V["analysis-store streamingReport"]
  V --> W["SidePanel 渲染最终组件报告"]"#;
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            source,
            &theme,
            generated_preview_fonts(),
        );

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("Mermaid preview failed"));
        assert!(!svg.contains(" C "));
        for bare_label in ["C", "D", "F", "S", "T"] {
            assert!(!svg.contains(&format!(">{bare_label}</text>")));
        }
        for expected in [
            "白名单门控",
            "挂载",
            "Shadow",
            "DOM",
            "React",
            "Root",
            "暂存",
            "preparedAnalysis",
            "返回同构",
            "events",
            "数组",
            "POST",
            "analyze",
            "bridge",
            "fallback",
        ] {
            assert!(
                svg.contains(expected),
                "missing Mermaid fragment: {expected}"
            );
        }
    }

    #[test]
    fn generated_mermaid_block_preview_rasterizes_emoji_and_cjk() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            "flowchart LR\n    A[\"✅ Ready 😀\"] --> B[\"🚀 发布\"]",
            &theme,
            generated_preview_fonts(),
        );

        assert!(!svg.contains("Mermaid preview failed"));
        for expected in ["✅", "😀", "🚀", "发布"] {
            assert!(svg.contains(expected), "missing Mermaid label: {expected}");
        }
        load_generated_preview_image_from_svg(&svg, markdown::BlockPreviewKind::Mermaid)
            .unwrap_or_else(|error| panic!("failed to rasterize Mermaid SVG: {error}"));
    }

    #[test]
    fn generated_mermaid_common_diagram_families_rasterize() {
        let theme = Theme::one_dark();
        let cases = [
            (
                "sequence",
                "sequenceDiagram\n    participant U as 用户\n    U->>E: 打开编辑器 ✅\n    E-->>U: 显示预览",
            ),
            (
                "class",
                "classDiagram\n    Animal <|-- Dog\n    class Animal {\n        +String name\n    }",
            ),
            (
                "state",
                "stateDiagram-v2\n    [*] --> Editing\n    Editing --> Preview\n    Preview --> [*]",
            ),
            ("er", "erDiagram\n    USER ||--o{ NOTE : owns"),
            (
                "gantt",
                "gantt\n    title Release\n    dateFormat YYYY-MM-DD\n    section Build\n    Compile :done, a1, 2026-08-01, 1d",
            ),
            (
                "pie",
                "pie showData\n    title Usage\n    \"Edit\" : 70\n    \"Preview\" : 30",
            ),
            (
                "mindmap",
                "mindmap\n  root((Moeka))\n    Editor\n    Preview",
            ),
        ];

        for (name, source) in cases {
            let svg = build_generated_block_preview_svg(
                markdown::BlockPreviewKind::Mermaid,
                source,
                &theme,
                generated_preview_fonts(),
            );
            assert!(
                !svg.contains("Mermaid preview failed"),
                "{name} failed to render"
            );
            load_generated_preview_image_from_svg(&svg, markdown::BlockPreviewKind::Mermaid)
                .unwrap_or_else(|error| panic!("{name} failed to rasterize: {error}"));
        }
    }

    #[test]
    fn generated_mermaid_help_examples_all_rasterize() {
        let help = include_str!("../../docs/help-zh-cn.md");
        let mut examples = Vec::new();
        let mut current = String::new();
        let mut inside_mermaid = false;

        for line in help.lines() {
            match (inside_mermaid, line.trim()) {
                (false, "```mermaid") => inside_mermaid = true,
                (true, "```") => {
                    examples.push(std::mem::take(&mut current));
                    inside_mermaid = false;
                }
                (true, _) => {
                    current.push_str(line);
                    current.push('\n');
                }
                _ => {}
            }
        }

        assert!(!inside_mermaid, "unclosed Mermaid fence in help-zh-cn.md");
        assert!(!examples.is_empty(), "no Mermaid examples found");

        let theme = Theme::one_dark();
        for (index, source) in examples.iter().enumerate() {
            let diagram_type = source.lines().next().unwrap_or("unknown");
            let svg = build_generated_block_preview_svg(
                markdown::BlockPreviewKind::Mermaid,
                source,
                &theme,
                generated_preview_fonts(),
            );
            assert!(
                !svg.contains("Mermaid preview failed"),
                "example {} ({diagram_type}) failed to render",
                index + 1
            );
            load_generated_preview_image_from_svg(&svg, markdown::BlockPreviewKind::Mermaid)
                .unwrap_or_else(|error| {
                    panic!(
                        "example {} ({diagram_type}) failed to rasterize: {error}",
                        index + 1
                    )
                });
        }
    }

    #[test]
    fn generated_mermaid_block_preview_uses_configured_code_font_with_cjk_fallback() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            "flowchart LR\n    A[代码] --> B[预览]",
            &theme,
            GeneratedPreviewFontFamilies {
                monospace: "Fira Code",
                body_cjk: "PingFang SC",
                mermaid_theme: MermaidTheme::Auto,
            },
        );

        assert!(svg.contains("Fira Code"));
        assert!(svg.contains("PingFang SC"));
        assert!(svg.contains("ui-monospace"));
    }

    #[test]
    fn generated_mermaid_block_preview_sanitizes_system_font_sentinel() {
        let theme = Theme::one_dark();
        let svg = build_generated_block_preview_svg(
            markdown::BlockPreviewKind::Mermaid,
            "flowchart LR\n    A[中文] --> B[System]",
            &theme,
            GeneratedPreviewFontFamilies {
                monospace: ".SystemUIFont",
                body_cjk: ".SystemUIFont",
                mermaid_theme: MermaidTheme::Auto,
            },
        );

        assert!(!svg.contains(".SystemUIFont"));
        assert!(svg.contains("ui-monospace"));
        assert!(svg.contains("PingFang SC"));
    }

    #[test]
    fn compute_generated_preview_size_preserves_intrinsic_width_when_possible() {
        assert_eq!(
            compute_generated_preview_size(640.0, 320.0, 800.0),
            (320.0, 160.0)
        );
        assert_eq!(
            compute_generated_preview_size(1280.0, 640.0, 320.0),
            (320.0, 160.0)
        );
    }

    #[test]
    fn mermaid_preview_height_follows_intrinsic_aspect_ratio() {
        let (wide_w, wide_h, _, wide_background_h) = fit_generated_preview_size(
            1200.0,
            300.0,
            markdown::BlockPreviewKind::Mermaid,
            600.0,
            50.0,
            300.0,
            0.0,
            0.0,
        );
        let (tall_w, tall_h, _, tall_background_h) = fit_generated_preview_size(
            300.0,
            1200.0,
            markdown::BlockPreviewKind::Mermaid,
            600.0,
            50.0,
            300.0,
            0.0,
            0.0,
        );
        assert_eq!(wide_w / wide_h, 4.0);
        assert_eq!(tall_h / tall_w, 4.0);
        assert_eq!(wide_background_h, wide_h);
        assert_eq!(tall_background_h, tall_h);
        assert!(tall_background_h > 300.0);
    }

    #[test]
    fn math_preview_height_follows_intrinsic_aspect_ratio() {
        let (width, height, _, background_height) = fit_generated_preview_size(
            300.0,
            1200.0,
            markdown::BlockPreviewKind::Math,
            300.0,
            50.0,
            300.0,
            0.0,
            8.0,
        );
        assert_eq!(height / width, 4.0);
        assert_eq!(background_height, height + 16.0);
        assert!(background_height > 300.0);
    }

    #[test]
    fn generated_previews_use_rendered_size_without_upscaling() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40" viewBox="0 0 120 40"><rect width="120" height="40" fill="black"/></svg>"#;
        for kind in [
            markdown::BlockPreviewKind::Mermaid,
            markdown::BlockPreviewKind::Math,
        ] {
            let image = load_generated_preview_image_from_svg_for_editor(svg, kind).unwrap();
            let raster = image.size(0);
            let (width, height, background_width, background_height) = fit_generated_preview_size(
                raster.width.0 as f32,
                raster.height.0 as f32,
                kind,
                800.0,
                50.0,
                300.0,
                12.0,
                8.0,
            );
            assert_eq!((width, height), (120.0, 40.0));
            assert_eq!((background_width, background_height), (144.0, 56.0));

            let (width, height, background_width, background_height) = fit_generated_preview_size(
                raster.width.0 as f32,
                raster.height.0 as f32,
                kind,
                84.0,
                50.0,
                300.0,
                12.0,
                8.0,
            );
            assert_eq!((width, height), (60.0, 20.0));
            assert_eq!((background_width, background_height), (84.0, 36.0));
        }
    }

    #[test]
    fn source_markdown_shaping_matches_fully_expanded_markdown_styles() {
        let theme = Theme::one_dark();
        for line in [
            "plain **bold** *italic* ~~gone~~ ==mark== `code` [link](note.md)",
            "# title **bold**",
            "- **bold** and `code`",
            "1. **bold** and *italic*",
            "- [x] **bold**",
            "> quote *italic*",
        ] {
            let canonical = markdown::parse_decorations(line).remove(0);
            let mut projected = canonical.clone();
            super::source_markdown_decoration(&mut projected);
            let mut source_active = vec![2..2];
            super::expand_source_syntax(true, line.len(), &mut source_active);
            assert_eq!(source_active, vec![0..line.len()]);
            let shape = |decoration: &LineDecoration, active: &[Range<usize>]| {
                let (display, d2b, b2d, collapsed) = build_display_text(
                    line, decoration, active, false, None, 0, 3, 3, 2, 0, 0.7, 0.7, 0.7,
                );
                assert_eq!(display, line);
                assert_eq!(d2b, (0..line.len()).collect::<Vec<_>>());
                assert_eq!(b2d, (0..=line.len()).collect::<Vec<_>>());
                assert!(!collapsed);
                build_text_runs_for_display(
                    &display,
                    line,
                    &d2b,
                    decoration,
                    active,
                    false,
                    collapsed,
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
                    theme.git_status_added,
                    theme.text_danger,
                    theme.link,
                    FontWeight::NORMAL,
                    theme.list_marker,
                    FontWeight::BOLD,
                    theme.syntax_marker,
                    None,
                    false,
                )
            };
            let source_runs = shape(&projected, &source_active);
            let selected_runs = shape(&canonical, &[0..line.len()]);
            assert_eq!(source_runs.len(), selected_runs.len());
            for (source, selected) in source_runs.iter().zip(&selected_runs) {
                assert_eq!(source.len, selected.len);
                assert_eq!(source.font, selected.font);
                assert_eq!(source.color, selected.color);
                assert_eq!(source.background_color, selected.background_color);
                assert_eq!(source.underline, selected.underline);
                assert_eq!(source.strikethrough, selected.strikethrough);
            }
            if line.starts_with("plain") {
                assert_eq!(
                    run_at_byte(&source_runs, 0).font.family.as_ref(),
                    "Test Body"
                );
                assert_eq!(
                    run_at_byte(&source_runs, line.find("bold").unwrap())
                        .font
                        .weight,
                    FontWeight::EXTRA_BOLD
                );
                assert_eq!(
                    run_at_byte(&source_runs, line.find("italic").unwrap())
                        .font
                        .family
                        .as_ref(),
                    "Test Italic"
                );
                assert_eq!(
                    run_at_byte(&source_runs, line.find("code").unwrap())
                        .font
                        .family
                        .as_ref(),
                    "Test Mono"
                );
            } else if line.starts_with('#') {
                assert_eq!(
                    run_at_byte(&source_runs, line.find("title").unwrap())
                        .font
                        .family
                        .as_ref(),
                    "Test Heading"
                );
            }
        }
        let mut unchanged = vec![2..2];
        super::expand_source_syntax(false, 10, &mut unchanged);
        assert_eq!(unchanged, vec![2..2]);
    }

    fn source_syntax_runs(
        line: &str,
        canonical: &LineDecoration,
        theme: &Theme,
    ) -> (LineDecoration, Vec<TextRun>) {
        let mut projected = canonical.clone();
        super::source_markdown_decoration(&mut projected);
        assert_eq!(projected.block_type, markdown::BlockType::Normal);
        assert!(projected.block_preview.is_none());
        assert!(projected.table_info.is_none());
        assert!(projected.front_matter.is_none());
        assert!(projected.inline_spans.is_empty());
        assert!(projected.inline_math_spans.is_empty());
        assert!(projected.syntax_ranges.is_empty());
        let mut active = Vec::new();
        super::expand_source_syntax(true, line.len(), &mut active);
        let (display, d2b, b2d, collapsed) = build_display_text(
            line, &projected, &active, false, None, 0, 3, 3, 2, 0, 0.7, 0.7, 0.7,
        );
        assert_eq!(display, line);
        assert_eq!(d2b, (0..line.len()).collect::<Vec<_>>());
        assert_eq!(b2d, (0..=line.len()).collect::<Vec<_>>());
        assert!(!collapsed);
        let runs = build_text_runs_for_display(
            &display,
            line,
            &d2b,
            &projected,
            &active,
            false,
            collapsed,
            &font("Test Body"),
            &font("Test CJK"),
            &font("Test Mono"),
            &font("Test Heading"),
            &font("Test Heading CJK"),
            &font("Test Italic"),
            &font("Test Italic CJK"),
            3,
            16.0,
            theme,
            theme.git_status_added,
            theme.text_danger,
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );
        if !line.is_empty() {
            assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), line.len());
        }
        assert!(runs.iter().all(|run| run.background_color.is_none()));
        (projected, runs)
    }

    #[test]
    fn source_table_syntax_reaches_text_runs_without_rich_ui_or_hidden_bytes() {
        for theme in [
            Theme::one_dark(),
            Theme::one_light(),
            Theme::moeka_print_light(),
        ] {
            for source in [
                "| **bold** | *italic* | `code` | [link](note.md) |\n| :--- | :---: | ---: | --- |\n| 中🙂 \\| literal | `a|b` | [[Page|alias]] | a<br>b |",
                "**bold** | *italic* | `code` | [link](note.md)\n:--- | :---: | ---: | ---\n中🙂 | normal | plain | a<br>b",
            ] {
                let canonical = markdown::parse_decorations(source);
                let saved = canonical.clone();
                for (index, line) in source.lines().enumerate() {
                    assert_eq!(canonical[index].block_type, markdown::BlockType::Table);
                    let (projected, runs) = source_syntax_runs(line, &canonical[index], &theme);
                    assert_eq!(projected.code_language.as_deref(), Some("markdown"));
                    assert!(projected.display_replacements.is_empty());
                    for offset in markdown::table_pipe_offsets(line) {
                        assert_eq!(
                            run_at_byte(&runs, offset).color,
                            gpui::rgb(theme.code_palette.operator).into(),
                            "{line:?}"
                        );
                    }
                    if index == 1 {
                        for (offset, ch) in line.char_indices() {
                            if matches!(ch, '-' | ':') {
                                assert_eq!(
                                    run_at_byte(&runs, offset).color,
                                    gpui::rgb(theme.code_palette.keyword).into(),
                                    "{line:?}"
                                );
                            }
                        }
                    }
                    for (token, color) in [
                        ("bold", theme.code_palette.number),
                        ("italic", theme.code_palette.keyword),
                        ("code", theme.code_palette.string),
                        ("note.md", theme.code_palette.function),
                    ] {
                        if let Some(offset) = line.find(token) {
                            assert_eq!(
                                run_at_byte(&runs, offset).color,
                                gpui::rgb(color).into(),
                                "{line:?}: {token}"
                            );
                        }
                    }
                    if let Some(offset) = line.find("bold") {
                        assert_eq!(run_at_byte(&runs, offset).font.weight, FontWeight::BOLD);
                    }
                }
                assert_eq!(canonical, saved);
            }
        }
    }

    #[test]
    fn source_mermaid_flowchart_and_sequence_tokens_reach_text_runs() {
        for theme in [Theme::one_dark(), Theme::one_light()] {
            let p = theme.code_palette;
            let cases = [
                (
                    "flowchart LR",
                    vec![("flowchart", p.keyword), ("LR", p.number)],
                ),
                (
                    "my-node.v2[中🙂 end 42] -->|yes end| other-node{Done?}",
                    vec![
                        ("my-node.v2", p.variable),
                        ("中🙂 end 42", p.string),
                        ("-->", p.operator),
                        ("yes end", p.string),
                        ("other-node", p.variable),
                        ("Done?", p.string),
                    ],
                ),
                (
                    "A-.->B==>C<-->D~~~E",
                    vec![
                        ("A", p.variable),
                        ("-.->", p.operator),
                        ("B", p.variable),
                        ("==>", p.operator),
                        ("C", p.variable),
                        ("<-->", p.operator),
                        ("D", p.variable),
                        ("~~~", p.operator),
                        ("E", p.variable),
                    ],
                ),
                (
                    "A -- end label --> B",
                    vec![
                        ("--", p.operator),
                        ("end label", p.string),
                        ("B", p.variable),
                    ],
                ),
                (
                    "A -. dotted text .-> B",
                    vec![
                        ("dotted text", p.string),
                        (".->", p.operator),
                        ("B", p.variable),
                    ],
                ),
                (
                    "A == thick text ==> B",
                    vec![
                        ("thick text", p.string),
                        ("==>", p.operator),
                        ("B", p.variable),
                    ],
                ),
                (
                    "A o--o B & C x--x D",
                    vec![
                        ("o--o", p.operator),
                        ("B", p.variable),
                        ("x--x", p.operator),
                        ("D", p.variable),
                    ],
                ),
                (
                    "x-->y",
                    vec![("x", p.variable), ("-->", p.operator), ("y", p.variable)],
                ),
                (
                    "开始[Don't stop]-->结束((完成🙂))",
                    vec![
                        ("开始", p.variable),
                        ("Don't stop", p.string),
                        ("-->", p.operator),
                        ("结束", p.variable),
                        ("完成🙂", p.string),
                    ],
                ),
                (
                    r#"A["say \"end\" %% --> 中"] --> B"#,
                    vec![(r#"say \"end\" %% --> 中"#, p.string), ("B", p.variable)],
                ),
                (
                    "A[unfinished 中🙂",
                    vec![("A", p.variable), ("unfinished 中🙂", p.string)],
                ),
                (
                    "A-->B %% comment 中🙂",
                    vec![
                        ("A", p.variable),
                        ("-->", p.operator),
                        ("B", p.variable),
                        ("%% comment 中🙂", p.comment),
                    ],
                ),
                ("sequenceDiagram", vec![("sequenceDiagram", p.keyword)]),
                (
                    "participant Alice as Client 中🙂",
                    vec![
                        ("participant", p.keyword),
                        ("Alice", p.variable),
                        ("as", p.keyword),
                        ("Client 中🙂", p.string),
                    ],
                ),
                ("actor Bob", vec![("actor", p.keyword), ("Bob", p.variable)]),
                (
                    "Alice->>+Bob: Hello end 42 **中🙂**",
                    vec![
                        ("Alice", p.variable),
                        ("->>+", p.operator),
                        ("Bob", p.variable),
                        ("Hello end 42 **中🙂**", p.string),
                    ],
                ),
                (
                    "Bob-->>-Alice: Done!",
                    vec![
                        ("Bob", p.variable),
                        ("-->>-", p.operator),
                        ("Alice", p.variable),
                        ("Done!", p.string),
                    ],
                ),
                (
                    "Alice--xBob: failure",
                    vec![
                        ("--x", p.operator),
                        ("Bob", p.variable),
                        ("failure", p.string),
                    ],
                ),
                (
                    "Alice-)Bob: async",
                    vec![("-)", p.operator), ("Bob", p.variable), ("async", p.string)],
                ),
                (
                    "Note right of Alice,Bob: end --> 中🙂",
                    vec![
                        ("Note", p.keyword),
                        ("right", p.keyword),
                        ("Alice", p.variable),
                        ("Bob", p.variable),
                        ("end --> 中🙂", p.string),
                    ],
                ),
                (
                    "loop Every minute",
                    vec![("loop", p.keyword), ("Every minute", p.string)],
                ),
                (
                    "alt success",
                    vec![("alt", p.keyword), ("success", p.string)],
                ),
                (
                    "else failure",
                    vec![("else", p.keyword), ("failure", p.string)],
                ),
                ("end", vec![("end", p.keyword)]),
            ];
            for fence in ["```mermaid", "~~~mmd", "```Mermaid"] {
                let marker = &fence[..3];
                let source = format!(
                    "{fence}\n{}\n{marker}",
                    cases
                        .iter()
                        .map(|(line, _)| *line)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                let canonical = markdown::parse_decorations(&source);
                for (index, line) in source.lines().enumerate() {
                    let (_, runs) = source_syntax_runs(line, &canonical[index], &theme);
                    if index == 0 || index == cases.len() + 1 {
                        for offset in 0..3 {
                            assert_eq!(run_at_byte(&runs, offset).color, theme.syntax_marker);
                        }
                    } else {
                        for (token, color) in &cases[index - 1].1 {
                            let offset = line.rfind(token).unwrap();
                            for byte in offset..offset + token.len() {
                                assert_eq!(
                                    run_at_byte(&runs, byte).color,
                                    super::code_palette_hsla(*color),
                                    "{line:?}: {token}"
                                );
                            }
                        }
                    }
                    let mut offset = 0;
                    for run in &runs {
                        assert!(line.is_char_boundary(offset));
                        offset += run.len;
                        assert!(line.is_char_boundary(offset));
                        assert!(run.color.a > 0.0);
                    }
                }
            }
        }
    }

    #[test]
    fn source_highlighting_mermaid_slanted_shapes_keep_delimiters_and_following_syntax() {
        for theme in [Theme::one_dark(), Theme::one_light()] {
            for (open, close) in [("[/", "/]"), ("[\\", "\\]"), ("[/", "\\]"), ("[\\", "/]")] {
                for label in ["Input", "Input 中🙂", r#""say \"end\" \] 中🙂""#] {
                    let line = format!("A{open}{label}{close} --> B[Done]");
                    let source = format!("```mermaid\nflowchart TD\n{line}\n```");
                    let canonical = markdown::parse_decorations(&source);
                    let saved = canonical.clone();
                    let (_, runs) = source_syntax_runs(&line, &canonical[2], &theme);
                    for (offset, len, color) in [
                        (0, 1, theme.code_palette.variable),
                        (1, open.len(), theme.code_palette.operator),
                        (1 + open.len(), label.len(), theme.code_palette.string),
                        (
                            1 + open.len() + label.len(),
                            close.len(),
                            theme.code_palette.operator,
                        ),
                        (line.find("-->").unwrap(), 3, theme.code_palette.operator),
                        (line.rfind("B[").unwrap(), 1, theme.code_palette.variable),
                        (line.find("Done").unwrap(), 4, theme.code_palette.string),
                    ] {
                        for byte in offset..offset + len {
                            assert_eq!(
                                run_at_byte(&runs, byte).color,
                                super::code_palette_hsla(color),
                                "{line:?}: byte {byte}"
                            );
                        }
                    }
                    assert_eq!(run_at_byte(&runs, 0).font.family.as_ref(), "Test Body");
                    assert_eq!(canonical, saved);
                }
            }
            // An ordinary label's escaped bracket must not become a shape closer.
            let line = r#"A[escaped \] and \"quotes\" 中🙂] --> B[Done]"#;
            let source = format!("```mermaid\nflowchart TD\n{line}\n```");
            let canonical = markdown::parse_decorations(&source);
            let (_, runs) = source_syntax_runs(line, &canonical[2], &theme);
            for (token, color) in [
                (r#"\] and \"quotes\" 中🙂"#, theme.code_palette.string),
                ("-->", theme.code_palette.operator),
                ("B", theme.code_palette.variable),
            ] {
                let offset = line.find(token).unwrap();
                for byte in offset..offset + token.len() {
                    assert_eq!(
                        run_at_byte(&runs, byte).color,
                        super::code_palette_hsla(color),
                        "{line:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn source_mermaid_complete_common_diagrams_keep_canonical_languages_and_colors() {
        let theme = Theme::one_dark();
        let p = theme.code_palette;
        for (source, tokens) in [
            (
                "```mermaid\nflowchart TD\n    Start([开始🙂]) --> Gate{Ready?}\n    Gate -->|Yes| Finish[Done]\n    Gate -.->|No| Start\n```",
                vec![
                    ("flowchart", p.keyword),
                    ("Start", p.variable),
                    ("Gate", p.variable),
                    ("开始🙂", p.string),
                    ("Ready?", p.string),
                    ("Yes", p.string),
                    ("Done", p.string),
                    ("No", p.string),
                    ("-->", p.operator),
                    ("-.->", p.operator),
                ],
            ),
            (
                "~~~mermaid\nsequenceDiagram\n    participant Alice as Client\n    participant Bob\n    Alice->>+Bob: Hello 中🙂\n    Note over Alice,Bob: A friendly note\n    alt success\n        Bob-->>-Alice: OK\n    else retry\n        Alice-)Bob: Try again\n    end\n~~~",
                vec![
                    ("sequenceDiagram", p.keyword),
                    ("participant", p.keyword),
                    ("Alice", p.variable),
                    ("Bob", p.variable),
                    ("Client", p.string),
                    ("Hello 中🙂", p.string),
                    ("A friendly note", p.string),
                    ("Try again", p.string),
                    ("->>+", p.operator),
                    ("-->>-", p.operator),
                    ("-)", p.operator),
                ],
            ),
        ] {
            let canonical = markdown::parse_decorations(source);
            for (row, line) in source.lines().enumerate() {
                assert!(canonical[row].block_preview.is_some());
                let (projected, runs) = source_syntax_runs(line, &canonical[row], &theme);
                assert_eq!(projected.code_language.as_deref(), Some("mermaid"));
                for (token, color) in &tokens {
                    for (offset, _) in line.match_indices(token) {
                        assert_eq!(
                            run_at_byte(&runs, offset).color,
                            super::code_palette_hsla(*color),
                            "{line:?}: {token}"
                        );
                    }
                }
                if line.starts_with("```") || line.starts_with("~~~") {
                    assert_eq!(run_at_byte(&runs, 0).color, theme.syntax_marker);
                }
            }
        }
    }

    #[test]
    fn source_table_and_mermaid_lazy_projection_matches_eager_text_runs() {
        use crate::buffer::Buffer;
        use crate::editor::{
            new_lazy_markdown_decorations, parse_lazy_markdown_range, source_markdown_presentation,
        };

        let source = format!(
            "# Lead **heading**\n\n{}| **bold** | `code` |\n| :--- | ---: |\n| 中🙂 | value |\n\n{}\n```mermaid\nsequenceDiagram\n{}\n```\n",
            "plain\n\n".repeat(500),
            "more\n\n".repeat(500),
            "Alice->>Bob: Hello 中🙂\n\n".repeat(300),
        );
        let buffer = Buffer::from_text(&source);
        let (mut canonical, mut lazy) = new_lazy_markdown_decorations(buffer.line_count());
        let initial = parse_lazy_markdown_range(&buffer, &mut canonical, &mut lazy, 0..1).unwrap();
        let mut presentation = source_markdown_presentation(Vec::new(), &canonical, None, initial);
        let heading_spans = presentation[0].inline_spans.as_ptr();
        let eager = markdown::parse_decorations(&source);
        let theme = Theme::one_dark();
        for needle in ["| **bold**", "| :---", "Alice->>Bob"] {
            let row = (0..buffer.line_count())
                .rfind(|&row| buffer.line_content(row).starts_with(needle))
                .unwrap();
            if let Some(parsed) =
                parse_lazy_markdown_range(&buffer, &mut canonical, &mut lazy, row..row + 1)
            {
                presentation = source_markdown_presentation(presentation, &canonical, None, parsed);
            }
            assert_eq!(canonical[row], eager[row]);
            let (projected, runs) =
                source_syntax_runs(buffer.line_content(row), &canonical[row], &theme);
            let (_, eager_runs) = source_syntax_runs(buffer.line_content(row), &eager[row], &theme);
            assert_eq!(presentation[row], projected);
            assert_eq!(presentation[0].inline_spans.as_ptr(), heading_spans);
            assert!(runs.iter().any(|run| run.color != theme.text_primary));
            for (lazy, eager) in runs.iter().zip(&eager_runs) {
                assert_eq!(lazy.len, eager.len);
                assert_eq!(lazy.color, eager.color);
                assert_eq!(lazy.font, eager.font);
            }
            assert_eq!(runs.len(), eager_runs.len());
            assert!(
                parse_lazy_markdown_range(&buffer, &mut canonical, &mut lazy, row..row + 1)
                    .is_none()
            );
        }
    }

    #[test]
    fn source_table_and_mermaid_incremental_projection_matches_eager_text_runs() {
        use crate::buffer::Buffer;
        use crate::editor::{
            PendingLineDisplayTransfer, compute_incremental_markdown_decorations,
            line_display_transfer_line_bounds, source_markdown_presentation,
        };

        let before = "# Lead **heading**\n\n| A | B |\n| :--- | ---: |\n| **old** | `code` |\n\n```mermaid\nflowchart TD\nA[old]-->B\n```\n\nTail\n";
        for (old, inserted) in [
            ("**old**", "**new 中🙂**"),
            (
                "| **old** | `code` |",
                "| **new** | `code` |\n| extra | 中🙂 |",
            ),
            (":--- | ---:", ":---: | ---"),
            ("A[old]-->B", "A[中🙂]-.->B\nB==>|yes|C"),
        ] {
            let old_buffer = Buffer::from_text(before);
            let old_canonical = markdown::parse_decorations(before);
            let previous = source_markdown_presentation(
                Vec::new(),
                &old_canonical,
                None,
                0..old_canonical.len(),
            );
            let heading_spans = previous[0].inline_spans.as_ptr();
            let start = before.find(old).unwrap();
            let range = start..start + old.len();
            let (start_line, old_end_line_exclusive, new_end_line_exclusive) =
                line_display_transfer_line_bounds(&old_buffer, &range, inserted);
            let after = before.replacen(old, inserted, 1);
            let buffer = Buffer::from_text(&after);
            let byte_delta = after.len() as isize - before.len() as isize;
            let (canonical, parsed) = compute_incremental_markdown_decorations(
                old_canonical,
                &buffer,
                new_end_line_exclusive,
                old_end_line_exclusive,
                start_line,
                new_end_line_exclusive,
                byte_delta,
            )
            .expect("small table/diagram edits parse at canonical block boundaries");
            let transfer = PendingLineDisplayTransfer {
                cached_lines: Vec::new(),
                start_line,
                old_end_line_exclusive,
                new_end_line_exclusive,
                dirty_start_line: parsed.start,
                dirty_end_line_exclusive: parsed.end,
                markdown_plain_decoration_transfer: false,
                force_full_markdown_reparse: false,
                byte_delta,
            };
            let presentation =
                source_markdown_presentation(previous, &canonical, Some(&transfer), parsed);
            let eager = markdown::parse_decorations(&after);
            assert_eq!(canonical, eager);
            assert_eq!(
                presentation,
                source_markdown_presentation(Vec::new(), &eager, None, 0..eager.len())
            );
            assert_eq!(presentation[0].inline_spans.as_ptr(), heading_spans);
            let theme = Theme::one_dark();
            for row in start_line..new_end_line_exclusive {
                let (projected, runs) =
                    source_syntax_runs(buffer.line_content(row), &canonical[row], &theme);
                assert_eq!(presentation[row], projected);
                assert!(runs.iter().any(|run| run.color != theme.text_primary));
                let (_, eager_runs) =
                    source_syntax_runs(buffer.line_content(row), &eager[row], &theme);
                assert_eq!(runs.len(), eager_runs.len());
                for (incremental, eager) in runs.iter().zip(&eager_runs) {
                    assert_eq!(incremental.len, eager.len);
                    assert_eq!(incremental.color, eager.color);
                    assert_eq!(incremental.font, eager.font);
                }
            }
        }
    }

    #[test]
    fn source_code_and_diagram_syntax_has_colors_with_identity_maps() {
        for theme in [Theme::one_dark(), Theme::one_light()] {
            for (source, language, tokens) in [
                (
                    "```rust\nlet title = \"中🙂\";\n\n```",
                    "rust",
                    vec![
                        ("let", theme.code_palette.keyword),
                        ("\"中🙂\"", theme.code_palette.string),
                    ],
                ),
                (
                    "~~~python\nreturn \"中🙂\"\n~~~",
                    "python",
                    vec![
                        ("return", theme.code_palette.keyword),
                        ("\"中🙂\"", theme.code_palette.string),
                    ],
                ),
                (
                    "```mermaid\nflowchart TD\nA[中🙂] --> B[End]\nA-->B\nA-.->B\nmy-node-->other-node\n```",
                    "mermaid",
                    vec![
                        ("flowchart", theme.code_palette.keyword),
                        ("-->", theme.code_palette.operator),
                    ],
                ),
            ] {
                let canonical = markdown::parse_decorations(source);
                let saved = canonical.clone();
                for (index, line) in source.lines().enumerate() {
                    let (projected, runs) = source_syntax_runs(line, &canonical[index], &theme);
                    assert_eq!(
                        projected.code_language.as_deref(),
                        Some(if index == canonical.len() - 1 && language != "mermaid" {
                            "markdown"
                        } else {
                            language
                        }),
                        "{line:?}",
                    );
                    for (token, color) in &tokens {
                        if let Some(offset) = line.find(token) {
                            assert_eq!(
                                run_at_byte(&runs, offset).color,
                                gpui::rgb(*color).into(),
                                "{line:?}: {token}"
                            );
                        }
                    }
                    if line.starts_with("```") || line.starts_with("~~~") {
                        assert_eq!(run_at_byte(&runs, 0).color, theme.syntax_marker);
                    }
                }
                assert_eq!(canonical, saved);
            }
        }
    }

    #[test]
    fn code_text_runs_follow_palette_across_theme_cache_switches() {
        let line = "let moeka_theme_cache_probe = \"中🙂\"; // palette probe";
        let decoration = LineDecoration::default();
        let display_to_buffer = (0..line.len()).collect::<Vec<_>>();
        assert!(super::find_syntax_for_language("rust").is_some());
        let runs_for = |theme: &Theme| {
            super::build_text_runs_for_display_with_language(
                line,
                line,
                &display_to_buffer,
                &decoration,
                &[],
                false,
                false,
                &font("Test Body"),
                &font("Test CJK"),
                &font("Test Mono"),
                &font("Test Heading"),
                &font("Test Heading CJK"),
                &font("Test Italic"),
                &font("Test Italic CJK"),
                0,
                16.0,
                theme,
                theme.diff_added_bg,
                theme.diff_removed_bg,
                theme.link,
                FontWeight::NORMAL,
                theme.list_marker,
                FontWeight::BOLD,
                theme.syntax_marker,
                None,
                true,
                Some("rust"),
            )
        };
        let a = ThemeKind::OneDark.build();
        let b = ThemeKind::PorcelainMist.build();
        assert_ne!(a.code_palette.keyword, b.code_palette.keyword);
        let first_a = runs_for(&a);
        let middle_b = runs_for(&b);
        let cached_a = runs_for(&a);

        for (theme, runs) in [(&a, &first_a), (&b, &middle_b), (&a, &cached_a)] {
            assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), line.len());
            for (token, color) in [
                ("let", theme.code_palette.keyword),
                ("\"中🙂\"", theme.code_palette.string),
                (";", theme.code_palette.foreground),
                ("// palette probe", theme.code_palette.comment),
            ] {
                let start = line.find(token).unwrap();
                for offset in start..start + token.len() {
                    let run = run_at_byte(runs, offset);
                    assert_eq!(run.color, gpui::rgb(color).into(), "{token}, byte {offset}");
                    assert_eq!(run.font.family.as_ref(), "Test Mono");
                }
            }
            assert_eq!(
                run_at_byte(runs, line.find("//").unwrap()).font.style,
                FontStyle::Italic
            );
        }
        assert_ne!(
            run_at_byte(&first_a, 0).color,
            run_at_byte(&middle_b, 0).color
        );
        assert_eq!(first_a, cached_a);
    }

    mod code_text_run_regression_tests {
        use super::super::build_text_runs_for_display_with_language;
        use super::*;
        use crate::theme::CodeSyntaxPalette;

        struct Fixture {
            language: &'static str,
            source: &'static str,
            tokens: &'static [(usize, &'static str, &'static str)],
        }

        // Ordinary call names use variable.function.* in the bundled grammars.
        const FIXTURES: &[Fixture] = &[
            Fixture {
                language: "rust",
                source: "fn palette_probe() {\n    let title = \"中🙂\";\n    let count = value + 1;\n    render_title(title);\n    // 注释🙂 palette probe\n}",
                tokens: &[
                    (0, "fn", "keyword"),
                    (0, "palette_probe", "function"),
                    (1, "let", "keyword"),
                    (1, "\"中🙂\"", "string"),
                    (1, ";", "foreground"),
                    (2, "+", "operator"),
                    (3, "render_title", "function"),
                    (4, "// 注释🙂 palette probe", "comment"),
                ],
            },
            Fixture {
                language: "javascript",
                source: "function palette_probe() {\n    const title = \"中🙂\";\n    const count = value + 1;\n    render_title(title);\n    // 注释🙂 palette probe\n}",
                tokens: &[
                    (0, "function", "keyword"),
                    (0, "palette_probe", "function"),
                    (1, "const", "keyword"),
                    (1, "\"中🙂\"", "string"),
                    (1, ";", "foreground"),
                    (2, "+", "operator"),
                    (3, "render_title", "function"),
                    (4, "// 注释🙂 palette probe", "comment"),
                ],
            },
            Fixture {
                language: "python",
                source: "def palette_probe():\n    title = \"中🙂\"\n    count = value + 1\n    render_title(title)\n    return title  # 注释🙂 palette probe",
                tokens: &[
                    (0, "def", "keyword"),
                    (0, "palette_probe", "function"),
                    (0, ":", "foreground"),
                    (1, "\"中🙂\"", "string"),
                    (2, "+", "operator"),
                    (3, "render_title", "function"),
                    (4, "return", "keyword"),
                    (4, "# 注释🙂 palette probe", "comment"),
                ],
            },
        ];

        #[derive(Clone, Copy, Debug)]
        enum Entry {
            Fenced,
            RawFile,
        }

        const ENTRIES: [Entry; 2] = [Entry::Fenced, Entry::RawFile];

        fn palette_roles(palette: CodeSyntaxPalette) -> [(&'static str, u32); 9] {
            [
                ("keyword", palette.keyword),
                ("operator", palette.operator),
                ("string", palette.string),
                ("function", palette.function),
                ("type", palette.type_name),
                ("number", palette.number),
                ("variable", palette.variable),
                ("comment", palette.comment),
                ("foreground", palette.foreground),
            ]
        }

        fn role_color(palette: CodeSyntaxPalette, role: &str) -> gpui::Hsla {
            let (_, packed) = palette_roles(palette)
                .into_iter()
                .find(|(name, _)| *name == role)
                .expect("known code palette role");
            gpui::rgb(packed).into()
        }

        fn sentinel_theme() -> Theme {
            let mut theme = ThemeKind::PorcelainMist.build();
            theme.code_palette = CodeSyntaxPalette {
                keyword: 0xD12345,
                operator: 0x2A917C,
                string: 0x7A46B3,
                function: 0x3D71C2,
                type_name: 0xAF7526,
                number: 0x7B8E31,
                variable: 0xAB4578,
                comment: 0x69788F,
                foreground: 0x263A4D,
            };
            let colors = palette_roles(theme.code_palette)
                .map(|(_, color)| color)
                .into_iter()
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(colors.len(), 9);
            theme
        }

        fn fixture_runs(fixture: &Fixture, entry: Entry, theme: &Theme) -> Vec<Vec<TextRun>> {
            let fenced_source = format!("```{}\n{}\n```", fixture.language, fixture.source);
            let decorations = markdown::parse_decorations(&fenced_source);
            let raw_decoration = LineDecoration::default();
            assert!(super::super::find_syntax_for_language(fixture.language).is_some());
            fixture
                .source
                .lines()
                .enumerate()
                .map(|(row, line)| {
                    let (decoration, document_language, force_monospace) = match entry {
                        Entry::Fenced => {
                            let decoration = &decorations[row + 1];
                            assert_eq!(decoration.block_type, markdown::BlockType::CodeBlock);
                            assert_eq!(decoration.code_language.as_deref(), Some(fixture.language));
                            (decoration, None, false)
                        }
                        Entry::RawFile => (&raw_decoration, Some(fixture.language), true),
                    };
                    let (display, d2b, b2d, collapsed) = build_display_text(
                        line,
                        decoration,
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
                    assert_eq!(display, line);
                    assert_eq!(d2b, (0..line.len()).collect::<Vec<_>>());
                    assert_eq!(b2d, (0..=line.len()).collect::<Vec<_>>());
                    build_text_runs_for_display_with_language(
                        &display,
                        line,
                        &d2b,
                        decoration,
                        &[],
                        false,
                        collapsed,
                        &font("Test Body"),
                        &font("Test CJK"),
                        &font("Test Mono"),
                        &font("Test Heading"),
                        &font("Test Heading CJK"),
                        &font("Test Italic"),
                        &font("Test Italic CJK"),
                        0,
                        16.0,
                        theme,
                        theme.diff_added_bg,
                        theme.diff_removed_bg,
                        theme.link,
                        FontWeight::NORMAL,
                        theme.list_marker,
                        FontWeight::BOLD,
                        theme.syntax_marker,
                        None,
                        force_monospace,
                        document_language,
                    )
                })
                .collect()
        }

        fn assert_fixture_runs(
            fixture: &Fixture,
            theme: &Theme,
            runs: &[Vec<TextRun>],
            context: &str,
        ) {
            let lines = fixture.source.lines().collect::<Vec<_>>();
            assert_eq!(runs.len(), lines.len(), "{context}: fixture row count");
            for (line, line_runs) in lines.iter().zip(runs) {
                assert_eq!(
                    line_runs.iter().map(|run| run.len).sum::<usize>(),
                    line.len(),
                    "{context}: {line:?}"
                );
                let mut end = 0;
                for run in line_runs {
                    assert!(run.len > 0, "{context}");
                    end += run.len;
                    assert!(line.is_char_boundary(end), "{context}: byte {end}");
                    assert_eq!(run.font.family.as_ref(), "Test Mono", "{context}");
                    assert_eq!(run.font.weight, FontWeight::NORMAL, "{context}");
                    assert!(run.background_color.is_none(), "{context}");
                }
            }
            for &(row, token, role) in fixture.tokens {
                let line = lines[row];
                assert_eq!(line.match_indices(token).count(), 1, "{context}: {token}");
                let start = line.find(token).unwrap();
                for byte in start..start + token.len() {
                    let run = run_at_byte(&runs[row], byte);
                    assert_eq!(
                        run.color,
                        role_color(theme.code_palette, role),
                        "{context}: {role} {token:?}, byte {byte}"
                    );
                    assert_eq!(
                        run.font.style,
                        if role == "comment" {
                            FontStyle::Italic
                        } else {
                            FontStyle::Normal
                        },
                        "{context}: {token:?}, byte {byte}"
                    );
                }
            }
        }

        #[test]
        fn all_light_palettes_map_real_tokens_in_fenced_and_raw_files() {
            assert_eq!(ThemeKind::light_themes().len(), 16);
            let sentinel = sentinel_theme();
            for fixture in FIXTURES {
                for entry in ENTRIES {
                    let reference = fixture_runs(fixture, entry, &sentinel);
                    let context = format!("sentinel/{} {entry:?}", fixture.language);
                    assert_fixture_runs(fixture, &sentinel, &reference, &context);
                    for &kind in ThemeKind::light_themes() {
                        let theme = kind.build();
                        let actual = fixture_runs(fixture, entry, &theme);
                        let context = format!("{kind:?}/{} {entry:?}", fixture.language);
                        assert_fixture_runs(fixture, &theme, &actual, &context);
                        // Learn identifier roles from the real lexer, not from their spelling.
                        for (row, line) in fixture.source.lines().enumerate() {
                            for byte in 0..line.len() {
                                let expected = run_at_byte(&reference[row], byte);
                                let (role, _) = palette_roles(sentinel.code_palette)
                                    .into_iter()
                                    .find(|(_, packed)| {
                                        expected.color == gpui::Hsla::from(gpui::rgb(*packed))
                                    })
                                    .unwrap_or_else(|| {
                                        panic!("{context}: unrecognized sentinel at {row}:{byte}")
                                    });
                                let run = run_at_byte(&actual[row], byte);
                                assert_eq!(
                                    run.color,
                                    role_color(theme.code_palette, role),
                                    "{context}: {role} at {row}:{byte}"
                                );
                                assert_eq!(run.font, expected.font, "{context}: {row}:{byte}");
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn similar_light_pairs_restore_fenced_and_raw_runs_after_cache_switches() {
            let pairs = [
                (ThemeKind::VanillaLatte, ThemeKind::PearlMorning),
                (ThemeKind::VanillaLatte, ThemeKind::LavenderHaze),
                (ThemeKind::PearlMorning, ThemeKind::LavenderHaze),
                (ThemeKind::NordicSnow, ThemeKind::CoastalFog),
                (ThemeKind::LinenPaper, ThemeKind::SunlitCanvas),
                (ThemeKind::LinenPaper, ThemeKind::ApricotBloom),
                (ThemeKind::SunlitCanvas, ThemeKind::ApricotBloom),
            ];
            for fixture in FIXTURES {
                for entry in ENTRIES {
                    for (left, right) in pairs {
                        let a = left.build();
                        let b = right.build();
                        let first_a = fixture_runs(fixture, entry, &a);
                        let middle_b = fixture_runs(fixture, entry, &b);
                        let restored_a = fixture_runs(fixture, entry, &a);
                        let context = format!(
                            "{left:?} → {right:?} → {left:?}/{} {entry:?}",
                            fixture.language
                        );
                        for (theme, runs) in [(&a, &first_a), (&b, &middle_b), (&a, &restored_a)] {
                            assert_fixture_runs(fixture, theme, runs, &context);
                        }
                        assert_ne!(first_a, middle_b, "{context}: palette switch had no effect");
                        assert_eq!(first_a, restored_a, "{context}: stale cached text runs");
                    }
                }
            }
        }
    }

    #[test]
    fn source_math_syntax_and_delimiters_have_colors_with_identity_maps() {
        for theme in [Theme::one_dark(), Theme::one_light()] {
            for source in [
                "$$\n\\frac{x^2}{2} + \\alpha\n\\text{中🙂}\n$$",
                "\\[\n\\frac{x^2}{2} + \\alpha\n\\text{中🙂}\n\\]",
                "$$\\frac{x^2}{2} + \\alpha$$",
                "\\[\\frac{x^2}{2} + \\alpha\\]",
            ] {
                let canonical = markdown::parse_decorations(source);
                let saved = canonical.clone();
                for (index, line) in source.lines().enumerate() {
                    let (projected, runs) = source_syntax_runs(line, &canonical[index], &theme);
                    assert_eq!(
                        projected.code_language.as_deref(),
                        Some(super::SOURCE_MATH_HIGHLIGHT_LANGUAGE)
                    );
                    for (token, color) in [
                        ("frac", theme.code_palette.function),
                        ("alpha", theme.code_palette.keyword),
                        ("^", theme.code_palette.operator),
                        ("2", theme.code_palette.number),
                    ] {
                        if let Some(offset) = line.find(token) {
                            assert_eq!(
                                run_at_byte(&runs, offset).color,
                                gpui::rgb(color).into(),
                                "{line:?}: {token}",
                            );
                        }
                    }
                    for marker in ["$$", "\\[", "\\]"] {
                        for (offset, _) in line.match_indices(marker) {
                            assert_eq!(
                                run_at_byte(&runs, offset).color,
                                theme.syntax_marker,
                                "{line:?}"
                            );
                        }
                    }
                }
                assert_eq!(canonical, saved);
            }
        }
    }

    #[test]
    fn source_highlighting_fenced_latex_keeps_document_context_distinct_from_math_previews() {
        let body = "x^2 + \\alpha + \\text{中🙂}";
        for theme in [Theme::one_dark(), Theme::one_light()] {
            // Repeat after math shaping to exercise both warm-cache directions.
            for _ in 0..2 {
                for language in ["latex", "tex"] {
                    let source =
                        format!("```{language}\n{body}\n\\[\n{body}\n\\]\n$$\n{body}\n$$\n```");
                    let canonical = markdown::parse_decorations(&source);
                    let saved = canonical.clone();
                    for (row, line) in source.lines().enumerate() {
                        assert!(canonical[row].block_preview.is_none());
                        let (_, runs) = source_syntax_runs(line, &canonical[row], &theme);
                        if row == 0 || row == canonical.len() - 1 {
                            assert_eq!(run_at_byte(&runs, 0).color, theme.syntax_marker);
                            continue;
                        }
                        let mut colors = vec![theme.text_primary; line.len()];
                        let mut weights = vec![FontWeight::NORMAL; line.len()];
                        let mut styles = vec![FontStyle::Normal; line.len()];
                        super::apply_code_block_highlighting(
                            line,
                            language,
                            &theme,
                            &mut colors,
                            &mut weights,
                            &mut styles,
                        );
                        for byte in 0..line.len() {
                            let run = run_at_byte(&runs, byte);
                            assert_eq!(
                                run.color, colors[byte],
                                "{language}: {line:?}: byte {byte}"
                            );
                            assert_eq!(run.font.weight, weights[byte]);
                            assert_eq!(run.font.style, styles[byte]);
                        }
                        if line == body {
                            assert_eq!(run_at_byte(&runs, 0).font.family.as_ref(), "Test Body");
                            assert_eq!(
                                run_at_byte(&runs, line.find('中').unwrap())
                                    .font
                                    .family
                                    .as_ref(),
                                "Test CJK"
                            );
                        }
                    }
                    assert_eq!(canonical, saved);
                }
                for source in [format!("$$\n{body}\n$$"), format!("\\[\n{body}\n\\]")] {
                    let canonical = markdown::parse_decorations(&source);
                    let saved = canonical.clone();
                    for (row, line) in source.lines().enumerate() {
                        assert!(
                            canonical[row]
                                .block_preview
                                .as_ref()
                                .is_some_and(
                                    |preview| preview.kind == markdown::BlockPreviewKind::Math
                                )
                        );
                        let (_, runs) = source_syntax_runs(line, &canonical[row], &theme);
                        if line == body {
                            assert_eq!(
                                run_at_byte(&runs, line.find('^').unwrap()).color,
                                super::code_palette_hsla(theme.code_palette.operator)
                            );
                            assert_eq!(
                                run_at_byte(&runs, line.find('2').unwrap()).color,
                                super::code_palette_hsla(theme.code_palette.number)
                            );
                            assert_eq!(
                                run_at_byte(&runs, line.find("alpha").unwrap()).color,
                                super::code_palette_hsla(theme.code_palette.keyword)
                            );
                            assert_eq!(run_at_byte(&runs, 0).font.family.as_ref(), "Test Body");
                            assert_eq!(
                                run_at_byte(&runs, line.find('中').unwrap())
                                    .font
                                    .family
                                    .as_ref(),
                                "Test CJK"
                            );
                        } else {
                            assert_eq!(run_at_byte(&runs, 0).color, theme.syntax_marker);
                        }
                    }
                    assert_eq!(canonical, saved);
                }
            }
        }
    }

    #[test]
    fn source_math_context_does_not_leak_into_cached_latex_document_highlighting() {
        let theme = Theme::one_dark();
        let line = "x^2 + \\alpha";
        let colors = |source_math| {
            let mut colors = vec![theme.text_primary; line.len()];
            super::apply_language_highlighting(
                line,
                "latex",
                &theme,
                &mut colors,
                &mut vec![FontWeight::NORMAL; line.len()],
                &mut vec![FontStyle::Normal; line.len()],
                source_math,
            );
            colors
        };
        let document = colors(false);
        let source = colors(true);
        assert_ne!(source[1], document[1]);
        assert_eq!(source[1], gpui::rgb(theme.code_palette.operator).into());
        assert_eq!(colors(false), document);
        assert_eq!(colors(true), source);
    }

    #[test]
    fn source_projection_keeps_diff_spans_and_normalizes_table_syntax_languages() {
        for source in [
            "```rust\nlet x = 1;\n```",
            "```mermaid\nflowchart TD\nA --> B\n```",
            "$$\n\\alpha\n$$",
        ] {
            for mut row in markdown::parse_decorations(source) {
                row.diff_added_spans = vec![0..1];
                row.diff_removed_spans = vec![1..2];
                super::source_markdown_decoration(&mut row);
                assert!(row.code_language.is_some());
                assert_eq!(row.diff_added_spans, vec![0..1]);
                assert_eq!(row.diff_removed_spans, vec![1..2]);
                let projected = row.clone();
                super::source_markdown_decoration(&mut row);
                assert_eq!(row, projected);
            }
        }
        for (source, language) in [
            ("```mermaid\nflowchart TD\nA --> B\n```", "mermaid"),
            ("$$\n\\alpha\n$$", super::SOURCE_MATH_HIGHLIGHT_LANGUAGE),
        ] {
            for mut row in markdown::parse_decorations(source) {
                row.code_language = None;
                super::source_markdown_decoration(&mut row);
                assert_eq!(row.code_language.as_deref(), Some(language));
            }
        }
        for mut row in
            markdown::parse_decorations("| A | B |\n| --- | --- |\n| **value** | a<br>b |")
        {
            // Inline diff stores removed tables as code tagged `table`.
            row.code_language = Some("table".to_string());
            row.block_type = markdown::BlockType::CodeBlock;
            super::source_markdown_decoration(&mut row);
            let mut expected = LineDecoration::default();
            expected.code_language = Some("markdown".to_string());
            assert_eq!(row, expected);
            super::source_markdown_decoration(&mut row);
            assert_eq!(row, expected);
        }
        for mut row in markdown::parse_decorations("---\ntitle: '**YAML**'\n---") {
            super::source_markdown_decoration(&mut row);
            assert_eq!(row, LineDecoration::default());
        }
    }

    #[test]
    fn heading_body_cursor_paints_syntax_marker_color() {
        let line = "# title";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges = [Range { start: 3, end: 3 }];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        assert!(!prefix_collapsed);
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, line);
        assert_eq!(runs.first().map(|run| run.color), Some(theme.syntax_marker));
    }

    #[test]
    fn empty_source_with_display_placeholder_builds_safe_text_run() {
        let decoration = markdown::LineDecoration::default();
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            "\u{2003}",
            "",
            &[0, 0, 0],
            &decoration,
            &[],
            false,
            false,
            &font(".SystemUIFont"),
            &font("PingFang SC"),
            &font("Menlo"),
            &font(".SystemUIFont"),
            &font("PingFang SC"),
            &font("Menlo"),
            &font("PingFang SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len, "\u{2003}".len());
        assert_eq!(runs[0].color, theme.text_primary);
    }

    #[test]
    fn folded_italic_uses_semantic_palette_and_italic_font_role() {
        for kind in [ThemeKind::PorcelainMist, ThemeKind::OneDark] {
            for &mode in EditorThemeMode::all() {
                let theme = apply_editor_theme_mode(&kind.build(), mode, kind.is_dark());
                let (display_text, runs) = folded_display_and_runs("plain *italic* text", &theme);

                assert_eq!(display_text, "plain italic text");
                let italic = run_at_byte(&runs, display_text.find("italic").unwrap());
                assert_eq!(italic.color, theme.italic, "{} {mode:?}", kind.label());
                assert_eq!(italic.font.family.as_ref(), "Menlo");
                assert_eq!(italic.font.style, FontStyle::Italic);
                assert_eq!(italic.font.weight, FontWeight::NORMAL);
                assert_eq!(run_at_byte(&runs, 0).color, theme.text_primary);
            }
        }
    }

    #[test]
    fn nested_italic_preserves_emphasis_and_inline_code_priority() {
        for kind in [ThemeKind::PorcelainMist, ThemeKind::OneDark] {
            for &mode in EditorThemeMode::all() {
                let theme = apply_editor_theme_mode(&kind.build(), mode, kind.is_dark());
                for (source, token, color) in [
                    ("**bold *italic***", "italic", theme.italic),
                    ("*italic **bold***", "bold", theme.bold),
                ] {
                    let (display_text, runs) = folded_display_and_runs(source, &theme);
                    let nested = run_at_byte(&runs, display_text.find(token).unwrap());
                    assert_eq!(nested.color, color, "{} {mode:?}: {source}", kind.label());
                    assert_eq!(nested.font.family.as_ref(), "Menlo");
                    assert_eq!(nested.font.style, FontStyle::Italic);
                    assert_eq!(nested.font.weight, FontWeight::EXTRA_BOLD);
                }

                let (display_text, runs) = folded_display_and_runs("*`code`*", &theme);
                assert_eq!(display_text, "code");
                let code = run_at_byte(&runs, 0);
                assert_eq!(code.color, theme.inline_code);
                assert_eq!(code.background_color, Some(theme.inline_code_bg));
                assert_eq!(code.font.family.as_ref(), "Test Mono");
                assert_eq!(code.font.style, FontStyle::Italic);
            }
        }
    }

    #[test]
    fn folded_cjk_italic_keeps_cjk_family_and_uses_oblique_style() {
        let line = "普通 *中文斜体* 文本";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges: [Range<usize>; 0] = [];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font("Menlo"),
            &font("PingFang SC"),
            &font("Menlo"),
            &font("PingFang SC"),
            &font("Menlo"),
            &font("Menlo"),
            &font("PingFang SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, "普通 中文斜体 文本");
        assert!(runs.iter().any(|run| {
            run.font.family.as_ref() == "PingFang SC"
                && run.font.style == FontStyle::Oblique
                && run.color == theme.italic
        }));
    }

    #[test]
    fn folded_inline_code_uses_inline_code_background() {
        let line = "use `code` here";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges: [Range<usize>; 0] = [];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, "use code here");
        assert!(
            runs.iter()
                .any(|run| run.background_color == Some(theme.inline_code_bg))
        );
    }

    #[test]
    fn nested_code_and_math_keep_semantic_style_priority() {
        let theme = Theme::one_dark();
        let (display_text, runs) = folded_display_and_runs("**==`code`==**", &theme);
        let code = run_at_byte(&runs, display_text.find("code").expect("nested code"));
        assert_eq!(display_text, "code");
        assert_eq!(code.color, theme.inline_code);
        assert_eq!(code.background_color, Some(theme.inline_code_bg));
        assert_eq!(code.font.family.as_ref(), "Test Mono");
        assert_eq!(code.font.weight, FontWeight::BOLD);

        let (display_text, runs) = folded_display_and_runs("**==$x$==**", &theme);
        let math = run_at_byte(&runs, display_text.find('x').expect("nested math"));
        assert_eq!(display_text, "x");
        assert_eq!(math.color, theme.bold);
        assert_eq!(math.background_color, Some(theme.highlight_bg));
        assert_eq!(math.font.family.as_ref(), ".SystemUIFont");
        assert_eq!(math.font.weight, FontWeight::BOLD);
    }

    #[test]
    fn link_label_custom_syntax_folds_and_combines_styles() {
        let theme = Theme::one_dark();
        let line = "[==label== ++under++ :rocket:](target)";
        let (display_text, runs) = folded_display_and_runs(line, &theme);
        let label = run_at_byte(&runs, display_text.find("label").expect("label"));
        let under = run_at_byte(&runs, display_text.find("under").expect("underline"));

        assert_eq!(display_text, "label under 🚀");
        assert_eq!(label.background_color, Some(theme.highlight_bg));
        assert_eq!(under.color, theme.link);
        assert!(under.underline.is_some());
    }

    #[test]
    fn nested_folded_display_and_text_runs_remain_consistent() {
        let theme = Theme::one_dark();
        let cases = [
            ("**==`code`==**", "code"),
            ("[==label== ++under++ :rocket:](target)", "label under 🚀"),
            ("[[target|==别名== ++线++ :rocket:]]", "别名 线 🚀"),
            ("~~H~2~O~~", "H O"),
            ("$x^2^$", "x^2^"),
            (r"\==literal== ++open", r"\==literal== ++open"),
        ];

        for (line, expected) in cases {
            let (display_text, runs) = folded_display_and_runs(line, &theme);
            assert_eq!(display_text, expected, "display: {line}");
            assert_eq!(
                runs.iter().map(|run| run.len).sum::<usize>(),
                display_text.len(),
                "run length: {line}"
            );
        }
    }

    #[test]
    fn expanded_inline_code_marks_backticks_with_background() {
        let line = "use `code` here";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges = [4..10];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, line);
        let mut backgrounds = vec![None; display_text.len()];
        let mut offset = 0;
        for run in runs {
            for background in backgrounds.iter_mut().skip(offset).take(run.len) {
                *background = run.background_color;
            }
            offset += run.len;
        }

        assert_eq!(backgrounds[4], Some(theme.inline_code_bg));
        assert_eq!(backgrounds[9], Some(theme.inline_code_bg));
    }

    #[test]
    fn folded_strikethrough_uses_muted_text_color() {
        let line = "use ~~gone~~ here";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges: [Range<usize>; 0] = [];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, "use gone here");
        assert!(
            runs.iter()
                .any(|run| { run.strikethrough.is_some() && run.color == theme.strikethrough })
        );
    }

    #[test]
    fn folded_underline_uses_text_run_underline() {
        let line = "use ++line++ here";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges: [Range<usize>; 0] = [];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, "use line here");
        assert!(runs.iter().any(|run| run.underline.is_some()));
    }

    #[test]
    fn folded_callout_marker_uses_icon_placeholder() {
        let line = "> [!WARNING] Use it";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, _, _, _) = build_display_text(
            line,
            &decoration,
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

        assert_eq!(display_text, "\u{2003}\u{2009}Use it");

        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &[],
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );
        assert!(
            runs.iter().any(|run| {
                run.font.weight == FontWeight::EXTRA_BOLD && run.color == theme.bold
            })
        );
    }

    #[test]
    fn front_matter_block_focus_includes_every_row_eol_selection_and_ime_not_body() {
        for source in [
            "---\ntitle: '中'\nbody: |\n  first\n\n  last\ntags:\n  - one\n  - two\n---\nBody",
            "---\r\ntitle: 中\r\n...\r\nBody",
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = markdown::parse_decorations(source);
            let end = decorations[0].table_info.as_ref().unwrap().table_end_line;
            assert!(!super::front_matter_block_is_active(
                &buffer,
                &decorations,
                &[],
                true
            ));
            for line in 0..end {
                for offset in [buffer.line_start(line), buffer.line_end(line)] {
                    assert!(
                        super::front_matter_block_is_active(
                            &buffer,
                            &decorations,
                            &[offset..offset],
                            true
                        ),
                        "line {line}"
                    );
                    assert!(!super::front_matter_block_is_active(
                        &buffer,
                        &decorations,
                        &[offset..offset],
                        false
                    ));
                }
            }
            let marked = buffer.line_start(1)..buffer.line_end(1);
            assert!(super::front_matter_block_is_active(
                &buffer,
                &decorations,
                &[marked],
                true
            ));
            let body = buffer.line_start(end);
            assert!(!super::front_matter_block_is_active(
                &buffer,
                &decorations,
                &[body..body, body..buffer.len()],
                true
            ));
            assert!(super::front_matter_block_is_active(
                &buffer,
                &decorations,
                &[buffer.line_start(1)..buffer.len()],
                true
            ));
        }
        let buffer = Buffer::from_text("---\ntitle: value\n---");
        let decorations = markdown::parse_decorations(buffer.text());
        let eof = buffer.len();
        assert!(super::front_matter_block_is_active(
            &buffer,
            &decorations,
            &[eof..eof],
            true
        ));
    }

    #[test]
    fn front_matter_scalar_focus_signature_changes_inside_an_already_active_block() {
        let buffer = Buffer::from_text("---\ntitle: 'one'\nflag: true\n---\nBody");
        for line in [1, 2] {
            let offset = buffer.line_end(line);
            let ranges = [offset..offset];
            assert!(super::front_matter_source_rows_are_active(
                &buffer,
                0..4,
                &ranges
            ));
            assert!(super::front_matter_source_rows_are_active(
                &buffer,
                line..line + 1,
                &ranges
            ));
            let other = 3 - line;
            assert!(!super::front_matter_source_rows_are_active(
                &buffer,
                other..other + 1,
                &ranges
            ));
        }
    }

    fn front_matter_test_display(
        source: &str,
        decoration: &LineDecoration,
        active_ranges: &[Range<usize>],
        group_active: bool,
    ) -> (String, Vec<usize>, Vec<usize>, bool) {
        build_display_text(
            source,
            decoration,
            active_ranges,
            group_active,
            None,
            0,
            3,
            0,
            0,
            1,
            1.0,
            1.0,
            1.0,
        )
    }

    fn front_matter_test_runs(
        source: &str,
        decoration: &LineDecoration,
        active_ranges: &[Range<usize>],
        group_active: bool,
        theme: &Theme,
    ) -> (String, Vec<TextRun>) {
        let (display, d2b, _, collapsed) =
            front_matter_test_display(source, decoration, active_ranges, group_active);
        let runs = build_text_runs_for_display(
            &display,
            source,
            &d2b,
            decoration,
            active_ranges,
            group_active,
            collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Kaiti SC"),
            3,
            16.0,
            theme,
            theme.git_status_added,
            theme.text_danger,
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            Some(theme.text_bright),
            false,
        );
        (display, runs)
    }

    #[test]
    fn front_matter_mapper_preserves_scalar_source_and_continuation_padding() {
        let source = concat!(
            "---\n",
            "title: \"Moeka: 编辑器\" # title\n",
            "quoted_bool: 'true'\n",
            "enabled: true # switch\n",
            "disabled: false\n",
            "nothing: null\n",
            "number: 1.20e+3\n",
            "date: 2026-10-12T14:30:00Z\n",
            "tags:\n",
            "  - \"first: item\"\n",
            "  - 'true'\n",
            "nested:\n",
            "  key: \"中文\\u263a\"\n",
            "description: |-\n",
            "  1. literal **text**\n",
            "  true\n",
            "  2026-10-12\n",
            "  key: not a mapping\n",
            "folded: >+\n",
            "  false\n",
            "  another line\n",
            "---",
        );
        let decorations = markdown::parse_decorations(source);
        let lines: Vec<_> = source.lines().collect();
        let expected = [
            (1, "Moeka: 编辑器 # title"),
            (2, "true"),
            (3, "\u{a0}\u{a0}\u{a0} # switch"),
            (4, "\u{a0}\u{a0}\u{a0}"),
            (5, "null"),
            (6, "1.20e+3"),
            (7, "2026-10-12T14:30:00Z"),
            (8, ""),
            (9, "• first: item"),
            (10, "• true"),
            (11, ""),
            (12, "中文☺"),
            (13, ""),
            (14, "1. literal **text**"),
            (15, "true"),
            (16, "2026-10-12"),
            (17, "key: not a mapping"),
            (18, ""),
            (19, "false"),
            (20, "another line"),
        ];
        for (index, value) in expected {
            let decoration = &decorations[index];
            let info = decoration
                .front_matter
                .as_ref()
                .expect("front matter metadata");
            let (display, _, b2d, _) =
                front_matter_test_display(lines[index], decoration, &[], true);
            let display_start = if info.key_range.is_some() {
                b2d[info.value_range.start]
            } else {
                display.len() - display.trim_start_matches(' ').len()
            };
            assert_eq!(&display[display_start..], value, "collapsed row {index}");
            assert!(!display.contains(['\n', '\r']));
            // The mapper accepts row-local activity; the shared group plan
            // supplies it to every member of an active multiline/list value.
            for position in lines[index]
                .char_indices()
                .map(|(i, _)| i)
                .chain([lines[index].len()])
            {
                let (expanded, _, b2d, _) = front_matter_test_display(
                    lines[index],
                    decoration,
                    &[position..position],
                    false,
                );
                if info.key_range.is_some() {
                    assert_eq!(
                        &expanded[b2d[info.value_range.start]..],
                        &lines[index][info.value_range.start..]
                    );
                } else {
                    let value_display = b2d[info.value_range.start];
                    assert_eq!(
                        &expanded[value_display..],
                        &lines[index][info.value_range.start..]
                    );
                    let collapsed = front_matter_test_display(lines[index], decoration, &[], false);
                    assert_eq!(
                        display_width(&expanded[..value_display]),
                        display_width(&collapsed.0[..collapsed.2[info.value_range.start]])
                    );
                }
            }
            let selected = [0..lines[index].len()];
            let expanded = front_matter_test_display(lines[index], decoration, &selected, false).0;
            assert!(expanded.contains(&lines[index][info.value_range.clone()]));
            assert_eq!(
                front_matter_test_display(lines[index], decoration, &[], false).0,
                display
            );
        }
        let width = decorations[1].table_info.as_ref().unwrap().column_widths[0];
        assert_eq!(width, "quoted_bool".len());
        for index in [9, 10, 14, 15, 16, 17, 19, 20] {
            assert!(
                decorations[index]
                    .front_matter
                    .as_ref()
                    .unwrap()
                    .key_range
                    .is_none()
            );
            assert!(decorations[index].table_info.as_ref().unwrap().cells[0].is_empty());
        }
        assert_eq!(source.lines().collect::<Vec<_>>(), lines);
    }

    #[test]
    fn front_matter_fold_summary_maps_synthetic_utf8_only_to_delimiter_boundaries() {
        for source in ["---", "\u{feff}---", "---\r"] {
            for count in [0, 1, 20, 1000] {
                let summary = crate::i18n::front_matter_fold_summary(
                    crate::i18n::UiLanguage::ChineseSimplified,
                    count,
                );
                let (display, d2b, b2d, collapsed) =
                    super::build_front_matter_fold_display(source, &summary);
                assert_eq!(display, summary);
                assert!(!collapsed);
                assert_eq!(d2b.len(), display.len() + 1);
                assert!(d2b[..display.len()].iter().all(|offset| *offset == 0));
                assert_eq!(d2b[display.len()], source.len());
                assert_eq!(b2d.len(), source.len() + 1);
                assert!(b2d[..source.len()].iter().all(|offset| *offset == 0));
                assert_eq!(b2d[source.len()], display.len());
                assert!(d2b.iter().all(|offset| source.is_char_boundary(*offset)));
                assert!(b2d.iter().all(|offset| display.is_char_boundary(*offset)));
            }
        }
    }

    #[test]
    fn front_matter_active_block_indicator_alone_uses_faded_syntax_color() {
        let theme = crate::theme::Theme::one_dark();
        for token in ["|", "|-", "|+", ">", ">-", ">+", "|2-", ">+2"] {
            let source = format!("---\nbody: {token} # keep\n  中文🌸\n---");
            let decorations = markdown::parse_decorations(&source);
            let text = source.lines().nth(1).unwrap();
            let info = decorations[1].front_matter.as_ref().unwrap();
            let (display, runs) = front_matter_test_runs(
                text,
                &decorations[1],
                &[info.value_range.start..info.value_range.start],
                true,
                &theme,
            );
            let (_, _, b2d, _) = front_matter_test_display(
                text,
                &decorations[1],
                &[info.value_range.start..info.value_range.start],
                true,
            );
            let colors: Vec<_> = runs
                .iter()
                .flat_map(|run| std::iter::repeat_n(run.color, run.len))
                .collect();
            assert_eq!(
                &display[b2d[info.value_range.start]..b2d[info.value_range.end]],
                token
            );
            assert!(
                colors[b2d[info.value_range.start]..b2d[info.value_range.end]]
                    .iter()
                    .all(|color| *color == theme.syntax_marker)
            );
            assert_eq!(colors[0], theme.text_bright);
            assert_eq!(colors[display.find('#').unwrap()], theme.text_primary);
            let body = source.lines().nth(2).unwrap();
            let (body_display, body_runs) =
                front_matter_test_runs(body, &decorations[2], &[0..body.len()], true, &theme);
            assert!(body_display.contains("中文🌸"));
            assert!(body_runs.iter().all(|run| run.color == theme.text_primary));
        }
    }

    #[test]
    fn front_matter_group_focus_reveals_only_own_members_and_keeps_scalar_rows_local() {
        let source = "---\ntitle: 'scalar'\nbody: |-\n  中文🌸\n  second\ntags:\n  - 'one'\n  - \"二\\u263a\"\nother: true\n---";
        let buffer = Buffer::from_text(source);
        let decorations = markdown::parse_decorations(source);
        let collapsed = front_matter_group_plan(&buffer, &decorations, &[], true);
        assert_eq!(collapsed.len(), 2);
        assert!(collapsed[0].hides_line(2));
        assert_eq!(collapsed[0].label_line(), 3);
        assert!(!collapsed[0].hides_line(3));
        assert!(!collapsed[1].hides_line(5));
        assert!(collapsed[1].hides_line(6));
        assert!(collapsed[1].hides_line(7));
        for caret_line in 2..=7 {
            let caret = buffer.line_end(caret_line);
            let plans = front_matter_group_plan(&buffer, &decorations, &[caret..caret], true);
            let active_index = usize::from(caret_line >= 5);
            assert!(plans[active_index].active);
            assert!(!plans[1 - active_index].active);
            assert!(
                plans[active_index]
                    .group
                    .source_lines
                    .clone()
                    .all(|line| !plans[active_index].hides_line(line))
            );
        }
        let ranges = [
            buffer.line_end(3)..buffer.line_end(3),
            buffer.line_start(7)..buffer.line_end(7),
        ];
        assert!(
            front_matter_group_plan(&buffer, &decorations, &ranges, true)
                .iter()
                .all(|plan| plan.active)
        );
        // The same absolute ranges cover extra carets, selections and IME.
        assert!(
            front_matter_group_plan(&buffer, &decorations, &ranges, false)
                .iter()
                .all(|plan| !plan.active)
        );
        for line in [0, 1, 8, 9] {
            let caret = buffer.line_end(line);
            assert!(
                front_matter_group_plan(&buffer, &decorations, &[caret..caret], true)
                    .iter()
                    .all(|plan| !plan.active)
            );
        }
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn front_matter_long_key_list_uses_identical_source_visible_width_fallback_in_both_focus_states()
     {
        let key = "long_key_".repeat(20);
        for source in [
            format!("---\n{key}: [one, '二']\n---"),
            format!("---\n{key}:\n  - one\n  - '二'\n---"),
        ] {
            let buffer = Buffer::from_text(&source);
            let decorations = markdown::parse_decorations(&source);
            for ranges in [vec![], vec![buffer.line_end(1)..buffer.line_end(1)]] {
                let mut plans = front_matter_group_plan(&buffer, &decorations, &ranges, true);
                super::apply_front_matter_group_width_limit(&mut plans, &decorations, 12.0);
                assert!(plans[0].source_visible);
                for line in plans[0].group.source_lines.clone() {
                    assert!(!plans[0].hides_line(line));
                    let text = buffer.line_content(line);
                    let mut decoration = decorations[line].clone();
                    apply_front_matter_group_display(&mut decoration, line, Some(&plans[0]), text);
                    let (display, d2b, b2d, _) =
                        front_matter_test_display(text, &decoration, &[], false);
                    assert_eq!(display, text);
                    for (index, _) in text.char_indices() {
                        assert_eq!(b2d[index], index);
                        assert_eq!(d2b[index], index);
                    }
                }
            }
            assert_eq!(buffer.text(), source);
        }
        let source = "---\ntags: [one, two]\n---";
        let buffer = Buffer::from_text(source);
        let decorations = markdown::parse_decorations(source);
        let mut plans = front_matter_group_plan(&buffer, &decorations, &[], true);
        super::apply_front_matter_group_width_limit(&mut plans, &decorations, 20.0);
        assert!(!plans[0].source_visible);
    }

    #[test]
    fn front_matter_commented_lists_remain_readable_without_expanding_or_mutating_source() {
        for source in [
            "---\ntags: [one, 'two#literal'] # header\n---",
            "---\ntags:\n  - 'one#literal' # item\n  # between\n  - two\n---",
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = markdown::parse_decorations(source);
            let plans = front_matter_group_plan(&buffer, &decorations, &[], false);
            assert!(plans[0].source_visible);
            for line in plans[0].group.source_lines.clone() {
                assert!(!plans[0].hides_line(line));
                let text = buffer.line_content(line);
                let mut decoration = decorations[line].clone();
                apply_front_matter_group_display(&mut decoration, line, Some(&plans[0]), text);
                assert_eq!(
                    front_matter_test_display(text, &decoration, &[], false).0,
                    text
                );
            }
            assert_eq!(buffer.text(), source);
        }
        let source = "---\ntags: ['one#literal']\n---";
        let buffer = Buffer::from_text(source);
        let decorations = markdown::parse_decorations(source);
        assert!(!front_matter_group_plan(&buffer, &decorations, &[], false)[0].source_visible);
    }

    #[test]
    fn front_matter_wide_dash_spacing_keeps_all_item_value_columns_and_exact_scalar_hits() {
        let source = "---\ntags:\n  -        one\n  - 二🌸\n---";
        let buffer = Buffer::from_text(source);
        let decorations = markdown::parse_decorations(source);
        let mut column = None;
        for line in [2, 3] {
            let text = buffer.line_content(line);
            let info = decorations[line].front_matter.as_ref().unwrap();
            for ranges in [vec![], vec![0..text.len()]] {
                let (display, d2b, b2d, _) =
                    front_matter_test_display(text, &decorations[line], &ranges, false);
                let value = b2d[info.value_range.start];
                let value_column = display_width(&display[..value]);
                assert_eq!(*column.get_or_insert(value_column), value_column);
                for (index, _) in text[info.value_range.clone()].char_indices() {
                    let source_index = info.value_range.start + index;
                    assert_eq!(d2b[b2d[source_index]], source_index);
                }
                if !ranges.is_empty() {
                    assert_eq!(&display[value..], &text[info.value_range.start..]);
                }
            }
        }
        assert_eq!(column, Some(6));
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn front_matter_normalized_newline_hits_keep_decoded_character_spans_and_crlf_selection() {
        for (token, display) in [
            ("\"a\\nb\"", "a b"),
            ("\"中\\r\\n🌸\"", "中 🌸"),
            ("\"a\\Nb\\Lc\\Pd\"", "a b c d"),
        ] {
            let start = 100;
            let map = front_matter_item_source_map(token, display, start);
            assert_eq!(map.len(), display.len() + 1);
            let last_display = display.char_indices().next_back().unwrap().0;
            let last_source = token[..token.len() - 1]
                .char_indices()
                .next_back()
                .unwrap()
                .0;
            assert_eq!(map[last_display], start + last_source);
            assert!(
                map.iter()
                    .all(|offset| token.is_char_boundary(offset - start))
            );
            if let Some(crlf) = token.find("\\r\\n") {
                let space = display.find(' ').unwrap();
                assert_eq!(map[space], start + crlf);
                assert_eq!(
                    super::front_matter_display_selection_ranges(
                        display,
                        &map,
                        &[start + crlf + 2..start + crlf + 4]
                    ),
                    vec![space..space + 1]
                );
            }
        }
    }

    #[test]
    fn front_matter_group_body_columns_are_focus_invariant() {
        let source = "---\nnested:\n  body: |2-\n    first\n    中文🌸\n  tags:\n    - 'first'\n    - second\n---";
        let decorations = markdown::parse_decorations(source);
        for line in [3, 4, 6, 7] {
            let text = source.lines().nth(line).unwrap();
            let info = decorations[line].front_matter.as_ref().unwrap();
            let (collapsed, _, collapsed_map, _) =
                front_matter_test_display(text, &decorations[line], &[], false);
            let (active, _, active_map, _) = front_matter_test_display(
                text,
                &decorations[line],
                &[text.len()..text.len()],
                false,
            );
            assert_eq!(
                display_width(&collapsed[..collapsed_map[info.value_range.start]]),
                display_width(&active[..active_map[info.value_range.start]]),
                "line {line}"
            );
            assert_eq!(
                &active[active_map[info.value_range.start]..],
                &text[info.value_range.start..]
            );
        }
    }

    #[test]
    fn front_matter_blank_multiline_body_has_the_same_value_column_for_caret_and_ime() {
        let source = "---\nnested:\n  body: |-\n\n    中文\n    \n  next: scalar\n---";
        let buffer = Buffer::from_text(source);
        let decorations = markdown::parse_decorations(source);
        let plans = front_matter_group_plan(&buffer, &decorations, &[], true);
        let plan = &plans[0];
        assert_eq!(plan.label_line(), 3);
        let mut expected_column = None;
        for line in 3..6 {
            let text = buffer.line_content(line);
            let mut decoration = decorations[line].clone();
            apply_front_matter_group_display(&mut decoration, line, Some(plan), text);
            let value_start = decoration.front_matter.as_ref().unwrap().value_range.start;
            for active in [vec![], vec![0..text.len()]] {
                let (display, _, map, _) =
                    front_matter_test_display(text, &decoration, &active, false);
                let column = display_width(&display[..map[value_start]]);
                if text.is_empty() {
                    assert_eq!(column, 0);
                    assert_eq!(display, "");
                    continue;
                }
                assert_eq!(*expected_column.get_or_insert(column), column);
                assert_eq!(map[text.len()], display.len());
            }
        }
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn front_matter_list_hits_map_decoded_utf8_characters_to_exact_absolute_source() {
        for (token, display) in [
            ("\"中\\u263a\\\"🌸\"", "中☺\"🌸"),
            ("'中''文'", "中'文"),
            ("你好🌸", "你好🌸"),
        ] {
            let start = 100;
            let map = front_matter_item_source_map(token, display, start);
            assert_eq!(map.len(), display.len() + 1);
            assert!(
                map.iter()
                    .all(|offset| token.is_char_boundary(offset - start))
            );
            for (index, ch) in display.char_indices() {
                assert!(
                    map[index..index + ch.len_utf8()]
                        .iter()
                        .all(|offset| *offset == map[index])
                );
            }
            assert_eq!(map[display.len()], start + token.len());
            if let Some(escaped) = token.find("\\u") {
                assert_eq!(map[display.find('☺').unwrap()], start + escaped);
            }
        }
    }

    #[test]
    fn front_matter_collapsed_list_selection_projects_source_escapes_without_splitting_utf8() {
        let token = "\"中\\u263a🌸\"";
        let display = "中☺🌸";
        let map = front_matter_item_source_map(token, display, 100);
        let escape = 100 + token.find("\\u").unwrap();
        assert_eq!(
            super::front_matter_display_selection_ranges(display, &map, &[escape + 1..escape + 2]),
            vec!["中".len().."中☺".len()]
        );
        assert_eq!(
            super::front_matter_display_selection_ranges(display, &map, &[100..100 + token.len()]),
            vec![0..display.len()]
        );
        assert!(
            super::front_matter_display_selection_ranges(display, &map, &[escape..escape])
                .is_empty()
        );
    }

    #[test]
    fn front_matter_empty_block_header_does_not_disappear_and_plain_first_value_is_retained() {
        for source in [
            "---\nbody: |-\nnext: value\n---",
            "---\nbody: first\n  second\nnext: value\n---",
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = markdown::parse_decorations(source);
            let plans = front_matter_group_plan(&buffer, &decorations, &[], true);
            assert!(!plans[0].hides_line(1));
            assert_eq!(plans[0].label_line(), 1);
        }
    }

    #[test]
    fn front_matter_unicode_mappings_keep_character_identity_and_token_bounds() {
        let source = "---\n\"键:名\": \"中\\u263a\\\"x\" # 评论\nflag: true # 开关\nlist:\n  - '中''文'\n---";
        let decorations = markdown::parse_decorations(source);
        let lines: Vec<_> = source.lines().collect();
        for index in 0..lines.len() {
            for active in [vec![], vec![lines[index].len()..lines[index].len()]] {
                let (display, d2b, b2d, _) =
                    front_matter_test_display(lines[index], &decorations[index], &active, true);
                assert_eq!(d2b.len(), display.len());
                assert_eq!(b2d.len(), lines[index].len() + 1);
                assert!(
                    d2b.iter()
                        .all(|offset| lines[index].is_char_boundary(*offset))
                );
                assert!(b2d.iter().all(|offset| display.is_char_boundary(*offset)));
                assert_eq!(b2d[lines[index].len()], display.len());
                for (position, ch) in lines[index].char_indices() {
                    // Exact identity for unchanged key and comment characters.
                    let info = decorations[index].front_matter.as_ref();
                    if info.is_some_and(|info| {
                        info.key_range
                            .as_ref()
                            .is_some_and(|r| r.contains(&position))
                            || position >= info.value_range.end
                    }) {
                        assert_eq!(display[b2d[position]..].chars().next(), Some(ch));
                        assert_eq!(d2b[b2d[position]], position);
                    }
                }
            }
        }
        let info = decorations[1].front_matter.as_ref().unwrap();
        let (display, d2b, b2d, _) =
            front_matter_test_display(lines[1], &decorations[1], &[], false);
        let value_display = b2d[info.value_range.start]..b2d[info.value_range.end];
        assert_eq!(&display[value_display.clone()], "中☺\"x");
        let smile = display.find('☺').unwrap();
        assert_eq!(&lines[1][d2b[smile]..d2b[smile] + 6], "\\u263a");
        assert_eq!(
            d2b[display.find('中').unwrap()],
            lines[1].find('中').unwrap()
        );
        let info = decorations[2].front_matter.as_ref().unwrap();
        assert_eq!(&lines[2][info.value_range.clone()], "true");
        let (display, d2b, b2d, _) =
            front_matter_test_display(lines[2], &decorations[2], &[], false);
        let range = b2d[info.value_range.start]..b2d[info.value_range.end];
        assert_eq!(&display[range.clone()], "\u{a0}".repeat(3));
        assert!(
            d2b[range.clone()]
                .iter()
                .all(|i| *i == info.value_range.start)
        );
        assert_eq!(&display[range.end..], " # 开关");
        let (display, d2b, _, _) = front_matter_test_display(lines[4], &decorations[4], &[], false);
        assert_eq!(d2b[display.find('•').unwrap()], lines[4].find('-').unwrap());
        assert_eq!(
            d2b[display.find('中').unwrap()],
            lines[4].find('中').unwrap()
        );
    }

    #[test]
    fn front_matter_nested_block_bodies_align_and_indented_delimiters_are_literal() {
        let source = "---\nnested:\n  body: |2- # header\n    1. **literal**\n    ---\n    true\n  items:\n    - name: 'record'\n      body: >-\n        2026-10-12\n        false\n---";
        let decorations = markdown::parse_decorations(source);
        let lines: Vec<_> = source.lines().collect();
        assert_eq!(
            decorations[1].table_info.as_ref().unwrap().table_end_line,
            lines.len()
        );
        let display = |index: usize| {
            front_matter_test_display(lines[index], &decorations[index], &[], true).0
        };
        let value_start = |index: usize| {
            let info = decorations[index].front_matter.as_ref().unwrap();
            front_matter_test_display(lines[index], &decorations[index], &[], true).2
                [info.value_range.start]
        };
        assert_eq!(value_start(2), value_start(3));
        assert_eq!(value_start(8), value_start(9));
        assert_eq!(display(4).trim(), "---");
        assert_eq!(display(5).trim(), "true");
        assert_eq!(display(9).trim(), "2026-10-12");
        assert_eq!(display(10).trim(), "false");
        assert!(display(7).contains("• name:"));
        for index in [3, 4, 5, 9, 10] {
            assert!(
                decorations[index]
                    .front_matter
                    .as_ref()
                    .unwrap()
                    .key_range
                    .is_none()
            );
            let (expanded, _, b2d, _) = front_matter_test_display(
                lines[index],
                &decorations[index],
                &[lines[index].len()..lines[index].len() + 1],
                false,
            );
            let info = decorations[index].front_matter.as_ref().unwrap();
            assert_eq!(
                &expanded[b2d[info.value_range.start]..],
                &lines[index][info.value_range.start..]
            );
            assert_eq!(b2d[info.value_range.start], value_start(index));
        }
    }

    #[test]
    fn front_matter_formatted_dates_and_newline_scalars_have_safe_mappings() {
        let source = "---\ndate: '2026-10-12' # 日期\ntext: \"first\\nsecond\"\n---";
        let mut decorations = markdown::parse_decorations(source);
        let line = source.lines().nth(1).unwrap();
        assert_eq!(
            decorations[1].front_matter.as_ref().unwrap().value,
            FrontMatterValue::DateTime("2026-10-12".into())
        );
        decorations[1].front_matter.as_mut().unwrap().value =
            FrontMatterValue::Text("2026年10月12日".into());
        let info = decorations[1].front_matter.as_ref().unwrap();
        let (display, d2b, b2d, _) = front_matter_test_display(line, &decorations[1], &[], false);
        assert_eq!(
            &display[b2d[info.value_range.start]..b2d[info.value_range.end]],
            "2026年10月12日"
        );
        assert!(d2b.iter().all(|i| line.is_char_boundary(*i)));
        assert!(b2d.iter().all(|i| display.is_char_boundary(*i)));
        let expanded =
            front_matter_test_display(line, &decorations[1], &[line.len()..line.len()], false).0;
        assert!(expanded.contains("'2026-10-12' # 日期"));
        let line = source.lines().nth(2).unwrap();
        let display = front_matter_test_display(line, &decorations[2], &[], false).0;
        assert!(display.contains("\"first\\nsecond\""));
        assert!(!display.contains('\n'));
    }

    #[test]
    fn front_matter_incomplete_and_collection_rows_remain_visible() {
        for source in [
            "---\ntitle: \"unfinished\ncontinuation: still quote\n---",
            "-\ncategory: 入门教程\ndescription: 介绍小语文稿的使用\nefefef\n-",
            "---\nflow: [one,\n  {key: true}]\n---",
        ] {
            let decorations = markdown::parse_decorations(source);
            for (index, line) in source.lines().enumerate() {
                let (display, d2b, b2d, _) =
                    front_matter_test_display(line, &decorations[index], &[], false);
                if let Some(info) = decorations[index].front_matter.as_ref()
                    && let Some(key) = info.key_range.as_ref()
                {
                    assert_eq!(&display[b2d[key.start]..b2d[key.end]], &line[key.clone()]);
                    assert_eq!(
                        &display[b2d[info.value_range.start]..],
                        &line[info.value_range.start..]
                    );
                } else {
                    assert_eq!(
                        display, line,
                        "unsupported row {index} stays source-visible"
                    );
                }
                assert!(d2b.iter().all(|offset| line.is_char_boundary(*offset)));
                assert!(b2d.iter().all(|offset| display.is_char_boundary(*offset)));
            }
        }
    }

    #[test]
    fn front_matter_keys_bullets_booleans_and_delimiters_have_metadata_styles() {
        let source = "---\n\"键:名\": 'value'\nflag: false # switch\ntags:\n  - \"item: value\"\nbody: |-\n  literal: **body**\n---";
        let decorations = markdown::parse_decorations(source);
        let lines: Vec<_> = source.lines().collect();
        let theme = Theme::one_dark();
        let (display, runs) = front_matter_test_runs(lines[1], &decorations[1], &[], true, &theme);
        assert_eq!(
            run_at_byte(&runs, display.find('名').unwrap()).color,
            theme.text_bright
        );
        assert_eq!(
            run_at_byte(&runs, display.find("value").unwrap()).color,
            theme.text_primary
        );
        let (display, runs) = front_matter_test_runs(lines[2], &decorations[2], &[], true, &theme);
        assert_eq!(
            run_at_byte(&runs, display.find('\u{a0}').unwrap()).color.a,
            0.0
        );
        assert_eq!(
            run_at_byte(&runs, display.find('#').unwrap()).color,
            theme.text_primary
        );
        let (display, runs) = front_matter_test_runs(lines[4], &decorations[4], &[], true, &theme);
        assert_eq!(
            run_at_byte(&runs, display.find('•').unwrap()).color,
            theme.list_marker
        );
        assert_eq!(
            run_at_byte(&runs, display.find("item").unwrap())
                .font
                .weight,
            FontWeight::NORMAL
        );
        let (display, runs) = front_matter_test_runs(lines[6], &decorations[6], &[], true, &theme);
        for offset in [
            display.find("literal").unwrap(),
            display.find("body").unwrap(),
        ] {
            assert_eq!(run_at_byte(&runs, offset).color, theme.text_primary);
            assert_eq!(run_at_byte(&runs, offset).font.weight, FontWeight::NORMAL);
        }
        let (_, runs) = front_matter_test_runs(lines[0], &decorations[0], &[], true, &theme);
        assert!(
            runs[0].color.a > 0.0,
            "a focused front matter block shows its delimiters"
        );
        let (_, runs) = front_matter_test_runs(lines[0], &decorations[0], &[], false, &theme);
        assert_eq!(
            runs[0].color.a, 0.0,
            "an unfocused delimiter keeps measured geometry with transparent ink"
        );
    }

    #[test]
    fn front_matter_values_align_after_longest_key() {
        let source = "---\ntitle: Moeka\ndescription: Markdown editor\n---";
        let decorations = markdown::parse_decorations(source);
        let displays = [1, 2].map(|line| {
            build_display_text(
                source.lines().nth(line).unwrap(),
                &decorations[line],
                &[],
                false,
                None,
                0,
                0,
                0,
                0,
                1,
                1.0,
                1.0,
                1.0,
            )
            .0
        });

        assert_eq!(displays[0], "title:       Moeka");
        assert_eq!(displays[1], "description: Markdown editor");
        assert_eq!(displays[0].find("Moeka"), displays[1].find("Markdown"));
    }

    #[test]
    fn front_matter_keys_are_brighter_and_bold_while_values_use_body_color() {
        let source = "---\nfirst: one\nsecond: two\nthird: three\n---";
        let decorations = markdown::parse_decorations(source);
        let theme = Theme::one_dark();
        let key_color = crate::theme::with_alpha(theme.text_bright, 1.0);
        let build_runs = |line_index, active| {
            let line = source.lines().nth(line_index).unwrap();
            let decoration = &decorations[line_index];
            let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
                line,
                decoration,
                &[],
                false,
                None,
                0,
                0,
                0,
                0,
                1,
                1.0,
                1.0,
                1.0,
            );
            build_text_runs_for_display(
                &display_text,
                line,
                &display_to_buffer,
                decoration,
                &[],
                active,
                prefix_collapsed,
                &font(".SystemUIFont"),
                &font(".SystemUIFont"),
                &font("Menlo"),
                &font(".SystemUIFont"),
                &font(".SystemUIFont"),
                &font(".SystemUIFont"),
                &font("Kaiti SC"),
                0,
                16.0,
                &theme,
                theme.git_status_added,
                theme.text_danger,
                theme.link,
                FontWeight::NORMAL,
                theme.list_marker,
                FontWeight::BOLD,
                theme.syntax_marker,
                Some(key_color),
                false,
            )
        };
        let style_at = |runs: &[TextRun], display_index: usize| {
            let mut start = 0;
            runs.iter()
                .find_map(|run| {
                    let end = start + run.len;
                    let contains = display_index >= start && display_index < end;
                    start = end;
                    contains.then_some((run.color, run.font.weight, run.font.style))
                })
                .expect("display index has a text run")
        };

        for line_index in 1..=3 {
            for active in [false, true] {
                let runs = build_runs(line_index, active);
                let line = source.lines().nth(line_index).unwrap();
                let value = line.split_once(':').unwrap().1.trim();
                let value_index = line.find(value).unwrap();
                assert_eq!(
                    style_at(&runs, 0),
                    (key_color, FontWeight::BOLD, FontStyle::Normal)
                );
                assert_eq!(
                    style_at(&runs, value_index),
                    (theme.text_primary, FontWeight::NORMAL, FontStyle::Normal)
                );
            }
        }
    }

    #[test]
    fn folded_quote_text_uses_normal_text_color() {
        let line = "> plain quote";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &[],
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, "plain quote");
        assert!(runs.iter().any(|run| run.color == theme.text_primary));
        assert!(runs.iter().all(|run| run.color != theme.syntax_marker));
    }

    fn table_cell_parser_parity_parse(
        content: &str,
        definitions: &str,
    ) -> (String, LineDecoration) {
        let source =
            format!("| Neighbor | Input |\n| --- | --- |\n| 隣🙂 | {content}|\n\n{definitions}");
        let decorations = markdown::parse_decorations(&source);
        let row = &decorations[2];
        let info = row.table_info.as_ref().expect("input remains a table row");
        let (content, range) = super::super::table_cell_render::content_with_range(
            info,
            source.lines().nth(2).unwrap(),
            1,
        );
        let decoration = table_cell_decoration(row, &content, &range);
        assert_table_cell_inline_ranges(&content, &decoration);
        (content, decoration)
    }

    fn table_cell_parser_parity_display(
        content: &str,
        decoration: &LineDecoration,
        active: &[std::ops::Range<usize>],
    ) -> String {
        build_display_text(
            content, decoration, active, false, None, 0, 3, 3, 2, 0, 1.0, 1.0, 1.0,
        )
        .0
    }

    #[test]
    fn table_cell_parser_parity_reference_boundary_triggers() {
        let definitions = "[ref]: https://reference.test\n[pic]: photo.png\n";
        for input in [
            "[label][ref]",
            "[ref]",
            "[ref][]",
            "![photo][pic]",
            "![pic]",
            "![pic][]",
        ] {
            for padding in ["", " "] {
                let (content, cell) =
                    table_cell_parser_parity_parse(&format!("{input}{padding}"), definitions);
                let normal =
                    markdown::parse_decorations(&format!("{content}\n\n{definitions}")).remove(0);
                assert_eq!(cell.inline_spans, normal.inline_spans, "{content:?}");
                assert!(!cell.syntax_ranges.is_empty(), "{content:?}");
                assert!(
                    cell.syntax_ranges
                        .iter()
                        .all(|syntax| syntax.trigger_range.start == 0)
                );
                for active in [vec![], vec![1..1], vec![0..content.len()]] {
                    assert_eq!(
                        table_cell_parser_parity_display(&content, &cell, &active),
                        table_cell_parser_parity_display(&content, &normal, &active),
                        "{content:?}, {active:?}",
                    );
                }
            }
        }
    }

    #[test]
    fn table_cell_parser_parity_reference_identifiers_are_opaque() {
        for identifier in [
            ":rocket:",
            "x^super^",
            "x~sub~",
            "==ref==",
            "++ref++",
            "**ref**",
            "`code`",
            "$x$",
            "https://label.test",
        ] {
            let input = format!("a [label][{identifier}]");
            let definitions = format!("[{identifier}]: https://reference.test\n");
            let (content, cell) = table_cell_parser_parity_parse(&input, &definitions);
            let normal =
                markdown::parse_decorations(&format!("{content}\n\n{definitions}")).remove(0);
            assert_eq!(cell.inline_spans, normal.inline_spans, "{identifier:?}");
            assert!(cell.display_replacements.is_empty(), "{identifier:?}");
            assert!(cell.highlight_spans.is_empty(), "{identifier:?}");
            assert!(cell.superscript_spans.is_empty(), "{identifier:?}");
            assert!(cell.subscript_spans.is_empty(), "{identifier:?}");
            assert!(cell.inline_math_spans.is_empty(), "{identifier:?}");
            for active in [vec![], vec![3..3], vec![0..content.len()]] {
                assert_eq!(
                    table_cell_parser_parity_display(&content, &cell, &active),
                    table_cell_parser_parity_display(&content, &normal, &active),
                    "{identifier:?}, {active:?}",
                );
            }
        }
        for input in [
            "[:rocket:][ref]",
            "a [==label==][ref]",
            "a [++label++][ref]",
        ] {
            let definitions = "[ref]: https://reference.test\n";
            let (content, cell) = table_cell_parser_parity_parse(input, definitions);
            let normal =
                markdown::parse_decorations(&format!("{content}\n\n{definitions}")).remove(0);
            assert_eq!(cell.highlight_spans, normal.highlight_spans);
            assert_eq!(cell.display_replacements, normal.display_replacements);
            assert_eq!(
                table_cell_parser_parity_display(&content, &cell, &[]),
                table_cell_parser_parity_display(&content, &normal, &[]),
            );
        }
    }

    #[test]
    fn table_cell_parser_parity_references_after_protected_pipes() {
        let definitions = "[ref]: https://reference.test\n[pic]: photo.png\n";
        for prefix in ["[[notes/page.md|Page]]", "`a|b`", r"a\|b"] {
            let input = format!("{prefix} a [label][ref] ![photo][pic]");
            let source = format!("| h |\n| --- |\n| {input} |\n\n{definitions}");
            let rows = markdown::parse_decorations(&source);
            let row = &rows[2];
            let cell_start = row.table_info.as_ref().unwrap().cell_ranges[0].start;
            assert_eq!(
                markdown::link_target_at_column(row, cell_start + input.find("label").unwrap()),
                Some("https://reference.test"),
                "row-level navigation metadata: {prefix:?}",
            );
            let (content, cell) = table_cell_parser_parity_parse(&input, definitions);
            let normal =
                markdown::parse_decorations(&format!("{content}\n\n{definitions}")).remove(0);
            assert_eq!(cell.inline_spans, normal.inline_spans, "{prefix:?}");
            for active in [
                vec![],
                vec![content.find("label").unwrap()..content.find("label").unwrap()],
            ] {
                assert_eq!(
                    table_cell_parser_parity_display(&content, &cell, &active),
                    table_cell_parser_parity_display(&content, &normal, &active),
                    "{prefix:?}, {active:?}",
                );
            }
        }
    }

    #[test]
    fn table_cell_parser_parity_row_links_without_reference_definitions() {
        for prefix in [
            "[[notes/page.md|Page]]",
            "`a|b`",
            "`[fake](https://fake.test)|raw`",
            "`https://fake.test|raw`",
            "`a|b` $[fake](https://fake.test)$",
            "`a|b` $$[fake](https://fake.test)$$",
            "`a|b` $[[fake/path|Fake]]$",
            "`a|b` $$[[fake/path|Fake]]$$",
        ] {
            for link in [
                "[real](https://real.test)",
                "<https://real.test>",
                "https://real.test",
                "<person@example.test>",
            ] {
                let input = format!("{prefix} plain {link}");
                let source = format!("| h |\n| --- |\n| {input} |\n| later |\n");
                let rows = markdown::parse_decorations(&source);
                let row = &rows[2];
                assert!(row.table_reference_definitions.is_none());
                let info = row.table_info.as_ref().unwrap();
                let cell = table_cell_decoration(row, &info.cells[0], &info.cell_ranges[0]);
                let cell_start = info.cell_ranges[0].start;
                for column in input.char_indices().map(|(column, _)| column) {
                    assert_eq!(
                        markdown::link_target_at_column(row, cell_start + column),
                        markdown::link_target_at_column(&cell, column),
                        "{input:?} at {column}",
                    );
                }
                assert!(
                    !row.inline_spans.iter().any(|span| {
                        span.kind == markdown::InlineKind::Link
                            && span
                                .target
                                .as_deref()
                                .is_some_and(|target| target.contains("fake.test"))
                    }),
                    "code contents must not acquire command-click targets: {input:?}"
                );
            }
        }
    }

    #[test]
    fn table_cell_parser_parity_row_bare_urls_do_not_capture_neighboring_cells() {
        for source in [
            "| A | B | C |\n| --- | --- | --- |\n| https://real.test | plain | [[notes/page.md|Page]] |",
            "|A|B|C|\n|---|---|---|\n|https://real.test|plain|[[notes/page.md|Page]]|",
            "> |A|B|C|\n> |---|---|---|\n> |https://real.test|plain|[[notes/page.md|Page]]|",
        ] {
            let rows = markdown::parse_decorations(source);
            let row = &rows[2];
            let info = row.table_info.as_ref().unwrap();
            for (content, range) in info.cells.iter().zip(&info.cell_ranges) {
                let cell = table_cell_decoration(row, content, range);
                for column in content.char_indices().map(|(column, _)| column) {
                    assert_eq!(
                        markdown::link_target_at_column(row, range.start + column),
                        markdown::link_target_at_column(&cell, column),
                        "{source:?}: {content:?} at {column}",
                    );
                }
            }
        }
    }

    #[test]
    fn table_cell_parser_parity_reference_context_preserves_control_character_uris() {
        for uri in [
            "https://reference.test/a&#10;b",
            "https://reference.test/a&#13;b",
            "https://reference.test/a&#13;&#10;b",
            "https://reference.test/a&#9;b",
            "https://reference.test/a&#32;b",
            "https://reference.test/a&#12;b",
            "https://reference.test/a&amp;amp;b?x=%26%20#片段",
            r"https://reference.test/a\\b\<c\>d",
            "",
        ] {
            let input = "[label][ref] ![photo][ref]";
            let definitions = format!("[ref]: <{uri}>\n");
            let (content, cell) = table_cell_parser_parity_parse(input, &definitions);
            let normal =
                markdown::parse_decorations(&format!("{content}\n\n{definitions}")).remove(0);
            assert_eq!(normal.inline_spans.len(), 2, "valid reference: {uri:?}");
            assert_eq!(cell.inline_spans, normal.inline_spans, "{uri:?}");
            let source = format!("| h |\n| --- |\n| [[page|Page]] {input} |\n\n{definitions}");
            let rows = markdown::parse_decorations(&source);
            let row = &rows[2];
            let range = &row.table_info.as_ref().unwrap().cell_ranges[0];
            assert_eq!(
                markdown::link_target_at_column(row, range.start + "[[page|Page]] [".len()),
                normal.inline_spans[0].target.as_deref(),
                "row URI: {uri:?}",
            );
        }
    }

    #[test]
    fn table_cell_parser_parity_break_tag_spellings() {
        for (tag, valid) in [
            ("<br>", true),
            ("<BR>", true),
            ("<br/>", true),
            ("<Br \t/>", true),
            ("<br >", true),
            ("<br\t>", true),
            ("<br\u{c}>", true),
            ("< br>", false),
            ("< br >", false),
            ("<br/ >", false),
            ("<br / >", false),
            ("<br\u{a0}>", false),
            ("<br\u{b}>", false),
            ("<br//>", false),
            ("</br>", false),
            ("<brx>", false),
        ] {
            let input = format!("a{tag}b<br>tail");
            let (content, cell) = table_cell_parser_parity_parse(&input, "");
            let display = table_cell_parser_parity_display(&content, &cell, &[]);
            let expected = if valid {
                "a\nb\ntail".to_string()
            } else {
                format!("a{tag}b\ntail")
            };
            assert_eq!(display, expected, "{tag:?}");
            assert_eq!(cell.table_cell_fragments.len(), if valid { 3 } else { 2 });
        }
    }

    #[test]
    fn table_cell_parser_parity_reference_context_preserves_normalization_and_urls() {
        for (reference, definition) in [
            ("STRASSE", "[straße]: https://reference.test"),
            ("a   b", "[a b]: https://reference.test"),
            (
                "ref",
                "[ref]: <https://reference.test/a&amp;amp;b\\>c> \"title\"",
            ),
            (
                "ref",
                "[ref]:\n  <https://reference.test/a&amp;b>\n  \"title\"",
            ),
            ("ref", "> [ref]: https://reference.test/quoted"),
            (r"a\]b", r"[a\]b]: https://reference.test/escaped-bracket"),
            (r"a\\b", r"[a\\b]: https://reference.test/backslash"),
            ("a&amp;b", "[a&amp;b]: https://reference.test/entity-label"),
            ("片段", "[片段]: https://reference.test/日本語#片段"),
            ("a b", "[a\n  b]: https://reference.test/multiline-label"),
            (
                "ref",
                "[ref]: https://reference.test/first\n[ref]: https://reference.test/second",
            ),
        ] {
            let input = format!("[[notes/page.md|Page]] [label][{reference}]");
            let (content, cell) = table_cell_parser_parity_parse(&input, definition);
            let normal =
                markdown::parse_decorations(&format!("{content}\n\n{definition}")).remove(0);
            assert!(
                normal
                    .inline_spans
                    .iter()
                    .any(|span| span.kind == markdown::InlineKind::Link)
            );
            assert_eq!(cell.inline_spans, normal.inline_spans, "{definition:?}");
            assert_eq!(
                table_cell_parser_parity_display(&content, &cell, &[]),
                table_cell_parser_parity_display(&content, &normal, &[]),
            );
        }
    }

    #[test]
    fn table_cell_parser_parity_breaks_exclude_link_and_image_tails() {
        for input in [
            "[label](https://reference.test \"<br>- [x] literal\")<br>tail :rocket:",
            "![photo](photo.png \"<br>- [x] literal\")<br>tail :rocket:",
            "[label][a<br>b]<br>tail :rocket:",
            "![photo][a<br>b]<br>tail :rocket:",
        ] {
            let definitions = "[a<br>b]: https://reference.test\n";
            let (content, cell) = table_cell_parser_parity_parse(input, definitions);
            let accepted = content.rfind("<br>").unwrap();
            assert_eq!(cell.table_cell_fragments.len(), 2, "{input:?}");
            assert_eq!(cell.table_cell_fragments[0].range.end, accepted);
            assert!(
                cell.table_cell_fragments
                    .iter()
                    .all(|fragment| fragment.prefix_len == 0)
            );
            let folded = table_cell_parser_parity_display(&content, &cell, &[]);
            assert!(folded.ends_with("\ntail 🚀"), "{folded:?}");
            assert_eq!(folded.matches('\n').count(), 1);
            let expanded = table_cell_parser_parity_display(&content, &cell, &[1..1]);
            assert!(expanded.contains("<br>"), "{expanded:?}");
            assert_eq!(expanded.matches('\n').count(), 1);
            assert!(expanded.ends_with("\ntail 🚀"));
        }
        for input in [
            "[a<br>b](https://reference.test)<br>tail",
            "![a<br>b](photo.png)<br>tail",
        ] {
            let (content, cell) = table_cell_parser_parity_parse(input, "");
            assert_eq!(cell.table_cell_fragments.len(), 3);
            assert_eq!(
                table_cell_parser_parity_display(&content, &cell, &[]),
                "a\nb\ntail"
            );
        }
    }

    #[test]
    fn table_cell_parser_parity_footnote_breaks_do_not_block_later_replacements() {
        let (content, cell) = table_cell_parser_parity_parse("[^a<br>b]<br>**tail** :rocket:", "");
        assert_eq!(cell.footnote_references.len(), 1);
        assert_eq!(cell.table_cell_fragments.len(), 2);
        assert_eq!(cell.table_cell_fragments[0].range, 0..9);
        assert_eq!(
            table_cell_parser_parity_display(&content, &cell, &[]),
            "      \ntail 🚀",
        );
        assert_eq!(
            table_cell_parser_parity_display(&content, &cell, &[3..3]),
            "[^a<br>b]\ntail 🚀",
        );
    }

    #[test]
    fn table_cell_parser_parity_non_list_blocks_keep_existing_semantics() {
        for input in [
            "# **heading**",
            "> **quote**",
            "---",
            "```rust",
            "[^note]: **body**",
        ] {
            let baseline =
                table_cell_decoration(&LineDecoration::default(), input, &(0..input.len()));
            let (_, cell) =
                table_cell_parser_parity_parse(input, "[ref]: https://reference.test\n");
            assert_eq!(cell, baseline, "{input:?}");
        }
        let content = "$$x^2$$";
        let standalone =
            table_cell_decoration(&LineDecoration::default(), content, &(0..content.len()));
        assert!(standalone.block_preview.is_some());
        assert_eq!(standalone.block_type, markdown::BlockType::CodeBlockFence);
    }

    #[test]
    fn table_cell_parser_parity_display_math_keeps_table_ownership() {
        for source in [
            "| h |\n| --- |\n| $$x^2$$ |\n| later |",
            "| $$x^2$$ |\n| --- |\n| later |",
            "| A | B |\n| --- | --- |\n| `a|b` $$x$$ $$y$$ | $$z$$ |\n| later | row |",
            "| $$a$$ | $$b$$ |\n| --- | --- |\n| $$c$$ | $$d$$ |\n| later | row |",
            "> | h |\n> | --- |\n> | $$x^2$$ |\n> | later |",
        ] {
            let decorations = markdown::parse_decorations(source);
            for (line, row) in decorations.iter().enumerate() {
                let info = row
                    .table_info
                    .as_ref()
                    .expect("all source rows stay in the table");
                assert_eq!(row.block_type, markdown::BlockType::Table);
                assert!(row.block_preview.is_none());
                assert!(row.code_language.is_none());
                assert_eq!(info.table_end_line, decorations.len());
                if info.is_separator {
                    continue;
                }
                for (content, range) in info.cells.iter().zip(&info.cell_ranges) {
                    let cell = table_cell_decoration(row, content, range);
                    assert_table_cell_inline_ranges(content, &cell);
                    assert!(cell.block_preview.is_none(), "{line}: {content:?}");
                    assert!(cell.code_language.is_none());
                    assert_eq!(
                        cell.inline_math_spans.len(),
                        content.matches("$$").count() / 2
                    );
                    for syntax in &cell.syntax_ranges {
                        assert!(!content[syntax.range.clone()].contains("x^2"));
                    }
                }
            }
        }
        for source in [
            "$$x^2$$",
            "text $$x^2$$",
            "$$\n| h |\n| --- |\n| literal |\n$$",
            "\\[\n| h |\n| --- |\n| literal |\n\\]",
            "```math\n| h |\n| --- |\n| literal |\n```",
            "```text\n| h |\n| --- |\n| $$literal$$ |\n```",
        ] {
            let decorations = markdown::parse_decorations(source);
            assert_eq!(
                decorations[0].block_preview.is_some(),
                !source.starts_with("```"),
                "{source:?}",
            );
            assert_eq!(
                decorations[0].block_type,
                markdown::BlockType::CodeBlockFence
            );
            assert!(decorations.iter().all(|row| row.table_info.is_none()));
        }
    }

    #[test]
    fn table_cells_keep_inline_syntax_and_document_reference_targets() {
        let source = "| inline | references |\n| --- | --- |\n| **bold** *italic* ~~gone~~ `code` [link](https://inline.test) ![img](inline.png) | [reference][ref] ![photo][pic] |\n\n[ref]: https://reference.test\n[pic]: photo.png\n";
        let decorations = markdown::parse_decorations(source);
        let row = &decorations[2];
        let table_info = row.table_info.as_ref().expect("table row");

        let inline = table_cell_decoration(row, &table_info.cells[0], &table_info.cell_ranges[0]);
        for kind in [
            markdown::InlineKind::Bold,
            markdown::InlineKind::Italic,
            markdown::InlineKind::Strikethrough,
            markdown::InlineKind::InlineCode,
            markdown::InlineKind::Link,
            markdown::InlineKind::Image,
        ] {
            assert!(
                inline.inline_spans.iter().any(|span| span.kind == kind),
                "missing inline kind: {kind:?}"
            );
        }

        let references =
            table_cell_decoration(row, &table_info.cells[1], &table_info.cell_ranges[1]);
        assert!(references.inline_spans.iter().any(|span| {
            span.kind == markdown::InlineKind::Link
                && span.target.as_deref() == Some("https://reference.test")
        }));
        assert!(references.inline_spans.iter().any(|span| {
            span.kind == markdown::InlineKind::Image && span.target.as_deref() == Some("photo.png")
        }));
    }

    fn table_cell_inline_display_and_runs(
        content: &str,
        decoration: &LineDecoration,
    ) -> (String, Vec<usize>, Vec<usize>, Vec<TextRun>) {
        let (display, d2b, b2d, collapsed) = build_display_text(
            content,
            decoration,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display,
            content,
            &d2b,
            decoration,
            &[],
            false,
            collapsed,
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
            theme.git_status_added,
            theme.text_danger,
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );
        assert_eq!(d2b.len(), display.len());
        assert_eq!(b2d.len(), content.len() + 1);
        assert_eq!(b2d[content.len()], display.len());
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), display.len());
        for (source, _) in content.char_indices() {
            assert!(display.is_char_boundary(b2d[source]));
        }
        for (shown, _) in display.char_indices() {
            assert!(content.is_char_boundary(d2b[shown]));
        }
        (display, d2b, b2d, runs)
    }

    fn assert_table_cell_inline_ranges(content: &str, decoration: &LineDecoration) {
        let mut ranges = Vec::new();
        ranges.extend(decoration.inline_spans.iter().map(|span| &span.range));
        ranges.extend(&decoration.inline_math_spans);
        ranges.extend(&decoration.protected_ranges);
        ranges.extend(&decoration.highlight_spans);
        ranges.extend(&decoration.superscript_spans);
        ranges.extend(&decoration.subscript_spans);
        for syntax in &decoration.syntax_ranges {
            ranges.extend([&syntax.range, &syntax.trigger_range]);
        }
        for replacement in &decoration.display_replacements {
            ranges.extend([&replacement.source_range, &replacement.trigger_range]);
        }
        for wiki in &decoration.wiki_links {
            ranges.extend([&wiki.full_range, &wiki.collapsed_range]);
        }
        for footnote in &decoration.footnote_references {
            ranges.extend([&footnote.full_range, &footnote.collapsed_range]);
        }
        for range in ranges {
            assert!(
                range.start <= range.end && range.end <= content.len(),
                "{range:?}"
            );
            assert!(content.is_char_boundary(range.start), "{range:?}");
            assert!(content.is_char_boundary(range.end), "{range:?}");
        }
    }

    #[test]
    fn table_cell_leading_break_keeps_bold_code_wiki_and_checked_body_styles() {
        let source = "| Neighbor | Input |\n| --- | --- |\n| 隣🙂 | <br>- [x] **完🙂** `code` [[notes/page.md|頁]]<br>- [ ] open |";
        let decorations = markdown::parse_decorations(source);
        let row = &decorations[2];
        let info = row.table_info.as_ref().unwrap();
        let content = &info.cells[1];
        let decoration = table_cell_decoration(row, content, &info.cell_ranges[1]);
        assert_eq!(decoration.table_cell_fragments.len(), 3);
        assert_eq!(decoration.table_cell_fragments[0].range, 0..0);
        assert_eq!(
            decoration.table_cell_fragments[1].block_type,
            markdown::BlockType::TaskList(true)
        );
        assert_eq!(decoration.table_cell_fragments[1].prefix_len, 6);
        assert_eq!(
            decoration.table_cell_fragments[2].block_type,
            markdown::BlockType::TaskList(false)
        );
        for (kind, word) in [
            (markdown::InlineKind::Bold, "完🙂"),
            (markdown::InlineKind::InlineCode, "code"),
        ] {
            let span = decoration
                .inline_spans
                .iter()
                .find(|span| span.kind == kind)
                .expect("leading break must not suppress inline parsing");
            assert_eq!(&content[span.range.clone()], word);
        }
        assert_eq!(decoration.wiki_links.len(), 1);
        let wiki = &decoration.wiki_links[0];
        assert_eq!(wiki.target_path, "notes/page.md");
        assert_eq!(&content[wiki.full_range.clone()], "[[notes/page.md|頁]]");
        assert_eq!(&content[wiki.collapsed_range.clone()], "頁");
        assert_table_cell_inline_ranges(content, &decoration);
        let (display, d2b, b2d, runs) = table_cell_inline_display_and_runs(content, &decoration);
        assert_eq!(display, "\n   完🙂 code 頁\n   open");
        for word in ["完🙂", "code", "頁", "open"] {
            let source = content.find(word).unwrap();
            let shown = display.find(word).unwrap();
            for relative in 0..word.len() {
                assert_eq!(d2b[shown + relative], source + relative);
                assert_eq!(b2d[source + relative], shown + relative);
            }
            for byte in shown..shown + word.len() {
                assert_eq!(
                    run_at_byte(&runs, byte).strikethrough.is_some(),
                    word != "open"
                );
            }
        }
        assert_eq!(
            run_at_byte(&runs, display.find("完🙂").unwrap())
                .font
                .weight,
            FontWeight::EXTRA_BOLD
        );
        let code = run_at_byte(&runs, display.find("code").unwrap());
        assert_eq!(code.font.family.as_ref(), "Test Mono");
        assert_eq!(
            code.background_color,
            Some(Theme::one_dark().inline_code_bg)
        );
        let fragment = &decoration.table_cell_fragments[1];
        for byte in b2d[fragment.range.start]..b2d[fragment.range.start + fragment.prefix_len] {
            let run = run_at_byte(&runs, byte);
            assert_eq!(run.font.family.as_ref(), "Test Mono");
            assert!(run.strikethrough.is_none());
        }
    }

    #[test]
    fn table_cell_leading_break_shifts_extended_inline_metadata_and_replacements() {
        let content = "<br>- [ ] ==標🙂== ++under++ ^super^ ~sub~ $x^2$ [^note] :rocket: <code>raw<br>literal</code>";
        let decoration =
            table_cell_decoration(&LineDecoration::default(), content, &(0..content.len()));
        assert_eq!(decoration.table_cell_fragments.len(), 2);
        assert_eq!(&content[decoration.highlight_spans[0].clone()], "標🙂");
        assert_eq!(&content[decoration.superscript_spans[0].clone()], "super");
        assert_eq!(&content[decoration.subscript_spans[0].clone()], "sub");
        assert_eq!(&content[decoration.inline_math_spans[0].clone()], "x^2");
        let underline = decoration
            .inline_spans
            .iter()
            .find(|span| span.kind == markdown::InlineKind::Underline)
            .unwrap();
        assert_eq!(&content[underline.range.clone()], "under");
        let footnote = &decoration.footnote_references[0];
        assert_eq!(footnote.label, "note");
        assert_eq!(&content[footnote.full_range.clone()], "[^note]");
        assert_eq!(&content[footnote.collapsed_range.clone()], "note");
        let rocket = decoration
            .display_replacements
            .iter()
            .find(|replacement| replacement.display_text == "🚀")
            .unwrap();
        assert_eq!(&content[rocket.source_range.clone()], ":rocket:");
        assert_eq!(rocket.source_range, rocket.trigger_range);
        let raw = content.find("<code>").unwrap();
        assert!(decoration.protected_ranges.iter().any(|range| {
            range.start == raw && &content[range.clone()] == "<code>raw<br>literal</code>"
        }));
        assert_table_cell_inline_ranges(content, &decoration);
        let (display, _, _, runs) = table_cell_inline_display_and_runs(content, &decoration);
        assert_eq!(display.matches('\n').count(), 1);
        assert!(display.contains("🚀"));
        assert!(display.contains("<code>raw<br>literal</code>"));
        assert!(runs.iter().all(|run| run.strikethrough.is_none()));
    }

    #[test]
    fn table_cell_inline_context_clips_cell_start_triggers_without_losing_syntax() {
        for (content, expected) in [
            ("**bold**", "bold"),
            ("*italic*", "italic"),
            ("~~gone~~", "gone"),
            ("`code`", "code"),
            ("[link](https://example.test)", "link"),
            ("*lead<br>- [x] task*", "lead\n   task"),
        ] {
            let decoration =
                table_cell_decoration(&LineDecoration::default(), content, &(0..content.len()));
            assert!(!decoration.syntax_ranges.is_empty(), "{content:?}");
            assert!(
                decoration
                    .syntax_ranges
                    .iter()
                    .any(|syntax| syntax.trigger_range.start == 0)
            );
            assert_table_cell_inline_ranges(content, &decoration);
            let (display, _, _, _) = table_cell_inline_display_and_runs(content, &decoration);
            assert_eq!(display, expected, "{content:?}");
        }
    }

    #[test]
    fn table_cell_leading_break_does_not_add_block_html_protection() {
        for content in ["<br>", "<br><br>"] {
            let decoration =
                table_cell_decoration(&LineDecoration::default(), content, &(0..content.len()));
            assert!(decoration.protected_ranges.is_empty());
            assert_eq!(
                decoration.table_cell_fragments.len(),
                content.matches("<br>").count() + 1
            );
            assert_table_cell_inline_ranges(content, &decoration);
            let (display, _, _, _) = table_cell_inline_display_and_runs(content, &decoration);
            assert_eq!(display, "\n".repeat(content.matches("<br>").count()));
        }
    }

    #[test]
    fn table_cell_leading_break_preserves_projected_document_reference_targets() {
        let source = "| Neighbor | Input |\n| --- | --- |\n| 隣🙂 | <br>- [ ] [参照][ref] ![写真][pic] |\n\n[ref]: https://reference.test\n[pic]: photo.png\n";
        let decorations = markdown::parse_decorations(source);
        let row = &decorations[2];
        let info = row.table_info.as_ref().unwrap();
        let content = &info.cells[1];
        let decoration = table_cell_decoration(row, content, &info.cell_ranges[1]);
        for (kind, target, label) in [
            (markdown::InlineKind::Link, "https://reference.test", "参照"),
            (markdown::InlineKind::Image, "photo.png", "写真"),
        ] {
            let span = decoration
                .inline_spans
                .iter()
                .find(|span| span.kind == kind && span.target.as_deref() == Some(target))
                .expect("document reference target must survive paragraph-context parsing");
            assert!(content[span.range.clone()].contains(label));
            assert_eq!(
                decoration
                    .inline_spans
                    .iter()
                    .filter(|other| { other.kind == kind && other.range == span.range })
                    .count(),
                1
            );
        }
        assert_table_cell_inline_ranges(content, &decoration);
        let (display, d2b, b2d, _) = table_cell_inline_display_and_runs(content, &decoration);
        assert!(display.starts_with("\n   参照"));
        assert!(!display.contains("[ref]"));
        let source = content.find("参照").unwrap();
        let shown = display.find("参照").unwrap();
        assert_eq!(b2d[source], shown);
        assert_eq!(d2b[shown], source);
    }

    #[test]
    fn table_cell_inline_context_preserves_explicit_non_list_block_parsing() {
        for content in [
            "# **heading**",
            "> **quote**",
            "---",
            "```rust",
            "[^note]: **body**",
            "$$x^2$$",
        ] {
            let original = markdown::parse_decorations(content).remove(0);
            let decoration =
                table_cell_decoration(&LineDecoration::default(), content, &(0..content.len()));
            assert_eq!(decoration.block_type, original.block_type, "{content:?}");
            assert_eq!(
                decoration.inline_spans, original.inline_spans,
                "{content:?}"
            );
            // Fragment classification already removes the leading quote's
            // structural syntax; the inline-context change must not alter it.
            let leading = super::list_marker_indent(content);
            let expected_syntax = original
                .syntax_ranges
                .iter()
                .filter(|syntax| {
                    !(syntax.range.start < leading && syntax.trigger_range.end <= leading)
                })
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(decoration.syntax_ranges, expected_syntax, "{content:?}");
            assert_eq!(
                decoration.display_replacements, original.display_replacements,
                "{content:?}"
            );
            assert_eq!(
                decoration.block_preview, original.block_preview,
                "{content:?}"
            );
            assert_eq!(
                decoration.footnote_definition_label, original.footnote_definition_label,
                "{content:?}"
            );
            assert_table_cell_inline_ranges(content, &decoration);
        }
    }

    #[test]
    fn table_cells_project_wiki_footnote_and_extended_inline_syntax() {
        let source = "| text |\n| --- |\n| [[notes/page.md]] ==mark== ^super^ ~sub~ [^note] ++under++ |\n\n[^note]: Footnote";
        let decorations = markdown::parse_decorations(source);
        let row = &decorations[2];
        let info = row.table_info.as_ref().unwrap();
        let cell = table_cell_decoration(row, &info.cells[0], &info.cell_ranges[0]);
        assert_eq!(cell.wiki_links.len(), 1);
        assert_eq!(cell.wiki_links[0].target_path, "notes/page.md");
        assert_eq!(cell.footnote_references.len(), 1);
        assert!(!cell.highlight_spans.is_empty());
        assert!(
            cell.inline_spans
                .iter()
                .any(|span| span.kind == markdown::InlineKind::Underline)
        );
        assert!(!cell.superscript_spans.is_empty());
        assert!(!cell.subscript_spans.is_empty());
        let (folded, display_to_buffer, _, _) = build_display_text(
            &info.cells[0],
            &cell,
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
        assert!(folded.contains("page"));
        assert!(!folded.contains("notes/page.md"));
        assert_eq!(display_to_buffer.len(), folded.len());
    }

    #[test]
    fn table_cells_keep_wiki_alias_pipe_inside_one_cell() {
        let text = "| Link |\n| --- |\n| [[notes/page.md|Page]] |";
        let decorations = markdown::parse_decorations(text);
        let row = &decorations[2];
        let info = row
            .table_info
            .as_ref()
            .expect("wiki alias row remains in table");
        assert_eq!(info.cells.len(), 1);
        let decoration = table_cell_decoration(row, &info.cells[0], &info.cell_ranges[0]);
        assert_eq!(decoration.wiki_links[0].target_path, "notes/page.md");
        assert_eq!(
            markdown::link_target_at_column(row, info.cell_ranges[0].start + 3),
            Some("notes/page.md")
        );
    }

    #[test]
    fn table_cells_fold_list_and_task_prefixes() {
        for (cell, expected_type) in [
            ("- item", markdown::BlockType::UnorderedList),
            ("1. item", markdown::BlockType::OrderedList),
            ("- [ ] item", markdown::BlockType::TaskList(false)),
        ] {
            let decoration =
                table_cell_decoration(&markdown::LineDecoration::default(), cell, &(0..cell.len()));
            assert_eq!(decoration.block_type, expected_type);
            let (folded, d2b, _, _) = build_display_text(
                cell,
                &decoration,
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
            assert!(folded.ends_with("item"));
            assert_eq!(d2b.len(), folded.len());
        }
    }

    #[test]
    fn table_cell_br_separated_nested_tasks_keep_source_mapping() {
        let content = "- [ ] parent<br>  - [x] child";
        let decoration = table_cell_decoration(
            &markdown::LineDecoration::default(),
            content,
            &(0..content.len()),
        );
        let (folded, display_to_buffer, buffer_to_display, _) = build_display_text(
            content,
            &decoration,
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
        assert!(folded.contains("parent\n"));
        assert!(folded.ends_with("child"));
        assert!(!folded.contains("[x]"));
        assert_eq!(display_to_buffer.len(), folded.len());
        let child = content.find("child").unwrap();
        assert_eq!(
            &folded[buffer_to_display[child]..buffer_to_display[child] + 5],
            "child"
        );
        let prefix = content.find("- [x]").unwrap() + 3;
        let (expanded, _, _, _) = build_display_text(
            content,
            &decoration,
            &[prefix..prefix],
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
        assert!(expanded.contains("- [x] child"));
    }

    #[test]
    fn table_cell_br_nested_lists_and_inline_styles_keep_offsets() {
        let content = "- parent<br>  - **child**<br>  1. `code` [[notes/page.md|Page]]";
        let decoration = table_cell_decoration(
            &markdown::LineDecoration::default(),
            content,
            &(0..content.len()),
        );
        let (folded, display_to_buffer, buffer_to_display, _) = build_display_text(
            content,
            &decoration,
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
        assert!(folded.contains("parent\n"));
        assert!(folded.contains("child\n"));
        assert!(!folded.contains("**"));
        assert!(folded.contains("code Page"));
        assert!(!folded.contains("1. code"));
        assert_eq!(display_to_buffer.len(), folded.len());
        for word in ["child", "code", "Page"] {
            let source = content.find(word).unwrap();
            assert_eq!(
                &folded[buffer_to_display[source]..buffer_to_display[source] + word.len()],
                word
            );
        }
    }

    #[test]
    fn table_ordered_list_number_expands_at_caret() {
        let content = "12. long ordered item that wraps";
        let decoration = table_cell_decoration(
            &markdown::LineDecoration::default(),
            content,
            &(0..content.len()),
        );
        let (collapsed, _, _, _) = build_display_text(
            content,
            &decoration,
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
        assert!(collapsed.starts_with("    long"));
        let (expanded, _, _, _) = build_display_text(
            content,
            &decoration,
            &[1..1],
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
        assert_eq!(expanded, content);
    }

    #[test]
    fn table_cell_break_tags_become_editable_display_newlines() {
        let content = "one<br>two<BR/>three<br />four";
        let decoration = table_cell_decoration(
            &markdown::LineDecoration::default(),
            content,
            &(0..content.len()),
        );
        assert_eq!(decoration.display_replacements.len(), 3);
        assert!(
            decoration
                .display_replacements
                .iter()
                .all(|replacement| replacement.display_text == "\n")
        );
        let (display_text, _, _, _) = build_display_text(
            content,
            &decoration,
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
        assert_eq!(display_text, "one\ntwo\nthree\nfour");
        let (selected_display_text, _, _, _) = build_display_text(
            content,
            &decoration,
            &[2..8],
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
        assert_eq!(selected_display_text, "one\ntwo\nthree\nfour");
        assert_eq!(
            super::hard_line_byte_ranges("one\n\nthree"),
            vec![0..3, 4..4, 5..10]
        );

        let literal = r"\<br> `<br>`";
        let decoration = table_cell_decoration(
            &markdown::LineDecoration::default(),
            literal,
            &(0..literal.len()),
        );
        assert!(decoration.display_replacements.is_empty());
    }

    #[test]
    fn emoji_shortcode_folds_into_unicode_display_text() {
        let line = "launch :rocket: now";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, display_to_buffer, buffer_to_display, _) = build_display_text(
            line,
            &decoration,
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

        assert_eq!(display_text, "launch 🚀 now");
        assert_eq!(display_to_buffer[7], 7);
        assert_eq!(buffer_to_display[7], 7);
        assert_eq!(buffer_to_display[14], 7);
    }

    #[test]
    fn collapsed_scripts_use_compact_placeholders() {
        let line = "E=mc^22^ H~2~O [^12]";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (display_text, _, _, _) = build_display_text(
            line,
            &decoration,
            &[],
            false,
            None,
            0,
            0,
            0,
            0,
            0,
            0.5,
            0.5,
            0.5,
        );

        assert!(display_text.len() < "E=mc22 H2O 12".len());
    }

    #[test]
    fn active_highlight_keeps_markers_as_syntax_color() {
        let line = "==hi==";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges = [Range { start: 2, end: 2 }];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        assert_eq!(display_text, "==hi==");
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].len, 2);
        assert_eq!(runs[0].color, theme.syntax_marker);
        assert_eq!(runs[0].background_color, None);
        assert_eq!(runs[1].len, 2);
        assert_eq!(runs[1].color, theme.bold);
        assert_eq!(
            runs[1].background_color,
            Some(crate::theme::with_alpha(crate::hex_hsla!("#E5C07B"), 0.22))
        );
        assert_eq!(runs[2].len, 2);
        assert_eq!(runs[2].color, theme.syntax_marker);
        assert_eq!(runs[2].background_color, None);
    }

    #[test]
    fn folded_highlight_keeps_yellow_background() {
        let line = "==hi==";
        let decoration = markdown::parse_decorations(line)
            .into_iter()
            .next()
            .unwrap_or_default();
        let active_ranges: [Range<usize>; 0] = [];
        let (display_text, display_to_buffer, _, prefix_collapsed) = build_display_text(
            line,
            &decoration,
            &active_ranges,
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
        let theme = Theme::one_dark();
        let runs = build_text_runs_for_display(
            &display_text,
            line,
            &display_to_buffer,
            &decoration,
            &active_ranges,
            false,
            prefix_collapsed,
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font(".SystemUIFont"),
            &font("Menlo"),
            &font("Kaiti SC"),
            0,
            16.0,
            &theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::NORMAL,
            theme.list_marker,
            FontWeight::BOLD,
            theme.syntax_marker,
            None,
            false,
        );

        let highlight_bg = crate::theme::with_alpha(crate::hex_hsla!("#E5C07B"), 0.22);
        assert!(
            runs.iter()
                .any(|run| run.background_color == Some(highlight_bg)),
            "expected a run with the yellow highlight background after folding"
        );
    }
}

pub(crate) fn find_syntax_for_language(language: &str) -> Option<&'static SyntaxReference> {
    let syntax_set = syntax_set();
    syntax_lookup_candidates(language)
        .into_iter()
        .find_map(|candidate| {
            syntax_set
                .find_syntax_by_token(&candidate)
                .or_else(|| syntax_set.find_syntax_by_name(&candidate))
        })
}

pub(crate) fn find_syntax_for_path(
    path: &Path,
    first_line: &str,
) -> Option<&'static SyntaxReference> {
    let syntaxes = syntax_set();
    // syntect's find_syntax_for_file reopens unknown files to inspect their first
    // line. Callers already have those bytes, including for disconnected mounts.
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| syntaxes.find_syntax_by_extension(name))
        .or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .and_then(|ext| syntaxes.find_syntax_by_extension(ext))
        })
        .or_else(|| syntaxes.find_syntax_by_first_line(first_line.trim_start_matches('\u{feff}')))
        .filter(|syntax| syntax.name != "Plain Text")
        .or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .and_then(find_syntax_for_language)
        })
}

#[cfg(test)]
mod path_syntax_tests {
    use super::*;

    #[test]
    fn syntax_detection_uses_supplied_first_line_without_opening_the_path() {
        let path = Path::new("/unavailable-share/does-not-exist/script");
        let syntax = find_syntax_for_path(path, "#!/usr/bin/env python3").unwrap();
        assert!(syntax.name.contains("Python"));
        assert!(find_syntax_for_path(path, "ordinary text").is_none());
    }

    #[test]
    fn filename_and_extension_syntax_take_precedence_over_the_first_line() {
        assert_eq!(
            find_syntax_for_path(
                Path::new("/unavailable-share/main.rs"),
                "#!/usr/bin/env python3"
            )
            .unwrap()
            .name,
            "Rust"
        );
        assert!(find_syntax_for_path(Path::new("/unavailable-share/Dockerfile"), "").is_some());
    }
}

#[cfg(test)]
mod syntax_registry_tests {
    use super::*;

    #[test]
    fn extended_registry_covers_common_modern_languages() {
        for language in [
            "typescript",
            "tsx",
            "vue",
            "svelte",
            "dockerfile",
            "toml",
            "kotlin",
            "swift",
            "zig",
            "wgsl",
            "terraform",
            "nix",
            "solidity",
        ] {
            assert!(
                find_syntax_for_language(language).is_some(),
                "missing syntax for {language}"
            );
        }
    }
}

fn code_palette_syntect_color(rgb: u32) -> SyntectColor {
    SyntectColor {
        r: ((rgb >> 16) & 0xFF) as u8,
        g: ((rgb >> 8) & 0xFF) as u8,
        b: (rgb & 0xFF) as u8,
        a: 0xFF,
    }
}

/// Return the (lazily built) syntect theme for a code syntax palette.
///
/// Built themes are leaked and cached; the number of distinct palettes is
/// bounded by the built-in theme list, so the leak is bounded too.
pub(crate) fn syntect_theme_for_palette(palette: CodeSyntaxPalette) -> &'static SyntectTheme {
    static CACHE: OnceLock<Mutex<HashMap<CodeSyntaxPalette, &'static SyntectTheme>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache
        .entry(palette)
        .or_insert_with(|| Box::leak(Box::new(build_syntect_theme(palette))))
}

/// Build a syntect theme with comprehensive scope coverage from a per-theme
/// token palette. Scope rules follow the One Dark Pro structure; the palette
/// supplies the actual colors.
fn build_syntect_theme(palette: CodeSyntaxPalette) -> SyntectTheme {
    let chalky = code_palette_syntect_color(palette.type_name);
    let coral = code_palette_syntect_color(palette.variable);
    let cyan = code_palette_syntect_color(palette.operator);
    let fountain_blue = code_palette_syntect_color(palette.function);
    let green = code_palette_syntect_color(palette.string);
    let ivory = code_palette_syntect_color(palette.foreground);
    let stone = code_palette_syntect_color(palette.comment);
    let violet = code_palette_syntect_color(palette.keyword);
    let whiskey = code_palette_syntect_color(palette.number);

    fn rule(scope: &str, fg: SyntectColor) -> ThemeItem {
        ThemeItem {
            scope: scope
                .parse::<ScopeSelectors>()
                .unwrap_or_else(|_| ScopeSelectors { selectors: vec![] }),
            style: StyleModifier {
                foreground: Some(fg),
                background: None,
                font_style: None,
            },
        }
    }

    fn rule_italic(scope: &str, fg: SyntectColor) -> ThemeItem {
        ThemeItem {
            scope: scope
                .parse::<ScopeSelectors>()
                .unwrap_or_else(|_| ScopeSelectors { selectors: vec![] }),
            style: StyleModifier {
                foreground: Some(fg),
                background: None,
                font_style: Some(syntect::highlighting::FontStyle::ITALIC),
            },
        }
    }

    fn rule_bold(scope: &str, fg: SyntectColor) -> ThemeItem {
        ThemeItem {
            scope: scope
                .parse::<ScopeSelectors>()
                .unwrap_or_else(|_| ScopeSelectors { selectors: vec![] }),
            style: StyleModifier {
                foreground: Some(fg),
                background: None,
                font_style: Some(syntect::highlighting::FontStyle::BOLD),
            },
        }
    }

    SyntectTheme {
        name: Some("Moeka Code".to_string()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(ivory),
            background: None,
            ..ThemeSettings::default()
        },
        scopes: vec![
            // ── Comments ────────────────────────────────────────
            rule_italic("comment, punctuation.definition.comment", stone),
            // ── Strings ─────────────────────────────────────────
            rule(
                "string, string.quoted, string.template, string.interpolated",
                green,
            ),
            rule("string.regexp", coral),
            // ── Constants / Numbers ─────────────────────────────
            rule(
                "constant.numeric, constant.language, constant.character",
                whiskey,
            ),
            rule("constant.other, constant.other.color", whiskey),
            // ── Variables ───────────────────────────────────────
            rule("variable, variable.other, variable.parameter", coral),
            rule("variable.language", coral),
            rule(
                "variable.other.constant, variable.other.enummember",
                whiskey,
            ),
            // ── Keywords ────────────────────────────────────────
            rule(
                "keyword, keyword.control, keyword.operator.new, keyword.operator.expression, keyword.operator.logical, keyword.operator.delete, keyword.operator.typeof, keyword.operator.void, keyword.operator.instanceof, keyword.operator.in",
                violet,
            ),
            rule(
                "keyword.operator, keyword.operator.assignment, keyword.operator.arithmetic, keyword.operator.comparison, keyword.operator.ternary",
                cyan,
            ),
            // ── Storage / Types ─────────────────────────────────
            rule("storage, storage.type, storage.modifier", violet),
            rule(
                "support.type, support.class, entity.name.type, entity.name.namespace",
                chalky,
            ),
            rule("entity.other.inherited-class", chalky),
            // ── Functions ───────────────────────────────────────
            rule(
                "entity.name.function, support.function, meta.function-call",
                fountain_blue,
            ),
            rule("variable.function", fountain_blue),
            // ── Classes / Entities ──────────────────────────────
            rule("entity.name.class, entity.name.type.class", chalky),
            rule("entity.name.tag", coral),
            rule("entity.other.attribute-name", whiskey),
            // ── Punctuation ─────────────────────────────────────
            rule("punctuation, meta.brace, meta.delimiter", ivory),
            rule(
                "punctuation.definition.string, punctuation.definition.template-expression",
                green,
            ),
            rule("punctuation.definition.tag", coral),
            rule("punctuation.section.embedded", coral),
            // ── Support ─────────────────────────────────────────
            rule("support.constant", whiskey),
            rule("support.variable", coral),
            rule("support.type.property-name", coral),
            // ── Meta ────────────────────────────────────────────
            rule("meta.object-literal.key", coral),
            // ─── Shell / Bash ───────────────────────────────────
            rule("source.shell variable, source.shell variable.other", coral),
            rule(
                "source.shell keyword.operator, source.shell keyword.operator.pipe, source.shell keyword.operator.redirect, source.shell keyword.operator.logical",
                cyan,
            ),
            rule("source.shell support.function", fountain_blue),
            rule("source.shell entity.name.function", fountain_blue),
            rule("source.shell keyword.control", violet),
            rule(
                "source.shell string.quoted, source.shell string.unquoted",
                green,
            ),
            rule("source.shell punctuation.definition.variable", coral),
            rule(
                "source.shell variable.other.special, source.shell variable.other.positional, source.shell variable.other.bracket",
                coral,
            ),
            rule("source.shell meta.function-call.arguments", ivory),
            rule("source.shell constant.other.option", whiskey),
            // ─── Python ─────────────────────────────────────────
            rule("source.python keyword.operator", cyan),
            rule(
                "source.python support.function, source.python meta.function-call",
                fountain_blue,
            ),
            rule("source.python support.type", chalky),
            rule(
                "source.python entity.name.function.decorator, source.python meta.function.decorator",
                chalky,
            ),
            rule("source.python constant.language", whiskey),
            // ─── JavaScript / TypeScript ────────────────────────
            rule(
                "source.js variable.other.constant, source.ts variable.other.constant",
                whiskey,
            ),
            rule(
                "source.js keyword.operator, source.ts keyword.operator",
                cyan,
            ),
            rule("source.js support.class, source.ts support.class", chalky),
            rule(
                "source.js punctuation.accessor, source.ts punctuation.accessor",
                ivory,
            ),
            rule(
                "source.js entity.name.function, source.ts entity.name.function",
                fountain_blue,
            ),
            // ─── Rust ───────────────────────────────────────────
            rule("source.rust keyword.operator", cyan),
            rule(
                "source.rust entity.name.type, source.rust entity.name.struct, source.rust entity.name.enum",
                chalky,
            ),
            rule("source.rust entity.name.function", fountain_blue),
            rule("source.rust entity.name.lifetime", whiskey),
            rule("source.rust storage.type", violet),
            rule(
                "source.rust support.macro, source.rust entity.name.macro",
                cyan,
            ),
            rule("source.rust constant.language", whiskey),
            // ─── Go ─────────────────────────────────────────────
            rule("source.go keyword.operator", cyan),
            rule("source.go entity.name.function", fountain_blue),
            rule("source.go entity.name.type", chalky),
            rule("source.go support.function.builtin", cyan),
            rule("source.go storage.type", violet),
            // ─── C / C++ ───────────────────────────────────────
            rule(
                "source.c keyword.operator, source.c++ keyword.operator",
                cyan,
            ),
            rule(
                "source.c entity.name.function, source.c++ entity.name.function",
                fountain_blue,
            ),
            rule("source.c storage.type, source.c++ storage.type", violet),
            rule(
                "source.c keyword.control.directive, source.c++ keyword.control.directive, meta.preprocessor",
                violet,
            ),
            rule(
                "source.c entity.name.type, source.c++ entity.name.type",
                chalky,
            ),
            // ─── Java ───────────────────────────────────────────
            rule("source.java keyword.operator", cyan),
            rule("source.java entity.name.function", fountain_blue),
            rule(
                "source.java entity.name.type, source.java storage.type",
                chalky,
            ),
            rule("source.java keyword.other.import", violet),
            rule("source.java entity.name.class", chalky),
            // ─── Ruby ───────────────────────────────────────────
            rule("source.ruby keyword.operator", cyan),
            rule("source.ruby entity.name.function", fountain_blue),
            rule(
                "source.ruby support.class, source.ruby entity.name.type.class",
                chalky,
            ),
            rule("source.ruby variable.other.readwrite.instance", coral),
            rule("source.ruby constant.language.symbol", fountain_blue),
            rule("source.ruby punctuation.definition.variable", coral),
            // ─── PHP ────────────────────────────────────────────
            rule("source.php keyword.operator", cyan),
            rule("source.php entity.name.function", fountain_blue),
            rule("source.php support.class", chalky),
            rule("source.php variable.other", coral),
            rule("source.php punctuation.definition.variable", coral),
            // ─── CSS / SCSS / LESS ──────────────────────────────
            rule(
                "source.css support.type.property-name, source.scss support.type.property-name",
                coral,
            ),
            rule(
                "source.css entity.other.attribute-name.class, source.scss entity.other.attribute-name.class",
                chalky,
            ),
            rule(
                "source.css entity.other.attribute-name.id, source.scss entity.other.attribute-name.id",
                fountain_blue,
            ),
            rule(
                "source.css constant.numeric, source.scss constant.numeric",
                whiskey,
            ),
            rule(
                "source.css keyword.other.unit, source.scss keyword.other.unit",
                whiskey,
            ),
            rule(
                "source.css support.constant, source.scss support.constant",
                whiskey,
            ),
            rule(
                "source.css entity.name.tag, source.scss entity.name.tag",
                coral,
            ),
            // ─── HTML / XML ─────────────────────────────────────
            rule("text.html entity.name.tag, text.xml entity.name.tag", coral),
            rule(
                "text.html entity.other.attribute-name, text.xml entity.other.attribute-name",
                whiskey,
            ),
            rule("text.html string.quoted, text.xml string.quoted", green),
            rule(
                "text.html punctuation.definition.tag, text.xml punctuation.definition.tag",
                ivory,
            ),
            // ─── JSON ───────────────────────────────────────────
            rule("source.json support.type.property-name", coral),
            rule("source.json string.quoted", green),
            rule("source.json constant", whiskey),
            // ─── YAML ───────────────────────────────────────────
            rule("source.yaml entity.name.tag", coral),
            rule("source.yaml string", green),
            rule("source.yaml constant", whiskey),
            // ─── TOML ───────────────────────────────────────────
            rule(
                "source.toml entity.name.tag, source.toml support.type.property-name",
                coral,
            ),
            rule("source.toml string", green),
            rule("source.toml constant", whiskey),
            // ─── SQL ────────────────────────────────────────────
            rule("source.sql keyword", violet),
            rule("source.sql entity.name.function", fountain_blue),
            rule(
                "source.sql constant.other.database-name, source.sql constant.other.table-name",
                chalky,
            ),
            rule("source.sql string", green),
            // ─── Markdown (nested) ──────────────────────────────
            rule("markup.heading", coral),
            rule_bold("markup.bold", whiskey),
            rule_italic("markup.italic", violet),
            rule("markup.inline.raw, markup.raw.inline", green),
            rule("markup.underline.link", fountain_blue),
            // ─── Diff ───────────────────────────────────────────
            rule("markup.inserted", green),
            rule("markup.deleted", coral),
            rule("markup.changed", whiskey),
            // ─── Dockerfile ─────────────────────────────────────
            rule("source.dockerfile keyword", violet),
            rule(
                "source.dockerfile entity.name.function, source.dockerfile support.function",
                fountain_blue,
            ),
            // ─── Makefile ───────────────────────────────────────
            rule("source.makefile variable", coral),
            rule("source.makefile entity.name.function", fountain_blue),
            rule("source.makefile keyword.control", violet),
            // ─── Catch-all for operators ────────────────────────
            rule("keyword.operator", cyan),
        ],
    }
}

fn syntect_color_to_hsla(color: SyntectColor) -> Hsla {
    rgba_to_hsla(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    )
}

fn rgba_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f32::EPSILON {
        return Hsla {
            h: 0.0,
            s: 0.0,
            l,
            a,
        };
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < f32::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    Hsla {
        h: h / 6.0,
        s,
        l,
        a,
    }
}

pub(crate) fn apply_code_block_highlighting(
    line_content: &str,
    language: &str,
    theme: &Theme,
    colors: &mut [Hsla],
    weights: &mut [FontWeight],
    styles: &mut [FontStyle],
) {
    apply_language_highlighting(
        line_content,
        language,
        theme,
        colors,
        weights,
        styles,
        false,
    );
    if matches!(
        language.trim().to_ascii_lowercase().as_str(),
        "markdown" | "md"
    ) {
        let cells = markdown::parse_table_cells_with_ranges(line_content);
        let pipe_color: Hsla = gpui::rgb(theme.code_palette.operator).into();
        let separator_color: Hsla = gpui::rgb(theme.code_palette.keyword).into();
        for (offset, byte) in line_content.bytes().enumerate() {
            if byte == b'|' && !cells.iter().any(|(_, range)| range.contains(&offset)) {
                if let Some(color) = colors.get_mut(offset) {
                    *color = pipe_color;
                }
            }
        }
        if !cells.is_empty()
            && markdown::is_separator_row(
                &cells
                    .iter()
                    .map(|(cell, _)| cell.clone())
                    .collect::<Vec<_>>(),
            )
        {
            for (_, range) in cells {
                for offset in range {
                    if let Some(color) = colors.get_mut(offset) {
                        *color = separator_color;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[test]
fn markdown_table_highlighting_preserves_escaped_and_code_pipes() {
    let content = "| \u{4e2d}\u{6587} \\| value | `a|b` |";
    let theme = Theme::moeka_print_light();
    let mut baseline = vec![theme.text_primary; content.len()];
    let mut colors = baseline.clone();
    let mut weights = vec![FontWeight::NORMAL; content.len()];
    let mut styles = vec![FontStyle::Normal; content.len()];
    apply_language_highlighting(
        content,
        "markdown",
        &theme,
        &mut baseline,
        &mut weights,
        &mut styles,
        false,
    );
    apply_code_block_highlighting(
        content,
        "markdown",
        &theme,
        &mut colors,
        &mut weights,
        &mut styles,
    );
    for offset in [
        content.find("\\|").unwrap() + 1,
        content.find("a|b").unwrap() + 1,
    ] {
        assert_eq!(colors[offset], baseline[offset]);
    }
    let pipe_color: Hsla = gpui::rgb(theme.code_palette.operator).into();
    assert_eq!(colors[0], pipe_color);
    assert_eq!(colors[content.len() - 1], pipe_color);
    assert_eq!(colors.len(), content.len());
}

fn apply_language_highlighting(
    line_content: &str,
    language: &str,
    theme: &Theme,
    colors: &mut [Hsla],
    weights: &mut [FontWeight],
    styles: &mut [FontStyle],
    math_context: bool,
) {
    if line_content.is_empty() {
        return;
    }

    if is_mermaid_language(language) {
        apply_mermaid_block_highlighting(line_content, theme, colors, weights, styles);
        return;
    }

    let cache_key = CodeHighlightCacheKey {
        language: language.to_string(),
        line_content: line_content.to_string(),
        palette: theme.code_palette,
        math_context,
    };
    if let Ok(cache) = code_highlight_cache().lock()
        && let Some(segments) = cache.get(&cache_key).cloned()
    {
        apply_code_highlight_segments(&segments, colors, weights, styles);
        return;
    }

    let syntax_set = syntax_set();
    let syntax =
        find_syntax_for_language(language).unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut highlighter =
        HighlightLines::new(syntax, syntect_theme_for_palette(theme.code_palette));
    if math_context
        && !line_content.trim_start().starts_with("$$")
        && !line_content.trim_start().starts_with("\\[")
    {
        // Each source row is highlighted independently. Seed body rows into
        // TeX's math grammar without adding bytes to the text or source maps.
        let _ = highlighter.highlight_line("$$\n", syntax_set);
    }
    if let Ok(ranges) = highlighter.highlight_line(line_content, syntax_set) {
        let mut byte_pos = 0usize;
        let mut segments = Vec::with_capacity(ranges.len());
        for (style, piece) in &ranges {
            let end = (byte_pos + piece.len()).min(colors.len());
            let color = syntect_color_to_hsla(style.foreground);
            let weight = if style
                .font_style
                .contains(syntect::highlighting::FontStyle::BOLD)
            {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            };
            let font_style = if style
                .font_style
                .contains(syntect::highlighting::FontStyle::ITALIC)
            {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            };
            segments.push(CodeHighlightSegment {
                len: piece.len(),
                color,
                weight,
                style: font_style,
            });
            for idx in byte_pos..end {
                colors[idx] = color;
                weights[idx] = weight;
                styles[idx] = font_style;
            }
            byte_pos = end;
        }
        if let Ok(mut cache) = code_highlight_cache().lock() {
            if cache.len() >= CODE_HIGHLIGHT_CACHE_MAX_ENTRIES {
                cache.clear();
            }
            cache.insert(cache_key, segments);
        }
    }
}

/// Image source for an asset shipped with the app.
///
/// Goes through `AssetSource` rather than a resolved path so it still works
/// where the assets only exist inside the binary, which is every Windows and
/// Linux build: those ship a lone executable with no `assets/` beside it.
pub(crate) fn asset_image_source(path: &'static str) -> gpui::ImageSource {
    gpui::ImageSource::Resource(gpui::Resource::Embedded(SharedString::new_static(path)))
}

#[cfg(target_os = "macos")]
pub(crate) const APP_ICON_PATH: &str = "assets/icons/app/icon-0.png";

pub(crate) fn is_remote_image_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// Resolve an image path to an absolute filesystem path.
/// Tries in order:
/// 1. Remote URLs (http:// https://) — not resolved to a local path
/// 2. file:// scheme — strip the scheme
/// 3. Home-relative paths (~/...)
/// 4. Relative paths resolved against the current file's parent directory
/// 5. Relative paths resolved against each workspace root
pub(crate) fn resolve_image_to_path(
    raw: &str,
    file_dir: Option<&PathBuf>,
    workspace_roots: &[PathBuf],
) -> Option<PathBuf> {
    if crate::platform::paths::is_file_url(raw) {
        let path = crate::platform::paths::path_from_file_url(raw)?;
        let path = normalize_image_path(&path);
        return path.exists().then_some(path);
    }

    let path_str = percent_decode_path(raw);

    let path_str = path_str.as_str();

    // Expand ~/
    let expanded = crate::platform::paths::expand_tilde(path_str)?;

    // If the path happens to be absolute (e.g. from file:// or ~/ expansion),
    // just check existence.
    if expanded.is_absolute() {
        let expanded = normalize_image_path(&expanded);
        return expanded.exists().then_some(expanded);
    }

    // Relative to current file's directory
    if let Some(dir) = file_dir {
        let candidate = normalize_image_path(&dir.join(&expanded));
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Relative to workspace roots
    for root in workspace_roots {
        let candidate = normalize_image_path(&root.join(&expanded));
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn normalize_image_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn percent_decode_path(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

#[cfg(test)]
mod image_path_tests {
    use super::resolve_image_to_path;

    #[test]
    fn resolves_percent_encoded_markdown_image_path() {
        let workspace = tempfile::tempdir().expect("workspace");
        let notes = workspace.path().join("notes");
        let images = workspace.path().join("images");
        std::fs::create_dir_all(&notes).expect("notes");
        std::fs::create_dir_all(&images).expect("images");
        let image = images.join("示例 image.png");
        std::fs::write(&image, b"image").expect("image");

        assert_eq!(
            resolve_image_to_path(
                "../images/%E7%A4%BA%E4%BE%8B%20image.png",
                Some(&notes),
                &[workspace.path().to_path_buf()],
            ),
            Some(image),
        );
    }
}

/// Load an image from any supported path/URL format.
/// Supports: http(s), file://, ~/paths, relative paths (to file dir or workspace roots).
pub(crate) fn load_image(
    raw_path: &str,
    file_dir: Option<&PathBuf>,
    workspace_roots: &[PathBuf],
) -> anyhow::Result<RenderImage> {
    if is_remote_image_url(raw_path) {
        return load_render_image_from_url(raw_path);
    }

    let resolved = resolve_image_to_path(raw_path, file_dir, workspace_roots)
        .ok_or_else(|| anyhow::anyhow!("could not resolve image path: {}", raw_path))?;
    load_render_image_from_path(&resolved)
}

fn load_render_image_from_bytes(bytes: &[u8]) -> anyhow::Result<RenderImage> {
    let dynamic = image::load_from_memory(bytes)?;
    let mut buffer = dynamic.to_rgba8();

    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let frame = Frame::new(buffer);
    Ok(RenderImage::new([frame]))
}

#[cfg(test)]
pub(crate) fn load_generated_preview_image_from_svg(
    svg: &str,
    kind: BlockPreviewKind,
) -> anyhow::Result<RenderImage> {
    load_render_image_from_svg_with_scale(svg, generated_preview_raster_scale(kind))
}

pub(crate) fn load_generated_preview_image_from_svg_for_editor(
    svg: &str,
    kind: BlockPreviewKind,
) -> anyhow::Result<RenderImage> {
    let raster_scale = generated_preview_raster_scale(kind);
    if kind == BlockPreviewKind::Math {
        return load_render_image_from_svg_with_scale(svg, raster_scale);
    }
    load_render_image_from_svg_with_limits(
        svg,
        raster_scale,
        Some(GENERATED_PREVIEW_EDITOR_MAX_RASTER_EDGE),
        Some(GENERATED_PREVIEW_EDITOR_MAX_RASTER_PIXELS),
    )
}

pub(crate) fn load_render_image_from_svg_with_scale(
    svg: &str,
    raster_scale: f32,
) -> anyhow::Result<RenderImage> {
    load_render_image_from_svg_with_limits(svg, raster_scale, None, None)
}

pub(crate) fn load_generated_preview_overlay_image_from_svg(
    svg: &str,
    kind: BlockPreviewKind,
    raster_scale: f32,
) -> anyhow::Result<RenderImage> {
    if kind == BlockPreviewKind::Mermaid {
        load_render_image_from_svg_with_limits(
            svg,
            raster_scale,
            Some(GENERATED_PREVIEW_OVERLAY_MAX_RASTER_EDGE),
            Some(GENERATED_PREVIEW_OVERLAY_MAX_RASTER_PIXELS),
        )
    } else {
        load_render_image_from_svg_with_scale(svg, raster_scale)
    }
}

fn load_render_image_from_svg_with_limits(
    svg: &str,
    raster_scale: f32,
    max_edge: Option<f32>,
    max_pixels: Option<f32>,
) -> anyhow::Result<RenderImage> {
    let mut options = usvg::Options::default();
    options.fontdb = generated_preview_fontdb().clone();
    let tree = usvg::Tree::from_str(svg, &options)?;
    let size = tree.size().to_int_size();
    let mut raster_scale = raster_scale.max(0.1);
    if let Some(max_edge) = max_edge {
        raster_scale = raster_scale
            .min(max_edge / size.width().max(1) as f32)
            .min(max_edge / size.height().max(1) as f32);
    }
    if let Some(max_pixels) = max_pixels {
        let source_pixels = size.width().max(1) as f32 * size.height().max(1) as f32;
        raster_scale = raster_scale.min((max_pixels / source_pixels).sqrt());
    }
    let raster_scale = raster_scale.max(f32::MIN_POSITIVE);
    let raster_width = ((size.width() as f32) * raster_scale).round().max(1.0) as u32;
    let raster_height = ((size.height() as f32) * raster_scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(raster_width, raster_height)
        .ok_or_else(|| anyhow::anyhow!("failed to allocate SVG pixmap"))?;
    let mut pixmap_mut = pixmap.as_mut();
    resvg::render(
        &tree,
        Transform::from_scale(raster_scale, raster_scale),
        &mut pixmap_mut,
    );

    let mut buffer = RgbaImage::from_raw(raster_width, raster_height, pixmap.take())
        .ok_or_else(|| anyhow::anyhow!("failed to convert SVG pixmap"))?;
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    Ok(RenderImage::new([Frame::new(buffer)]))
}

fn generated_preview_fontdb() -> &'static Arc<usvg::fontdb::Database> {
    static FONT_DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();

    FONT_DB.get_or_init(|| {
        let mut fontdb = usvg::fontdb::Database::new();
        fontdb.load_system_fonts();
        Arc::new(fontdb)
    })
}

fn generated_mathjax_renderer() -> &'static Mutex<MathJax> {
    static MATHJAX: OnceLock<Mutex<MathJax>> = OnceLock::new();
    MATHJAX.get_or_init(|| Mutex::new(MathJax::new()))
}

pub(crate) fn inline_math_cache_key(source_text: &str) -> String {
    format!("__generated_preview__:inline-math:{source_text}")
}

pub(crate) fn add_inline_math_display_replacements(
    decoration: &mut LineDecoration,
    line_content: &str,
    image_cache: &HashMap<String, Option<Arc<RenderImage>>>,
    placeholder_unit_width: f32,
) -> bool {
    if placeholder_unit_width <= 0.0 {
        return false;
    }
    let mut added = false;
    for source_range in decoration.inline_math_spans.clone() {
        let Some(source_text) = line_content.get(source_range.clone()) else {
            continue;
        };
        let Some(Some(image)) = image_cache.get(&inline_math_cache_key(source_text)) else {
            continue;
        };
        let image_size = image.size(0);
        let (natural_width, natural_height) = intrinsic_generated_preview_size_for_kind(
            image_size.width.0 as f32,
            image_size.height.0 as f32,
            BlockPreviewKind::Math,
        );
        if natural_width <= 0.0 || natural_height <= 0.0 {
            continue;
        }
        // Keep enough characters for source mappings; shaping later sets their
        // total advance to the exact intrinsic width.
        let placeholder_columns = (natural_width / placeholder_unit_width).ceil().max(1.0) as usize;
        decoration.display_replacements.push(DisplayReplacement {
            source_range: source_range.clone(),
            display_text: "\u{a0}".repeat(placeholder_columns),
            trigger_range: source_range.start.saturating_sub(1)
                ..(source_range.end + 1).min(line_content.len()),
        });
        added = true;
    }
    added
}

pub(crate) fn inline_math_display_range(
    decoration: &LineDecoration,
    source_range: &Range<usize>,
    buffer_to_display: &[usize],
) -> Option<Range<usize>> {
    let replacement = decoration.display_replacements.iter().find(|replacement| {
        replacement.source_range == *source_range
            && !replacement.display_text.is_empty()
            && replacement.display_text.chars().all(|ch| ch == '\u{a0}')
    })?;
    // Expanded replacements can map several placeholder characters to one source
    // byte, so the source start does not necessarily map to the placeholder start.
    let end = buffer_to_display_offset(buffer_to_display, source_range.end);
    Some(end.checked_sub(replacement.display_text.len())?..end)
}

fn inline_math_placeholder_glyph_x(
    index: usize,
    original_x: Pixels,
    spans: &[(&Range<usize>, Pixels, f32, f32)],
) -> Pixels {
    let mut x = original_x;
    let mut offset = 0.0;
    for (range, start_x, width, delta) in spans {
        if index >= range.end {
            offset += delta;
        } else if index >= range.start {
            let fraction = (index - range.start) as f32 / range.len() as f32;
            x = *start_x + px(fraction * width);
        }
    }
    x + px(offset)
}

pub(crate) fn keep_inline_math_placeholders_unwrapped(
    wrapped: &mut WrappedLine,
    decoration: &LineDecoration,
    line_content: &str,
    buffer_to_display: &[usize],
    active_ranges: &[Range<usize>],
    image_cache: &HashMap<String, Option<Arc<RenderImage>>>,
    display_offset: usize,
) {
    let ranges = decoration
        .inline_math_spans
        .iter()
        .filter_map(|source_range| {
            let trigger_range = source_range.start.saturating_sub(1)..source_range.end + 1;
            if active_ranges_intersect(active_ranges, &trigger_range) {
                return None;
            }
            let display_range =
                inline_math_display_range(decoration, source_range, buffer_to_display)?;
            let start = display_range.start;
            let end = display_range.end;
            if start < display_offset || end > display_offset + wrapped.text.len() {
                return None;
            }
            let range = start - display_offset..end - display_offset;
            let placeholder = wrapped.text.get(range.clone())?;
            if placeholder.is_empty() || !placeholder.chars().all(|ch| ch == '\u{a0}') {
                return None;
            }
            let source = line_content.get(source_range.clone())?;
            let image = image_cache.get(&inline_math_cache_key(source))?.as_ref()?;
            let image_size = image.size(0);
            let (natural_width, _) = intrinsic_generated_preview_size_for_kind(
                image_size.width.0 as f32,
                image_size.height.0 as f32,
                BlockPreviewKind::Math,
            );
            Some((range, natural_width))
        })
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return;
    }

    let old_line = &wrapped.unwrapped_layout;
    let spans = ranges
        .iter()
        .map(|(range, natural_width)| {
            let start_x = old_line.x_for_index(range.start);
            let width = f32::from(old_line.x_for_index(range.end) - start_x);
            let target_width = *natural_width;
            (range, start_x, target_width, target_width - width)
        })
        .collect::<Vec<_>>();
    let mut runs = old_line.runs.clone();
    // Placeholder characters preserve source mappings, not horizontal padding.
    // Match the raster width exactly, removing rounding and letter-spacing excess
    // as well as expanding undersized advances, and shift following text accordingly.
    for glyph in runs.iter_mut().flat_map(|run| &mut run.glyphs) {
        glyph.position.x = inline_math_placeholder_glyph_x(glyph.index, glyph.position.x, &spans);
    }
    let line = Arc::new(LineLayout {
        font_size: old_line.font_size,
        width: old_line.width + px(spans.iter().map(|span| span.3).sum::<f32>()),
        ascent: old_line.ascent,
        descent: old_line.descent,
        runs,
        len: old_line.len,
    });
    let wrap_width = wrapped.wrap_width;
    let mut wrap_boundaries = Default::default();
    if let Some(wrap_width) = wrap_width {
        // GPUI wraps between NBSP glyphs. Temporarily make each placeholder a
        // single glyph for wrapping, then restore all glyphs for accurate hit testing.
        let mut glyph_maps = Vec::new();
        let atomic_runs = line
            .runs
            .iter()
            .map(|run| {
                let glyph_indices = run
                    .glyphs
                    .iter()
                    .enumerate()
                    .filter_map(|(index, glyph)| {
                        (!ranges
                            .iter()
                            .any(|(range, _)| range.start < glyph.index && glyph.index < range.end))
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                let glyphs = glyph_indices
                    .iter()
                    .map(|&index| run.glyphs[index].clone())
                    .collect();
                glyph_maps.push(glyph_indices);
                ShapedRun {
                    font_id: run.font_id,
                    glyphs,
                }
            })
            .collect();
        let arc_mut: &mut Arc<WrappedLineLayout> = &mut *wrapped;
        *arc_mut = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: line.font_size,
                width: line.width,
                ascent: line.ascent,
                descent: line.descent,
                runs: atomic_runs,
                len: line.len,
            }),
            wrap_boundaries: Default::default(),
            wrap_width: Some(wrap_width),
            row_heights: Vec::new(),
        });
        wrapped.rewrap(wrap_width);
        wrap_boundaries = wrapped
            .wrap_boundaries
            .iter()
            .map(|boundary| gpui::WrapBoundary {
                run_ix: boundary.run_ix,
                glyph_ix: glyph_maps[boundary.run_ix][boundary.glyph_ix],
            })
            .collect();
    }
    let arc_mut: &mut Arc<WrappedLineLayout> = &mut *wrapped;
    *arc_mut = Arc::new(WrappedLineLayout {
        unwrapped_layout: line,
        wrap_boundaries,
        wrap_width,
        row_heights: Vec::new(),
    });
}

pub(crate) fn apply_inline_math_row_heights(
    wrapped: &mut WrappedLine,
    decoration: &LineDecoration,
    line_content: &str,
    buffer_to_display: &[usize],
    active_ranges: &[Range<usize>],
    image_cache: &HashMap<String, Option<Arc<RenderImage>>>,
    base_line_height: f32,
    display_offset: usize,
) {
    let formulas = decoration
        .inline_math_spans
        .iter()
        .filter_map(|source_range| {
            let trigger_range = source_range.start.saturating_sub(1)
                ..(source_range.end + 1).min(line_content.len());
            if active_ranges_intersect(active_ranges, &trigger_range) {
                return None;
            }
            let range = inline_math_display_range(decoration, source_range, buffer_to_display)?;
            if range.start < display_offset || range.end > display_offset + wrapped.text.len() {
                return None;
            }
            let local_range = range.start - display_offset..range.end - display_offset;
            if !wrapped
                .text
                .get(local_range)?
                .chars()
                .all(|ch| ch == '\u{a0}')
            {
                return None;
            }
            let source = line_content.get(source_range.clone())?;
            let image = image_cache.get(&inline_math_cache_key(source))?.as_ref()?;
            let image_size = image.size(0);
            let (_, height) = intrinsic_generated_preview_size_for_kind(
                image_size.width.0 as f32,
                image_size.height.0 as f32,
                BlockPreviewKind::Math,
            );
            Some((range.start - display_offset, height))
        })
        .collect::<Vec<_>>();
    let boundaries = wrap_boundary_byte_indices(wrapped, &wrapped.text);
    let heights =
        super::text_layout::inline_math_row_heights(base_line_height, &boundaries, &formulas);
    if heights.iter().all(|height| *height == base_line_height) {
        wrapped.set_row_heights(Vec::new());
    } else {
        wrapped.set_row_heights(heights.into_iter().map(px).collect());
    }
}

pub(crate) fn build_generated_block_preview_svg(
    kind: BlockPreviewKind,
    source_text: &str,
    theme: &Theme,
    font_families: GeneratedPreviewFontFamilies<'_>,
) -> String {
    let normalized = source_text.trim();
    let rendered = match kind {
        BlockPreviewKind::Math => render_math_block_preview_svg(normalized, theme),
        BlockPreviewKind::Mermaid => {
            render_mermaid_block_preview_svg(normalized, theme, font_families)
        }
    };

    match rendered {
        Ok(svg) if svg.contains("<svg") => svg,
        Ok(_) => build_generated_block_preview_error_svg(
            kind,
            "renderer returned invalid SVG",
            theme,
            font_families,
        ),
        Err(error) => {
            build_generated_block_preview_error_svg(kind, &error.to_string(), theme, font_families)
        }
    }
}

fn render_math_block_preview_svg(source_text: &str, theme: &Theme) -> anyhow::Result<String> {
    let options = MathJaxOptions {
        font_size: 36.0,
        horizontal_align: HorizontalAlign::Center,
    };
    let normalized_source = normalize_math_preview_source(source_text);
    let svg = render_mathjax_preview_svg(&normalized_source, &options)?;
    let svg = apply_math_preview_theme(&svg, theme);
    scale_math_preview_svg(&svg, 1.5)
}

fn scale_math_preview_svg(svg: &str, scale: f32) -> anyhow::Result<String> {
    // MathJax emits ex-sized roots: changing its font_size alone does not change
    // the size that usvg (and therefore the editor) assigns to the SVG.
    let size = usvg::Tree::from_str(svg, &usvg::Options::default())?.size();
    let mut scaled = svg.to_string();
    for (attribute, value) in [("width", size.width()), ("height", size.height())] {
        let root_end = scaled
            .find("<svg")
            .and_then(|start| scaled[start..].find('>').map(|end| start + end))
            .ok_or_else(|| anyhow::anyhow!("MathJax output has no SVG root"))?;
        let marker = format!(" {attribute}=\"");
        let start = scaled[..root_end]
            .find(&marker)
            .ok_or_else(|| anyhow::anyhow!("MathJax SVG root has no {attribute}"))?
            + marker.len();
        let end = start
            + scaled[start..root_end]
                .find('"')
                .ok_or_else(|| anyhow::anyhow!("MathJax SVG root has invalid {attribute}"))?;
        scaled.replace_range(start..end, &format!("{}px", value * scale));
    }
    Ok(scaled)
}

fn normalize_math_preview_source(source_text: &str) -> String {
    let mut normalized = source_text.trim().to_string();

    for (from, to) in [
        ("\\operatorname*{arg\\,min}", "\\arg\\min"),
        ("\\operatorname*{arg\\min}", "\\arg\\min"),
        ("\\operatorname{arg\\,min}", "\\arg\\min"),
        ("\\operatorname{arg\\min}", "\\arg\\min"),
        ("\\arg\\min", "\\min"),
        ("\\mathbb{R}", "\\mathbf{R}"),
        ("\\mathbb{N}", "\\mathbf{N}"),
        ("\\mathbb{Z}", "\\mathbf{Z}"),
        ("\\mathbb{Q}", "\\mathbf{Q}"),
        ("\\mathbb{C}", "\\mathbf{C}"),
        ("\\texttt{", "\\mathrm{"),
        ("\\textbf{", "\\mathbf{"),
        ("\\textit{", "\\mathit{"),
        ("\\textrm{", "\\mathrm{"),
        ("\\textsf{", "\\mathsf{"),
        ("\\ell", "L"),
        ("\\lVert", "\\Vert"),
        ("\\rVert", "\\Vert"),
    ] {
        normalized = normalized.replace(from, to);
    }

    for token in ["\\bigl", "\\bigr", "\\Bigl", "\\Bigr"] {
        normalized = normalized.replace(token, "");
    }

    normalized
}

fn mathjax_error_requires_reset(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("worker thread is unavailable")
        || normalized.contains("stopped before rendering finished")
        || normalized.contains("failed to call render function")
        || normalized.contains("runtimelimit")
}

fn render_mathjax_preview_svg(
    source_text: &str,
    options: &MathJaxOptions,
) -> anyhow::Result<String> {
    let mut renderer = generated_mathjax_renderer()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    match renderer.render_tex(source_text, options) {
        Ok(svg) => Ok(svg),
        Err(error) => {
            let message = error.to_string();
            if !mathjax_error_requires_reset(&message) {
                return Err(anyhow::anyhow!(message));
            }

            *renderer = MathJax::new();
            renderer
                .render_tex(source_text, options)
                .map_err(|retry_error| {
                    anyhow::anyhow!("{}; retry failed: {}", message, retry_error)
                })
        }
    }
}

fn render_mermaid_block_preview_svg(
    source_text: &str,
    theme: &Theme,
    font_families: GeneratedPreviewFontFamilies<'_>,
) -> anyhow::Result<String> {
    let font_family = build_generated_preview_font_family(font_families);
    let is_sequence = matches!(
        merman::detect::DetectorRegistry::new()
            .detect_type(source_text, &mut merman::MermaidConfig::default()),
        Ok("sequence")
    );
    let renderer = merman::render::HeadlessRenderer::new().with_vendored_text_measurer();
    let renderer = if let Some(preset) = font_families
        .mermaid_theme
        .host_preset(theme.bg_primary.l < 0.5)
    {
        let mut host_theme = merman::render::HostThemeProfile::from_preset(preset);
        host_theme.font_family = Some(font_family);
        host_theme.font_size = Some(if is_sequence { "18px" } else { "14px" }.to_string());
        if is_sequence {
            // Sequence layout reads the numeric root fontSize, not the theme variable.
            host_theme
                .site_config
                .insert("fontSize".into(), serde_json::json!(18));
        }
        host_theme.output = merman::render::HostThemeOutput {
            root_background: merman::render::HostThemeRootBackground::None,
            ..merman::render::HostThemeOutput::resvg_safe_editor()
        };
        renderer.with_host_theme(&host_theme)
    } else {
        let mermaid_theme = font_families
            .mermaid_theme
            .mermaid_theme_name()
            .ok_or_else(|| anyhow::anyhow!("invalid Mermaid theme"))?;
        let mut config = serde_json::json!({
            "theme": mermaid_theme,
            "fontFamily": font_family,
        });
        if is_sequence {
            config["fontSize"] = serde_json::json!(18);
            config["themeVariables"] = serde_json::json!({ "fontSize": "18px" });
        }
        renderer
            .with_site_config(merman::MermaidConfig::from_value(config))
            .with_svg_pipeline(merman::render::SvgPipeline::resvg_safe())
    };
    let svg = renderer
        .render_svg_sync(source_text)?
        .ok_or_else(|| anyhow::anyhow!("unsupported Mermaid diagram"))?;
    // merman 0.8.0-alpha.3 omits this semicolon in fallback font-family attributes.
    let svg = svg.replace("&apos\"", "&apos;\"");
    Ok(append_svg_root_style(
        &svg,
        "background-color:transparent;text-rendering:geometricPrecision;shape-rendering:geometricPrecision;",
    ))
}

fn build_generated_block_preview_error_svg(
    kind: BlockPreviewKind,
    message: &str,
    theme: &Theme,
    font_families: GeneratedPreviewFontFamilies<'_>,
) -> String {
    let title = match kind {
        BlockPreviewKind::Math => "LaTeX preview failed",
        BlockPreviewKind::Mermaid => "Mermaid preview failed",
    };
    let title_color = svg_color(theme.text_danger);
    let text_color = svg_color(theme.text_secondary);
    let lines = wrap_svg_text(message, 52, 5);
    let width = 920;
    let height = 140 + lines.len() as i32 * 24;
    let mut text_nodes = String::new();
    let font_family = build_generated_preview_font_family(font_families);

    for (index, line) in lines.iter().enumerate() {
        let y = 102 + index as i32 * 24;
        text_nodes.push_str(&format!(
            "<text x=\"48\" y=\"{y}\" font-family=\"{font_family}\" font-size=\"16\" fill=\"{text_color}\">{}</text>",
            escape_svg_text(line)
        ));
    }

    let background = svg_color(theme.bg_primary);

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><rect width=\"100%\" height=\"100%\" fill=\"{background}\"/><text x=\"48\" y=\"62\" font-family=\"{font_family}\" font-size=\"18\" font-weight=\"700\" fill=\"{title_color}\">{title}</text>{text_nodes}</svg>",
        background = background,
        font_family = font_family,
        title = escape_svg_text(title),
        title_color = title_color,
    )
}

fn build_generated_preview_font_family(font_families: GeneratedPreviewFontFamilies<'_>) -> String {
    let mut families = Vec::<String>::new();

    push_generated_preview_font_families(
        &mut families,
        sanitize_generated_preview_font_family(font_families.monospace, true),
    );
    push_generated_preview_font_families(
        &mut families,
        sanitize_generated_preview_font_family(font_families.body_cjk, false),
    );
    push_generated_preview_font_families(
        &mut families,
        GENERATED_PREVIEW_DEFAULT_MONOSPACE_FONT_FAMILY,
    );
    push_generated_preview_font_families(
        &mut families,
        GENERATED_PREVIEW_DEFAULT_EMOJI_FONT_FAMILY,
    );

    families.join(", ")
}

fn sanitize_generated_preview_font_family(configured: &str, monospace: bool) -> &str {
    let configured = configured.trim();
    if configured.is_empty() || configured == ".SystemUIFont" {
        if monospace {
            GENERATED_PREVIEW_DEFAULT_MONOSPACE_FONT_FAMILY
        } else {
            GENERATED_PREVIEW_DEFAULT_CJK_FONT_FAMILY
        }
    } else {
        configured
    }
}

fn push_generated_preview_font_families(families: &mut Vec<String>, configured: &str) {
    for family in configured
        .split(',')
        .map(str::trim)
        .filter(|family| !family.is_empty())
    {
        let normalized = family.trim_matches(|ch| ch == '\'' || ch == '"');
        if families.iter().any(|existing| {
            existing
                .trim_matches(|ch| ch == '\'' || ch == '"')
                .eq_ignore_ascii_case(normalized)
        }) {
            continue;
        }
        families.push(if normalized.contains(char::is_whitespace) {
            format!("'{}'", normalized.replace('\'', "\\'"))
        } else {
            normalized.to_string()
        });
    }
}

fn apply_math_preview_theme(svg: &str, theme: &Theme) -> String {
    let text_color = svg_color(generated_preview_foreground_color(theme));
    let mut themed = append_svg_root_style(
        svg,
        &format!(
            "background-color:transparent;color:{text_color};text-rendering:geometricPrecision;shape-rendering:geometricPrecision;stroke-linecap:round;stroke-linejoin:round;"
        ),
    );

    for (from, to) in [
        ("fill=\"black\"", format!("fill=\"{text_color}\"")),
        ("stroke=\"black\"", format!("stroke=\"{text_color}\"")),
        ("fill=\"#000\"", format!("fill=\"{text_color}\"")),
        ("stroke=\"#000\"", format!("stroke=\"{text_color}\"")),
        ("fill=\"#000000\"", format!("fill=\"{text_color}\"")),
        ("stroke=\"#000000\"", format!("stroke=\"{text_color}\"")),
        ("color: black", format!("color: {text_color}")),
        ("color:black", format!("color:{text_color}")),
    ] {
        themed = themed.replace(from, &to);
    }

    themed
}

fn generated_preview_foreground_color(theme: &Theme) -> Hsla {
    if theme.bg_primary.l < 0.4 {
        theme.text_bright
    } else {
        theme.text_primary
    }
}

fn append_svg_root_style(svg: &str, style_fragment: &str) -> String {
    let Some(svg_start) = svg.find("<svg") else {
        return svg.to_string();
    };
    let Some(tag_end_rel) = svg[svg_start..].find('>') else {
        return svg.to_string();
    };
    let tag_end = svg_start + tag_end_rel;
    let opening_tag = &svg[svg_start..tag_end];

    if let Some(style_attr_rel) = opening_tag.find("style=\"") {
        let style_value_start = svg_start + style_attr_rel + 7;
        let Some(style_value_end_rel) = svg[style_value_start..].find('"') else {
            return svg.to_string();
        };
        let style_value_end = style_value_start + style_value_end_rel;
        let mut merged = String::with_capacity(svg.len() + style_fragment.len() + 1);
        merged.push_str(&svg[..style_value_end]);
        if !svg[style_value_start..style_value_end]
            .trim_end()
            .ends_with(';')
        {
            merged.push(';');
        }
        merged.push_str(style_fragment);
        merged.push_str(&svg[style_value_end..]);
        return merged;
    }

    let mut merged = String::with_capacity(svg.len() + style_fragment.len() + 9);
    merged.push_str(&svg[..tag_end]);
    merged.push_str(&format!(" style=\"{style_fragment}\""));
    merged.push_str(&svg[tag_end..]);
    merged
}

fn svg_color(color: Hsla) -> String {
    crate::theme::hsla_to_hex_rgba(color)
}

fn wrap_svg_text(source_text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();

    for raw_line in source_text.lines() {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            let separator = if current.is_empty() { 0 } else { 1 };
            if current.chars().count() + separator + word.chars().count() > max_chars
                && !current.is_empty()
            {
                lines.push(current);
                current = String::new();
                if lines.len() == max_lines {
                    return lines;
                }
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }

        lines.push(current);
        if lines.len() == max_lines {
            return lines;
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines.truncate(max_lines);
    lines
}

fn escape_svg_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(crate) fn load_render_image_from_path(path: &PathBuf) -> anyhow::Result<RenderImage> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
    {
        let svg = std::fs::read_to_string(path)?;
        return load_render_image_from_svg_with_scale(&svg, 1.0);
    }

    let bytes = std::fs::read(path)?;
    load_render_image_from_bytes(&bytes)
}

pub(crate) fn load_render_image_from_url(url: &str) -> anyhow::Result<RenderImage> {
    let response = ureq::get(url).call()?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)?;
    load_render_image_from_bytes(&bytes)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn pick_image_file_blocking() -> Option<PathBuf> {
    crate::platform::dialog::run(|| {
        rfd::FileDialog::new()
            .set_title("Choose Image")
            .add_filter(
                "Images",
                &[
                    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "svg", "heic",
                    "heif", "avif",
                ],
            )
            .pick_file()
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn pick_markdown_file_blocking(title: &str) -> Option<PathBuf> {
    let title = title.to_string();
    crate::platform::dialog::run(move || {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter("Markdown", &["md", "markdown"])
            .pick_file()
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn pick_folder_blocking(title: &str) -> Option<PathBuf> {
    let title = title.to_string();
    crate::platform::dialog::run(move || rfd::FileDialog::new().set_title(title).pick_folder())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn save_file_blocking(default_name: &str, title: &str) -> Option<PathBuf> {
    let default_name = default_name.to_string();
    let title = title.to_string();
    crate::platform::dialog::run(move || {
        rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(default_name)
            .add_filter("Markdown", &["md", "markdown"])
            .save_file()
    })
}

/// Map a buffer byte offset (within line) to display byte offset.
pub(crate) fn buffer_to_display_offset(buffer_to_display: &[usize], buf_offset: usize) -> usize {
    if buf_offset < buffer_to_display.len() {
        buffer_to_display[buf_offset]
    } else if let Some(&last) = buffer_to_display.last() {
        last
    } else {
        0
    }
}

/// Select a content-independent baseline font using the same block roles as
/// `build_text_runs_for_display_with_language`, not the first visible text run.
pub(crate) fn line_baseline_font(
    decoration: &LineDecoration,
    force_monospace: bool,
    body_font: &Font,
    heading_font: &Font,
    monospace_font: &Font,
) -> Font {
    let is_front_matter = decoration
        .table_info
        .as_ref()
        .is_some_and(|table| table.is_front_matter);
    let is_code = force_monospace
        || is_front_matter
        || matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        );
    let is_heading = matches!(decoration.block_type, BlockType::Heading(_));
    let is_header = decoration
        .table_info
        .as_ref()
        .is_some_and(|table| table.is_header && !table.is_front_matter);
    let mut font = if is_code {
        monospace_font.clone()
    } else if is_heading {
        heading_font.clone()
    } else {
        body_font.clone()
    };
    font.weight = if !is_code && (is_heading || is_header) {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    };
    font.style = FontStyle::Normal;
    font
}

/// Shape a fixed Latin probe through the same platform backend as the content.
/// Raw FontMetrics descent is signed, unlike LineLayout descent, and Windows
/// also includes line-gap handling in shaping; do not mix those metric sources.
/// Cache the baseline by font, size, and line height within a single prepaint pass.
pub(crate) fn stable_text_baseline(
    font: &Font,
    font_size: Pixels,
    line_height: Pixels,
    cache: &mut Vec<(Font, Pixels, Pixels, Pixels)>,
    window: &Window,
) -> Pixels {
    if let Some((_, _, _, baseline)) =
        cache
            .iter()
            .find(|(cached_font, cached_font_size, cached_line_height, _)| {
                cached_font == font
                    && *cached_font_size == font_size
                    && *cached_line_height == line_height
            })
    {
        return *baseline;
    }

    let reference = window.text_system().shape_line(
        "M".into(),
        font_size,
        &[TextRun {
            len: 1,
            font: font.clone(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    let baseline = (line_height - reference.ascent - reference.descent) / 2.0 + reference.ascent;
    cache.push((font.clone(), font_size, line_height, baseline));
    baseline
}

/// Keep the original layout (including fallback glyphs and their metrics) and
/// row geometry intact. Only GPUI's paint origin needs baseline compensation.
pub(crate) fn baseline_aligned_text_origin(
    wrapped: &WrappedLine,
    origin: gpui::Point<Pixels>,
    line_height: Pixels,
    reference_baseline: Pixels,
) -> gpui::Point<Pixels> {
    origin
        + point(
            px(0.0),
            px(super::text_layout::text_baseline_paint_offset(
                f32::from(line_height),
                f32::from(reference_baseline),
                f32::from(wrapped.ascent()),
                f32::from(wrapped.descent()),
            )),
        )
}

/// Build styled TextRuns for the display text.
pub(crate) fn build_text_runs_for_display(
    display_text: &str,
    line_content: &str,
    display_to_buffer: &[usize],
    decoration: &LineDecoration,
    active_ranges: &[Range<usize>],
    code_block_syntax_active: bool,
    prefix_collapsed: bool,
    body_font: &Font,
    body_cjk_font: &Font,
    monospace_font: &Font,
    heading_font: &Font,
    heading_cjk_font: &Font,
    italic_font: &Font,
    italic_cjk_font: &Font,
    collapsed_task_prefix_chars: usize,
    font_size: f32,
    theme: &Theme,
    diff_inline_added_bg: Hsla,
    diff_inline_removed_bg: Hsla,
    wiki_link_text_color: Hsla,
    wiki_link_text_weight: FontWeight,
    list_marker_text_color: Hsla,
    list_marker_font_weight: FontWeight,
    heading_marker_text_color: Hsla,
    front_matter_key_text_color: Option<Hsla>,
    force_monospace: bool,
) -> Vec<TextRun> {
    build_text_runs_for_display_with_language(
        display_text,
        line_content,
        display_to_buffer,
        decoration,
        active_ranges,
        code_block_syntax_active,
        prefix_collapsed,
        body_font,
        body_cjk_font,
        monospace_font,
        heading_font,
        heading_cjk_font,
        italic_font,
        italic_cjk_font,
        collapsed_task_prefix_chars,
        font_size,
        theme,
        diff_inline_added_bg,
        diff_inline_removed_bg,
        wiki_link_text_color,
        wiki_link_text_weight,
        list_marker_text_color,
        list_marker_font_weight,
        heading_marker_text_color,
        front_matter_key_text_color,
        force_monospace,
        None,
    )
}

pub(crate) fn build_text_runs_for_display_with_language(
    display_text: &str,
    line_content: &str,
    display_to_buffer: &[usize],
    decoration: &LineDecoration,
    active_ranges: &[Range<usize>],
    code_block_syntax_active: bool,
    prefix_collapsed: bool,
    body_font: &Font,
    body_cjk_font: &Font,
    monospace_font: &Font,
    heading_font: &Font,
    heading_cjk_font: &Font,
    italic_font: &Font,
    italic_cjk_font: &Font,
    collapsed_task_prefix_chars: usize,
    _font_size: f32,
    theme: &Theme,
    diff_inline_added_bg: Hsla,
    diff_inline_removed_bg: Hsla,
    wiki_link_text_color: Hsla,
    wiki_link_text_weight: FontWeight,
    list_marker_text_color: Hsla,
    list_marker_font_weight: FontWeight,
    heading_marker_text_color: Hsla,
    front_matter_key_text_color: Option<Hsla>,
    force_monospace: bool,
    document_code_language: Option<&str>,
) -> Vec<TextRun> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FontRole {
        BodyLatin,
        BodyCjk,
        HeadingLatin,
        HeadingCjk,
        ItalicLatin,
        ItalicCjk,
        Monospace,
    }

    let is_front_matter_line = decoration
        .table_info
        .as_ref()
        .is_some_and(|table_info| table_info.is_front_matter);
    let is_front_matter_separator = decoration
        .table_info
        .as_ref()
        .is_some_and(|table_info| table_info.is_front_matter && table_info.is_separator);
    let is_code_line = matches!(
        decoration.block_type,
        BlockType::CodeBlock | BlockType::CodeBlockFence
    ) || is_front_matter_line
        || force_monospace;
    let default_font = if matches!(decoration.block_type, BlockType::Heading(_)) {
        heading_font
    } else {
        body_font
    };
    if display_text.is_empty() {
        return vec![TextRun {
            len: 1,
            font: if is_code_line {
                monospace_font.clone()
            } else {
                default_font.clone()
            },
            color: theme.text_primary,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
    }

    let buf_len = line_content.len();
    let disp_len = display_text.len();

    if buf_len == 0 {
        return vec![TextRun {
            len: disp_len,
            font: if is_code_line {
                monospace_font.clone()
            } else {
                default_font.clone()
            },
            color: theme.text_primary,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
    }

    let mut buf_colors: Vec<Hsla> = vec![theme.text_primary; buf_len];
    let mut buf_weights: Vec<FontWeight> = vec![FontWeight::NORMAL; buf_len];
    let mut buf_font_styles: Vec<FontStyle> = vec![FontStyle::Normal; buf_len];
    let mut buf_bg_colors: Vec<Option<Hsla>> = vec![None; buf_len];
    let mut buf_strikethroughs: Vec<bool> = vec![false; buf_len];
    let mut buf_underlines: Vec<bool> = vec![false; buf_len];
    let mut buf_font_roles: Vec<FontRole> = vec![FontRole::BodyLatin; buf_len];
    let mut assign_font_roles =
        |range: Range<usize>, latin_role: FontRole, non_latin_role: FontRole| {
            let mut start = range.start.min(buf_len);
            let mut end = range.end.min(buf_len);
            while start > 0 && !line_content.is_char_boundary(start) {
                start -= 1;
            }
            while end < buf_len && !line_content.is_char_boundary(end) {
                end += 1;
            }
            if start >= end {
                return;
            }

            for (char_start, ch) in line_content[start..end].char_indices() {
                let absolute_start = start + char_start;
                let absolute_end = (absolute_start + ch.len_utf8()).min(end);
                let role = if prefers_latin_text_font(ch) {
                    latin_role
                } else {
                    non_latin_role
                };
                for index in absolute_start..absolute_end {
                    buf_font_roles[index] = role;
                }
            }
        };

    // For metadata separators the caller supplies whole-block focus, not row
    // focus. Transparent ink preserves measured height and source byte maps.
    if matches!(decoration.block_type, BlockType::CodeBlockFence) || is_front_matter_separator {
        let fence_active = code_block_syntax_active;
        let fence_color = if fence_active {
            let mut active = theme.text_secondary;
            active.a *= 0.5;
            active
        } else {
            gpui::transparent_black()
        };
        buf_colors.fill(fence_color);
    }

    let is_heading = matches!(decoration.block_type, BlockType::Heading(_));

    if is_code_line {
        // CJK in code must use the configured code font too (glyph-level
        // fallback covers missing CJK glyphs); routing it to the body CJK
        // font broke monospaced alignment for mixed-script code.
        assign_font_roles(0..buf_len, FontRole::Monospace, FontRole::Monospace);
    } else {
        for (start, ch) in line_content.char_indices() {
            let role = if prefers_latin_text_font(ch) {
                if is_heading {
                    FontRole::HeadingLatin
                } else {
                    FontRole::BodyLatin
                }
            } else if is_heading {
                FontRole::HeadingCjk
            } else {
                FontRole::BodyCjk
            };
            let end = start + ch.len_utf8();
            assign_font_roles(start..end.min(buf_len), role, role);
        }
    }

    let leading_ws = list_marker_indent(line_content);
    let trimmed = &line_content[leading_ws..];
    match decoration.block_type {
        _ if !decoration.table_cell_fragments.is_empty() => {
            for fragment in &decoration.table_cell_fragments {
                assign_font_roles(
                    fragment.range.start..fragment.range.start + fragment.prefix_len,
                    FontRole::Monospace,
                    FontRole::Monospace,
                );
            }
        }
        BlockType::UnorderedList => {
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
                assign_font_roles(
                    0..(leading_ws + 2).min(buf_len),
                    FontRole::Monospace,
                    FontRole::Monospace,
                );
            }
        }
        BlockType::OrderedList => {
            let prefix_len = markdown::ordered_list_prefix_len(trimmed);
            if prefix_len > 0 {
                assign_font_roles(
                    0..(leading_ws + prefix_len).min(buf_len),
                    FontRole::Monospace,
                    FontRole::Monospace,
                );
            }
        }
        _ => {}
    }

    match &decoration.block_type {
        BlockType::Heading(_) => {
            for c in buf_colors.iter_mut() {
                *c = theme.heading;
            }
            for w in buf_weights.iter_mut() {
                *w = FontWeight::BOLD;
            }
        }
        BlockType::Table => {
            if decoration
                .table_info
                .as_ref()
                .is_some_and(|ti| ti.is_header && !ti.is_front_matter)
            {
                for w in buf_weights.iter_mut() {
                    *w = FontWeight::BOLD;
                }
            }
        }
        _ => {}
    }

    let checked_bodies = if decoration.table_cell_fragments.is_empty() {
        let prefix_len = task_prefix_len(trimmed);
        if decoration.block_type == BlockType::TaskList(true) && prefix_len > 0 {
            vec![leading_ws + prefix_len..buf_len]
        } else {
            Vec::new()
        }
    } else {
        decoration
            .table_cell_fragments
            .iter()
            .filter(|fragment| {
                fragment.block_type == BlockType::TaskList(true) && fragment.prefix_len > 0
            })
            .map(|fragment| fragment.range.start + fragment.prefix_len..fragment.range.end)
            .collect()
    };
    for body in checked_bodies {
        for index in body {
            buf_colors[index] = theme.strikethrough;
            buf_strikethroughs[index] = true;
        }
    }

    let highlight_entire_line = matches!(decoration.block_type, BlockType::CodeBlock)
        || matches!(decoration.block_type, BlockType::Normal)
            && decoration.inline_spans.is_empty()
            && decoration.syntax_ranges.is_empty()
            && decoration.table_info.is_none();
    if highlight_entire_line
        && let Some(language) = document_code_language.or(decoration.code_language.as_deref())
    {
        let is_source_block =
            decoration.block_type == BlockType::Normal && document_code_language.is_none();
        if is_source_block && language == SOURCE_MATH_HIGHLIGHT_LANGUAGE {
            apply_language_highlighting(
                line_content,
                "latex",
                theme,
                &mut buf_colors,
                &mut buf_weights,
                &mut buf_font_styles,
                true,
            );
        } else {
            apply_code_block_highlighting(
                line_content,
                language,
                theme,
                &mut buf_colors,
                &mut buf_weights,
                &mut buf_font_styles,
            );
        }
        if is_source_block {
            apply_source_block_marker_colors(line_content, language, theme, &mut buf_colors);
        }
    }

    for span in &decoration.inline_spans {
        let start = span.range.start.min(buf_len);
        let end = span.range.end.min(buf_len);
        for i in start..end {
            match span.kind {
                InlineKind::Bold => {
                    buf_colors[i] = theme.bold;
                    buf_weights[i] = FontWeight::EXTRA_BOLD;
                }
                InlineKind::Italic => {
                    buf_colors[i] = theme.italic;
                    buf_font_styles[i] = FontStyle::Italic;
                }
                InlineKind::Strikethrough => {
                    buf_colors[i] = theme.strikethrough;
                    buf_strikethroughs[i] = true;
                }
                InlineKind::Underline => {
                    buf_underlines[i] = true;
                }
                InlineKind::InlineCode => {
                    buf_colors[i] = theme.inline_code;
                    buf_bg_colors[i] = Some(theme.inline_code_bg);
                }
                InlineKind::Link => {
                    buf_colors[i] = theme.link;
                    buf_weights[i] = FontWeight::BOLD;
                }
                InlineKind::Image => {
                    buf_colors[i] = theme.text_primary;
                }
            }
        }

        if matches!(span.kind, InlineKind::InlineCode) {
            assign_font_roles(start..end, FontRole::Monospace, FontRole::Monospace);
            for syntax_range in decoration.syntax_ranges.iter().filter(|syntax_range| {
                syntax_range.trigger_range.start <= span.range.start
                    && syntax_range.trigger_range.end >= span.range.end
            }) {
                let syntax_start = syntax_range.range.start.min(buf_len);
                let syntax_end = syntax_range.range.end.min(buf_len);
                for index in syntax_start..syntax_end {
                    buf_bg_colors[index] = Some(theme.inline_code_bg);
                }
                assign_font_roles(
                    syntax_start..syntax_end,
                    FontRole::Monospace,
                    FontRole::Monospace,
                );
            }
        } else if matches!(span.kind, InlineKind::Italic) && !is_code_line {
            assign_font_roles(start..end, FontRole::ItalicLatin, FontRole::ItalicCjk);
        }
    }

    for highlight_range in &decoration.highlight_spans {
        let start = highlight_range.start.min(buf_len);
        let end = highlight_range.end.min(buf_len);
        for index in start..end {
            buf_bg_colors[index] = Some(theme.highlight_bg);
            buf_colors[index] = theme.bold;
            buf_weights[index] = FontWeight::BOLD;
        }
    }

    for range in decoration
        .inline_spans
        .iter()
        .filter(|span| span.kind == InlineKind::InlineCode)
        .map(|span| &span.range)
    {
        // Opaque semantic spans own color, background, and font after all
        // presentation layers; orthogonal weight/style/decoration still stack.
        let start = range.start.min(buf_len);
        let end = range.end.min(buf_len);
        for index in start..end {
            buf_colors[index] = theme.inline_code;
            buf_bg_colors[index] = Some(theme.inline_code_bg);
        }
        assign_font_roles(start..end, FontRole::Monospace, FontRole::Monospace);
    }

    for superscript_range in &decoration.superscript_spans {
        if inline_range_is_collapsed(decoration, superscript_range, active_ranges) {
            let start = superscript_range.start.min(buf_len);
            let end = superscript_range.end.min(buf_len);
            for index in start..end {
                buf_colors[index] = gpui::transparent_black();
            }
        }
    }

    for subscript_range in &decoration.subscript_spans {
        if inline_range_is_collapsed(decoration, subscript_range, active_ranges) {
            let start = subscript_range.start.min(buf_len);
            let end = subscript_range.end.min(buf_len);
            for index in start..end {
                buf_colors[index] = gpui::transparent_black();
            }
        }
    }

    // Cross-fragment emphasis must not change the reserved marker advances.
    for fragment in &decoration.table_cell_fragments {
        let prefix = fragment.range.start..fragment.range.start + fragment.prefix_len;
        assign_font_roles(prefix.clone(), FontRole::Monospace, FontRole::Monospace);
        for index in prefix {
            buf_font_styles[index] = FontStyle::Normal;
            buf_weights[index] = list_marker_font_weight;
        }
    }

    for footnote_reference in &decoration.footnote_references {
        if decoration.footnote_definition_label.is_none()
            && !active_ranges_intersect(active_ranges, &footnote_reference.full_range)
        {
            let start = footnote_reference.collapsed_range.start.min(buf_len);
            let end = footnote_reference.collapsed_range.end.min(buf_len);
            for index in start..end {
                buf_colors[index] = gpui::transparent_black();
            }
        }
    }

    for wiki_link in &decoration.wiki_links {
        let target_start = (wiki_link.full_range.start + 2).min(buf_len);
        let target_end = wiki_link.full_range.end.saturating_sub(2).min(buf_len);
        for index in target_start..target_end {
            buf_colors[index] = wiki_link_text_color;
            buf_weights[index] = wiki_link_text_weight;
        }
    }

    if is_front_matter_line && !is_front_matter_separator {
        buf_colors.fill(theme.text_primary);
        buf_weights.fill(FontWeight::NORMAL);
        buf_font_styles.fill(FontStyle::Normal);
        buf_bg_colors.fill(None);
        buf_strikethroughs.fill(false);
        buf_underlines.fill(false);
        if let Some(key_range) = decoration
            .front_matter
            .as_ref()
            .and_then(|info| info.key_range.as_ref())
        {
            let key_color = front_matter_key_text_color
                .unwrap_or_else(|| crate::theme::with_alpha(theme.text_bright, 1.0));
            let start = key_range.start.min(buf_len);
            let end = key_range.end.min(buf_len);
            for index in start..end {
                buf_colors[index] = key_color;
                buf_weights[index] = FontWeight::BOLD;
            }
        }
        if let Some(info) = decoration.front_matter.as_ref()
            && matches!(info.value, FrontMatterValue::BlockHeader)
            && front_matter_row_is_active(active_ranges, line_content.len())
        {
            for color in &mut buf_colors
                [info.value_range.start.min(buf_len)..info.value_range.end.min(buf_len)]
            {
                *color = theme.syntax_marker;
            }
        }
    }

    if !active_ranges.is_empty() {
        if !matches!(decoration.block_type, BlockType::Table) {
            for sr in &decoration.syntax_ranges {
                let syntax_is_active = if matches!(decoration.block_type, BlockType::Heading(_)) {
                    heading_syntax_is_active(decoration, active_ranges)
                } else if matches!(decoration.block_type, BlockType::CodeBlockFence) {
                    // Fence lines always have syntax visible; use active code block check
                    // to determine if markers should get syntax_marker color
                    code_block_syntax_active
                        || active_ranges_intersect(active_ranges, &sr.trigger_range)
                } else {
                    active_ranges_intersect(active_ranges, &sr.trigger_range)
                };
                if syntax_is_active {
                    let start = sr.range.start.min(buf_len);
                    let end = sr.range.end.min(buf_len);
                    let syntax_color = match decoration.block_type {
                        BlockType::Heading(_) => heading_marker_text_color,
                        BlockType::UnorderedList
                        | BlockType::OrderedList
                        | BlockType::TaskList(_) => list_marker_text_color,
                        _ => theme.syntax_marker,
                    };
                    for i in start..end {
                        buf_colors[i] = syntax_color;
                        if matches!(
                            decoration.block_type,
                            BlockType::UnorderedList
                                | BlockType::OrderedList
                                | BlockType::TaskList(_)
                        ) {
                            buf_weights[i] = list_marker_font_weight;
                        }
                    }
                    assign_font_roles(start..end, FontRole::Monospace, FontRole::Monospace);
                }
            }
        }
    } else if matches!(decoration.block_type, BlockType::CodeBlockFence) && code_block_syntax_active
    {
        for sr in &decoration.syntax_ranges {
            let start = sr.range.start.min(buf_len);
            let end = sr.range.end.min(buf_len);
            for i in start..end {
                buf_colors[i] = theme.syntax_marker;
            }
            assign_font_roles(start..end, FontRole::Monospace, FontRole::Monospace);
        }
    }

    for span in &decoration.diff_removed_spans {
        let start = span.start.min(buf_len);
        let end = span.end.min(buf_len);
        for index in start..end {
            buf_bg_colors[index] = Some(diff_inline_removed_bg);
            buf_strikethroughs[index] = true;
        }
    }

    for span in &decoration.diff_added_spans {
        let start = span.start.min(buf_len);
        let end = span.end.min(buf_len);
        for index in start..end {
            buf_bg_colors[index] = Some(diff_inline_added_bg);
        }
    }

    let mut colors: Vec<Hsla> = Vec::with_capacity(disp_len);
    let mut weights: Vec<FontWeight> = Vec::with_capacity(disp_len);
    let mut font_styles: Vec<FontStyle> = Vec::with_capacity(disp_len);
    let mut bg_colors: Vec<Option<Hsla>> = Vec::with_capacity(disp_len);
    let mut strikethroughs: Vec<bool> = Vec::with_capacity(disp_len);
    let mut underlines: Vec<bool> = Vec::with_capacity(disp_len);
    let mut font_roles: Vec<FontRole> = Vec::with_capacity(disp_len);

    for i in 0..disp_len {
        let buf_idx = if i < display_to_buffer.len() {
            display_to_buffer[i]
        } else {
            buf_len.saturating_sub(1)
        };
        let buf_idx = buf_idx.min(buf_len.saturating_sub(1));
        colors.push(buf_colors[buf_idx]);
        weights.push(buf_weights[buf_idx]);
        font_styles.push(buf_font_styles[buf_idx]);
        bg_colors.push(buf_bg_colors[buf_idx]);
        strikethroughs.push(buf_strikethroughs[buf_idx]);
        underlines.push(buf_underlines[buf_idx]);
        font_roles.push(buf_font_roles[buf_idx]);
    }

    if let Some(info) = decoration.front_matter.as_ref() {
        let active = front_matter_row_is_active(active_ranges, line_content.len());
        if !active && matches!(info.value, FrontMatterValue::Boolean(_)) {
            for (i, ch) in display_text.char_indices() {
                let source = display_to_buffer.get(i).copied().unwrap_or(buf_len);
                if ch == '\u{a0}' && info.value_range.contains(&source) {
                    colors[i..i + ch.len_utf8()].fill(gpui::transparent_black());
                }
            }
        }
        if !active
            && prefix_collapsed
            && !info.display_prefix.is_empty()
            && let Some(marker_start) = display_text.find(&info.display_prefix)
        {
            let marker_end = marker_start + info.display_prefix.len();
            colors[marker_start..marker_end].fill(list_marker_text_color);
            weights[marker_start..marker_end].fill(list_marker_font_weight);
            font_roles[marker_start..marker_end].fill(FontRole::Monospace);
        }
    }

    if let Some(label) = decoration.footnote_definition_label.as_deref() {
        let leading_ws = display_text.len() - display_text.trim_start_matches([' ', '\t']).len();
        let raw_prefix = format!("[^{}]:", label);
        let display_prefix = if line_content
            .get(leading_ws..)
            .is_some_and(|content| content.starts_with(&(raw_prefix.clone() + " ")))
        {
            format!("{}: ", label)
        } else {
            format!("{}:", label)
        };

        if display_text
            .get(leading_ws..)
            .is_some_and(|content| content.starts_with(&display_prefix))
        {
            let style_end = (leading_ws + display_prefix.trim_end().len()).min(disp_len);
            for index in leading_ws..style_end {
                colors[index] = theme.bold;
                weights[index] = FontWeight::BOLD;
            }
        }
    }

    if prefix_collapsed && decoration.table_cell_fragments.is_empty() {
        let leading_ws = display_text.len() - display_text.trim_start_matches([' ', '\t']).len();
        let prefix_len = compute_display_prefix_bytes(
            display_text,
            &decoration.block_type,
            true,
            collapsed_task_prefix_chars,
        )
        .min(disp_len);
        match &decoration.block_type {
            BlockType::UnorderedList => {
                let bullet_end = (leading_ws + "• ".len()).min(prefix_len);
                for i in leading_ws..bullet_end {
                    colors[i] = list_marker_text_color;
                    weights[i] = list_marker_font_weight;
                    font_roles[i] = FontRole::Monospace;
                }
            }
            BlockType::TaskList(_) => {
                let placeholder_start = leading_ws.saturating_sub(collapsed_task_prefix_chars);
                let placeholder_end = leading_ws.min(disp_len);
                for i in placeholder_start..placeholder_end {
                    colors[i] = gpui::transparent_black();
                    font_roles[i] = FontRole::Monospace;
                }
            }
            BlockType::OrderedList => {
                for i in leading_ws..prefix_len {
                    colors[i] = list_marker_text_color;
                    weights[i] = list_marker_font_weight;
                    font_roles[i] = FontRole::Monospace;
                }
            }
            _ => {}
        }

        if matches!(decoration.block_type, BlockType::TaskList(true)) {
            for strike in strikethroughs.iter_mut().take(prefix_len) {
                *strike = false;
            }
        }
    }

    let mut runs = Vec::new();
    let mut run_start = 0;
    while run_start < disp_len {
        let color = colors[run_start];
        let weight = weights[run_start];
        let style = font_styles[run_start];
        let bg = bg_colors[run_start];
        let strike = strikethroughs[run_start];
        let underline = underlines[run_start];

        let mut run_end = run_start + 1;
        while run_end < disp_len
            && colors[run_end] == color
            && weights[run_end] == weight
            && font_styles[run_end] == style
            && bg_colors[run_end] == bg
            && strikethroughs[run_end] == strike
            && underlines[run_end] == underline
            && font_roles[run_end] == font_roles[run_start]
        {
            run_end += 1;
        }

        while run_end < disp_len && !display_text.is_char_boundary(run_end) {
            run_end += 1;
        }

        let mut font = match font_roles[run_start] {
            FontRole::BodyLatin => body_font.clone(),
            FontRole::BodyCjk => body_cjk_font.clone(),
            FontRole::HeadingLatin => heading_font.clone(),
            FontRole::HeadingCjk => heading_cjk_font.clone(),
            FontRole::ItalicLatin => italic_font.clone(),
            FontRole::ItalicCjk => italic_cjk_font.clone(),
            FontRole::Monospace => monospace_font.clone(),
        };
        font.weight = weight;
        font.style = if font_roles[run_start] == FontRole::ItalicCjk && style == FontStyle::Italic {
            FontStyle::Oblique
        } else {
            style
        };

        runs.push(TextRun {
            len: run_end - run_start,
            font,
            color,
            background_color: bg,
            underline: if underline {
                Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(color),
                    wavy: false,
                })
            } else {
                None
            },
            strikethrough: if strike {
                Some(StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(theme.strikethrough),
                })
            } else {
                None
            },
        });

        run_start = run_end;
    }

    runs
}

/// Narrows sorted, non-overlapping matches to the ones covering `[start, end)`.
/// The returned index is where the slice begins, so callers can still line it up
/// with an active-match index.
pub(crate) fn matches_overlapping_range(
    matches: &[std::ops::Range<usize>],
    start: usize,
    end: usize,
) -> (usize, &[std::ops::Range<usize>]) {
    let first = matches.partition_point(|range| range.end <= start);
    let rest = &matches[first..];
    let len = rest.partition_point(|range| range.start < end);
    (first, &rest[..len])
}

pub(crate) fn apply_search_match_text_styles(
    runs: &mut Vec<TextRun>,
    display_text: &str,
    display_to_buffer: &[usize],
    line_start_offset: usize,
    search_matches: &[std::ops::Range<usize>],
    active_match_index: Option<usize>,
    inactive_color: Hsla,
    active_color: Hsla,
) {
    if runs.is_empty() || display_text.is_empty() || search_matches.is_empty() {
        return;
    }

    // Matches are sorted and non-overlapping, so a line only has to look at the
    // slice that actually covers it. Scanning all of them here made every frame
    // O(visible lines * total matches).
    let Some(line_span_end) = display_to_buffer
        .iter()
        .take(display_text.len())
        .copied()
        .max()
    else {
        return;
    };
    let line_end_offset = line_start_offset + line_span_end + 1;
    let (first, visible) =
        matches_overlapping_range(search_matches, line_start_offset, line_end_offset);
    if visible.is_empty() {
        return;
    }

    let mut byte_colors: Vec<Option<Hsla>> = vec![None; display_text.len()];
    for (offset, search_match) in visible.iter().enumerate() {
        let color = if active_match_index == Some(first + offset) {
            active_color
        } else {
            inactive_color
        };
        for (display_index, &buffer_index) in display_to_buffer
            .iter()
            .take(display_text.len())
            .enumerate()
        {
            let absolute_offset = line_start_offset + buffer_index;
            if absolute_offset >= search_match.start && absolute_offset < search_match.end {
                byte_colors[display_index] = Some(color);
            }
        }
    }

    let original_runs = std::mem::take(runs);
    let mut styled_runs = Vec::with_capacity(original_runs.len());
    let mut run_start = 0usize;
    for run in original_runs {
        if run_start >= display_text.len() {
            break;
        }
        let run_end = (run_start + run.len).min(display_text.len());
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

/// Apply letter spacing to a shaped WrappedLine by offsetting glyph positions.
///
/// A glyph is pushed along by the spacing owed to everything before it, and a
/// full-width character owes two columns' worth. Charging every glyph the same
/// amount would make two lines of equal column width come out different
/// lengths, which is what breaks box drawing and any other aligned ASCII art.
pub(crate) fn apply_letter_spacing(wrapped: &mut WrappedLine, spacing_px: f32) {
    if spacing_px == 0.0 {
        return;
    }

    let text = wrapped.text.clone();
    let old_layout = &**wrapped;
    let wrap_width = old_layout.wrap_width;
    let old_line = &old_layout.unwrapped_layout;
    let mut new_runs: Vec<ShapedRun> = Vec::with_capacity(old_line.runs.len());
    let mut columns: usize = 0;
    let mut last_offset = px(0.0);

    for run in &old_line.runs {
        let mut new_glyphs: Vec<ShapedGlyph> = Vec::with_capacity(run.glyphs.len());
        for glyph in &run.glyphs {
            last_offset = px(columns as f32 * spacing_px);
            new_glyphs.push(ShapedGlyph {
                id: glyph.id,
                position: point(glyph.position.x + last_offset, glyph.position.y),
                index: glyph.index,
                is_emoji: glyph.is_emoji,
            });
            columns += text
                .get(glyph.index..)
                .and_then(|rest| rest.chars().next())
                .map(char_display_width)
                .unwrap_or(1);
        }
        new_runs.push(ShapedRun {
            font_id: run.font_id,
            glyphs: new_glyphs,
        });
    }

    let new_line_layout = Arc::new(LineLayout {
        font_size: old_line.font_size,
        width: old_line.width + last_offset,
        ascent: old_line.ascent,
        descent: old_line.descent,
        runs: new_runs,
        len: old_line.len,
    });

    let new_wrapped_layout = Arc::new(WrappedLineLayout {
        unwrapped_layout: new_line_layout,
        wrap_boundaries: old_layout.wrap_boundaries.clone(),
        wrap_width: old_layout.wrap_width,
        row_heights: Vec::new(),
    });

    let arc_mut: &mut Arc<WrappedLineLayout> = &mut *wrapped;
    *arc_mut = new_wrapped_layout;
    if let Some(wrap_width) = wrap_width {
        wrapped.rewrap(wrap_width);
    }
}
