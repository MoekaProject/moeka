use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, Font, FontFallbacks, FontWeight, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, IntoElement, LayoutId, MouseMoveEvent, PaintQuad, Path, Pixels, Point,
    ShapedLine, SharedString, Style, TextAlign, TextRun, TransformationMatrix, Window, WrappedLine,
    fill, point, px, quad, relative, size,
};

use crate::markdown::{BlockType, CalloutKind, TableLineInfo};

use super::helpers;
use super::*;

pub(super) const LINE_FORMAT_ICON_SIZE_PX: f32 = 18.0;
const LINE_FORMAT_ICON_GAP_PX: f32 = 0.0;
pub(super) const LINE_FORMAT_CONTENT_GAP_PX: f32 = 6.0;
const FRONT_MATTER_LIST_DELETE_GAP_PX: f32 = 4.0;
const FRONT_MATTER_LIST_DELETE_WIDTH_PX: f32 = 14.0;
const FRONT_MATTER_LIST_ITEM_GAP_PX: f32 = 3.0;

/// Shared by prepaint and the uncached hit-test/IME shaping path. Reserve the
/// column on continuation rows too; source indentation stays inside the text.
pub(super) fn front_matter_content_geometry(
    is_front_matter_line: bool,
    content_left: f32,
    wrap_width: f32,
    scale: f32,
) -> (f32, f32) {
    if !is_front_matter_line {
        return (content_left, wrap_width);
    }
    let inset = (LINE_FORMAT_ICON_SIZE_PX + LINE_FORMAT_CONTENT_GAP_PX) * scale;
    (content_left + inset, (wrap_width - inset).max(1.0))
}

fn front_matter_svg_left_inset(svg: &[u8]) -> Option<f32> {
    fn painted_left(group: &usvg::Group) -> Option<f32> {
        group
            .children()
            .iter()
            .filter_map(|node| match node {
                usvg::Node::Group(group) => painted_left(group),
                usvg::Node::Path(path) if path.fill().is_some() || path.stroke().is_some() => {
                    Some(path.abs_stroke_bounding_box().left())
                }
                _ => None,
            })
            .min_by(f32::total_cmp)
    }
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    Some(painted_left(tree.root())? / tree.size().width())
}

fn front_matter_type_icon_inset(path: &'static str, cx: &App) -> f32 {
    static INSETS: std::sync::OnceLock<std::sync::Mutex<HashMap<&'static str, f32>>> =
        std::sync::OnceLock::new();
    let mut insets = INSETS.get_or_init(Default::default).lock().unwrap();
    *insets.entry(path).or_insert_with(|| {
        cx.asset_source()
            .load(path)
            .ok()
            .flatten()
            .and_then(|svg| front_matter_svg_left_inset(&svg))
            .unwrap_or(0.0)
    })
}

fn front_matter_type_glyph_bounds(slot: Bounds<Pixels>, icon_size: Pixels) -> Bounds<Pixels> {
    Bounds::new(
        slot.origin + point(px(0.0), (slot.size.height - icon_size) * 0.5),
        size(icon_size, icon_size),
    )
}

pub(super) fn front_matter_fold_hit_bounds(
    left: f32,
    content_left: f32,
    top: f32,
    height: f32,
    viewport: Bounds<Pixels>,
) -> Bounds<Pixels> {
    Bounds::new(
        point(px(left), px(top)),
        size(px((content_left - left).max(0.0)), px(height)),
    )
    .intersect(&viewport)
}

pub(super) fn front_matter_type_hit_bounds(
    slot: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
) -> Option<Bounds<Pixels>> {
    let clipped = slot.intersect(&viewport);
    (clipped.size.width > px(0.0) && clipped.size.height > px(0.0)).then_some(clipped)
}

#[cfg(test)]
#[test]
fn front_matter_column_preserves_right_edge_and_source_indent_at_every_zoom() {
    for scale in [0.75, 1.0, 1.5, 2.0] {
        let (left, width) = front_matter_content_geometry(true, 100.0, 400.0, scale);
        assert_eq!(left + width, 500.0);
        assert_eq!(left, 100.0 + 24.0 * scale);
        // Nested keys keep their source indent after the fixed icon column.
        let source_indent = 12.0 * scale;
        assert_eq!((left + source_indent) - left, source_indent);
        assert_eq!(
            front_matter_content_geometry(false, 100.0, 400.0, scale),
            (100.0, 400.0),
        );
    }
    assert_eq!(
        front_matter_content_geometry(true, 0.0, 10.0, 1.0),
        (24.0, 1.0)
    );
}

#[cfg(test)]
#[test]
fn front_matter_delimiters_and_summary_use_left_edge_and_fold_hits_stay_in_gutter() {
    let decorations = crate::markdown::parse_decorations("---\ntitle: '中'\n---\nBody");
    for scale in [0.75, 1.0, 1.5, 2.0] {
        for line in [0, 2] {
            let table = decorations[line].table_info.as_ref().unwrap();
            assert_eq!(
                front_matter_content_geometry(
                    table.is_front_matter && !table.is_separator,
                    100.0,
                    400.0,
                    scale
                ),
                (100.0, 400.0)
            );
        }
        let slot = Bounds::new(
            point(px(100.0), px(50.0)),
            size(px(18.0 * scale), px(18.0 * scale)),
        );
        let glyph = front_matter_type_glyph_bounds(slot, px(14.0 * scale));
        assert_eq!(glyph.left(), slot.left());
        assert_eq!(glyph.center().y, slot.center().y);
        let viewport = Bounds::new(point(px(85.0), px(50.0)), size(px(400.0), px(200.0)));
        let hit = front_matter_fold_hit_bounds(80.0, 100.0, 45.0, 25.0, viewport);
        assert_eq!(hit.left(), viewport.left());
        assert_eq!(hit.top(), viewport.top());
        assert_eq!(hit.right(), px(100.0));
        assert!(hit.contains(&point(px(95.0), px(60.0))));
        assert!(!hit.contains(&point(px(101.0), px(60.0))));
    }
}

#[cfg(test)]
#[test]
fn front_matter_visible_svg_ink_starts_at_content_left_without_changing_hit_targets() {
    for svg in [
        include_bytes!("../../assets/icons/format/front-matter-text.svg").as_slice(),
        include_bytes!("../../assets/icons/format/front-matter-number.svg").as_slice(),
    ] {
        let inset = front_matter_svg_left_inset(svg).unwrap();
        assert!(inset > 0.0);
        for scale in [0.75, 1.0, 1.5, 2.0] {
            let slot = Bounds::new(
                point(px(100.0), px(50.0)),
                size(px(18.0 * scale), px(18.0 * scale)),
            );
            let icon_size = px(14.0 * scale);
            let mut glyph = front_matter_type_glyph_bounds(slot, icon_size);
            glyph.origin.x -= icon_size * inset;
            assert_eq!(glyph.left() + icon_size * inset, slot.left());
            assert_eq!(glyph.size.width, icon_size);
            assert_eq!(slot.size.width, px(18.0 * scale));
        }
    }
}

#[cfg(test)]
#[test]
fn front_matter_list_spacing_moves_delete_right_without_shrinking_its_target() {
    for scale in [0.75, 1.0, 1.5, 2.0] {
        let text_end = 100.0;
        let delete_left = text_end + FRONT_MATTER_LIST_DELETE_GAP_PX * scale;
        let delete_right = delete_left + FRONT_MATTER_LIST_DELETE_WIDTH_PX * scale;
        let next_item = delete_right + FRONT_MATTER_LIST_ITEM_GAP_PX * scale;
        assert_eq!(delete_left - text_end, 4.0 * scale);
        assert_eq!(delete_right - delete_left, 14.0 * scale);
        assert_eq!(next_item - delete_right, 3.0 * scale);
        assert!(next_item < text_end + (2.0 + 14.0 + 6.0) * scale);
    }
}

#[cfg(test)]
#[test]
fn front_matter_type_hits_are_clipped_to_the_editor_viewport() {
    let viewport = Bounds::new(point(px(100.0), px(50.0)), size(px(300.0), px(200.0)));
    let slot = Bounds::new(point(px(95.0), px(45.0)), size(px(18.0), px(18.0)));
    let hit = front_matter_type_hit_bounds(slot, viewport).unwrap();
    assert_eq!(hit.origin, viewport.origin);
    assert_eq!(hit.size, size(px(13.0), px(13.0)));
    assert!(!hit.contains(&point(px(99.0), px(55.0))));
    assert!(!hit.contains(&point(px(105.0), px(49.0))));
    for origin in [point(px(80.0), px(60.0)), point(px(110.0), px(250.0))] {
        assert!(front_matter_type_hit_bounds(Bounds::new(origin, slot.size), viewport).is_none());
    }
}

fn table_group_expands_neighboring_rows(table: &TableLineInfo, cursor_line: usize) -> bool {
    !table.is_front_matter && (table.table_start_line..table.table_end_line).contains(&cursor_line)
}

#[cfg(test)]
#[test]
fn front_matter_scalar_focus_does_not_expand_neighboring_rows() {
    let yaml = crate::markdown::parse_decorations("---\ntitle: 'Moeka'\nflag: true\n---");
    for decoration in &yaml {
        assert!(!table_group_expands_neighboring_rows(
            decoration.table_info.as_ref().unwrap(),
            1,
        ));
    }
    let table = crate::markdown::parse_decorations("| title |\n| --- |\n| Moeka |");
    assert!(table_group_expands_neighboring_rows(
        table[0].table_info.as_ref().unwrap(),
        2,
    ));
}

fn table_border_track_path(
    table_left: f32,
    table_top: f32,
    table_right: f32,
    table_bottom: f32,
    track_width: f32,
    corner_radius: f32,
) -> Option<Path<Pixels>> {
    let radius = corner_radius.max(0.0);
    let outer_left = table_left - track_width;
    let outer_top = table_top - track_width;
    let control_factor = 0.552_284_8;
    let mut path = gpui::PathBuilder::fill();
    path.move_to(point(px(table_left), px(table_bottom)));
    path.line_to(point(px(outer_left), px(table_bottom)));
    path.line_to(point(px(outer_left), px(outer_top + radius)));
    path.cubic_bezier_to(
        point(px(outer_left + radius), px(outer_top)),
        point(
            px(outer_left),
            px(outer_top + radius * (1.0 - control_factor)),
        ),
        point(
            px(outer_left + radius * (1.0 - control_factor)),
            px(outer_top),
        ),
    );
    path.line_to(point(px(table_right), px(outer_top)));
    path.line_to(point(px(table_right), px(table_top)));
    path.line_to(point(px(table_left), px(table_top)));
    path.close();
    path.build().ok()
}

fn generated_preview_heights_changed(
    decorations: &[crate::markdown::LineDecoration],
    updates: &[(usize, Option<LineDisplayInfo>)],
    previous_height: impl Fn(usize) -> f32,
) -> bool {
    updates.iter().any(|(line, info)| {
        decorations.get(*line).is_some_and(|decoration| {
            decoration.block_preview.is_some() || !decoration.inline_math_spans.is_empty()
        }) && info
            .as_ref()
            .is_some_and(|info| (info.total_height() - previous_height(*line)).abs() > 0.01)
    })
}

fn preview_height_anchor_delta(
    previous_height: f32,
    measured_height: f32,
    previous_bottom: f64,
    viewport_top: f64,
) -> f32 {
    if previous_bottom <= viewport_top {
        measured_height - previous_height
    } else {
        0.0
    }
}

#[cfg(test)]
#[test]
fn preview_height_changes_above_viewport_keep_the_next_line_fixed() {
    for measured_height in [20.0, 300.0] {
        let delta = preview_height_anchor_delta(100.0, measured_height, -12.0, 0.0);
        assert_eq!(-12.0 + measured_height - 100.0 - delta, -12.0);
    }
    assert_eq!(preview_height_anchor_delta(100.0, 300.0, 12.0, 0.0), 0.0);
    // An uncached line still has an estimated height in the geometry index.
    assert_eq!(preview_height_anchor_delta(24.0, 300.0, -12.0, 0.0), 276.0);
}

#[cfg(test)]
#[test]
fn preview_height_change_at_viewport_boundary_anchors_the_following_line() {
    assert_eq!(
        preview_height_anchor_delta(100.0, 300.0, 500.0, 500.0),
        200.0
    );
}

#[cfg(test)]
#[test]
fn preview_reflow_classifies_each_change_against_previous_document_geometry() {
    fn measure(previous: &[f32], measured: &[f32], scroll: f64) -> (f32, f32) {
        let mut previous_bottom = 0.0;
        let mut painted_bottom = -(scroll as f32);
        let mut compensation = 0.0;
        for (&old, &new) in previous.iter().zip(measured) {
            previous_bottom += old as f64;
            compensation += preview_height_anchor_delta(old, new, previous_bottom, scroll);
            painted_bottom += new;
        }
        (compensation, painted_bottom - compensation)
    }
    // A tall VISIBLE preview shrinks. The next preview was below the viewport,
    // even though its new top is above it. Running painted-y classification
    // incorrectly compensates its growth and fights the viewport's own reflow.
    assert_eq!(measure(&[400.0, 20.0], &[20.0, 40.0], 250.0).0, 0.0);
    // Several changes above the viewport, including collapsed source rows,
    // compensate exactly once, preserving the next line's screen position.
    let previous = [100.0, 0.0, 200.0, 0.0];
    let measured = [200.0, 0.0, 50.0, 0.0];
    assert_eq!(measure(&previous, &measured, 350.0), (-50.0, -50.0));
    // Once committed, identical measurements cannot accumulate more movement.
    assert_eq!(measure(&measured, &measured, 300.0), (0.0, -50.0));
}

fn line_format_trigger_left(
    content_left: f32,
    format_size: f32,
    fold_size: Option<f32>,
    icon_gap: f32,
    content_gap: f32,
) -> f32 {
    content_left - content_gap - format_size - fold_size.map_or(0.0, |size| size + icon_gap)
}

fn markdown_fold_opacity(
    fold_hovered: bool,
    row_hovered: bool,
    row_opacity: f32,
    persistent_opacity: f32,
) -> f32 {
    if fold_hovered {
        selection_toolbar::FORMAT_TRIGGER_HOVER_OPACITY
    } else if row_hovered {
        row_opacity
    } else {
        persistent_opacity
    }
}

#[cfg(test)]
#[test]
fn front_matter_hover_controls_share_heading_order_on_the_moved_property_row() {
    let decorations = crate::markdown::parse_decorations("---\ntitle: Moeka\n---\nBody");
    let control_line = super::fold::front_matter_fold_control_line(&decorations, &[], false);
    assert_eq!(control_line, Some(1));
    for scale in [0.75, 1.0, 1.5, 2.0] {
        let content_left = 100.0;
        let format_size = LINE_FORMAT_ICON_SIZE_PX * scale;
        let fold_size = 12.0 * scale;
        let content_gap = LINE_FORMAT_CONTENT_GAP_PX * scale;
        let icon_gap = LINE_FORMAT_ICON_GAP_PX * scale;
        let format_left = line_format_trigger_left(
            content_left,
            format_size,
            Some(fold_size),
            icon_gap,
            content_gap,
        );
        let fold_left = content_left - content_gap - fold_size;
        assert!(format_left < fold_left);
        assert_eq!(format_left + format_size + icon_gap, fold_left);
        assert!(fold_left + fold_size <= content_left);
        assert_eq!(
            line_format_trigger_left(content_left, format_size, None, icon_gap, content_gap),
            content_left - format_size - content_gap
        );
    }
}

#[cfg(test)]
#[test]
fn front_matter_fold_buttons_are_hidden_until_row_or_button_hover() {
    assert_eq!(markdown_fold_opacity(false, false, 0.34, 0.0), 0.0);
    assert_eq!(markdown_fold_opacity(false, true, 0.34, 0.0), 0.34);
    assert_eq!(
        markdown_fold_opacity(true, false, 0.34, 0.0),
        selection_toolbar::FORMAT_TRIGGER_HOVER_OPACITY
    );
    // Other folded Markdown blocks retain their existing persistent affordance.
    assert_eq!(markdown_fold_opacity(false, false, 0.34, 0.5), 0.5);
}

fn gutter_fold_row_hovered(gutter_hovered: bool, pointer_y: Pixels, top: f32, height: f32) -> bool {
    gutter_hovered && (top..top + height).contains(&f32::from(pointer_y))
}

#[cfg(test)]
#[test]
fn gutter_hover_reveals_only_the_current_row() {
    for (top, height, expected) in [
        (100.0, 20.0, false),
        (120.0, 80.0, true),
        (200.0, 20.0, false),
    ] {
        assert_eq!(
            gutter_fold_row_hovered(true, px(150.0), top, height),
            expected
        );
    }
    assert!(!gutter_fold_row_hovered(false, px(150.0), 120.0, 80.0));
    assert!(!gutter_fold_row_hovered(true, px(200.0), 120.0, 80.0));
    assert!(gutter_fold_row_hovered(true, px(200.0), 200.0, 20.0));
}

fn callout_icon_path(kind: CalloutKind) -> &'static str {
    match kind {
        CalloutKind::Note => FMT_CALLOUT_NOTE_SVG,
        CalloutKind::Tip => FMT_CALLOUT_TIP_SVG,
        CalloutKind::Important => FMT_CALLOUT_IMPORTANT_SVG,
        CalloutKind::Warning => FMT_CALLOUT_WARNING_SVG,
        CalloutKind::Caution => FMT_CALLOUT_CAUTION_SVG,
    }
}

fn callout_icon_placeholder_len(display_text: &str) -> usize {
    display_text
        .char_indices()
        .find_map(|(index, character)| (!character.is_whitespace()).then_some(index))
        .unwrap_or(display_text.len())
}

fn callout_icon_size(
    available_width: f32,
    font_size: f32,
    line_height: f32,
    size_factor: f32,
    min_size: f32,
    max_line_height_factor: f32,
) -> f32 {
    let desired_size = (font_size * size_factor).max(min_size);
    let line_height_limit = (line_height * max_line_height_factor).max(min_size);
    desired_size
        .min(line_height_limit)
        .min(available_width.max(0.0))
}

fn generated_preview_action_is_visible(action: GeneratedPreviewAction, read_only: bool) -> bool {
    action != GeneratedPreviewAction::Code || !read_only
}

fn generated_preview_code_mode_active(
    preview: &crate::markdown::BlockPreview,
    cursor_line: usize,
    cursor_offset: usize,
    selection_ranges: &[Range<usize>],
    syntax_expansion_allowed: bool,
    search_reveal_range: Option<&Range<usize>>,
) -> bool {
    search_reveal_range.is_some_and(|range| {
        range.start < preview.source_range.end && preview.source_range.start < range.end
    }) || (syntax_expansion_allowed
        && ((preview.start_line..=preview.end_line).contains(&cursor_line)
            && preview.source_range.start <= cursor_offset
            && cursor_offset <= preview.source_range.end
            || selection_ranges.iter().any(|range| {
                range.start < preview.source_range.end && range.end > preview.source_range.start
            })))
}

fn uses_table_cell_selection(
    block_type: &BlockType,
    decoration: &crate::markdown::LineDecoration,
    active_tab_is_diff: bool,
) -> bool {
    matches!(block_type, BlockType::Table)
        && !active_tab_is_diff
        && decoration
            .table_info
            .as_ref()
            .is_some_and(|table_info| !table_info.is_front_matter)
}

#[cfg(test)]
#[test]
fn callout_icon_placeholder_excludes_default_title() {
    let placeholder = "\u{2003}\u{2003}\u{2003}\u{2003}";
    assert_eq!(callout_icon_placeholder_len(placeholder), placeholder.len());
    assert_eq!(
        callout_icon_placeholder_len(&format!("{placeholder}WARNING")),
        placeholder.len()
    );
}

#[cfg(test)]
#[test]
fn callout_icon_minimum_size_overrides_line_height_limit() {
    assert_eq!(
        callout_icon_size(60.0, 15.0, 22.5, 2.175, 28.0, 1.5),
        32.625
    );
}

#[cfg(test)]
#[test]
fn read_only_generated_previews_hide_code_action() {
    assert!(!generated_preview_action_is_visible(
        GeneratedPreviewAction::Code,
        true,
    ));
    assert!(generated_preview_action_is_visible(
        GeneratedPreviewAction::Copy,
        true,
    ));
    assert!(generated_preview_action_is_visible(
        GeneratedPreviewAction::Code,
        false,
    ));
}

#[cfg(test)]
#[test]
fn reading_mode_selection_does_not_activate_preview_code_mode() {
    let decorations = crate::markdown::parse_decorations("```mermaid\ngraph TD\nA-->B\n```\n");
    let preview = decorations
        .iter()
        .find_map(|decoration| decoration.block_preview.as_ref())
        .expect("generated preview");
    let selection = [preview.source_range.clone()];

    assert!(generated_preview_code_mode_active(
        preview,
        preview.start_line,
        preview.source_range.start,
        &selection,
        true,
        None,
    ));
    assert!(!generated_preview_code_mode_active(
        preview,
        preview.start_line,
        preview.source_range.start,
        &selection,
        false,
        None,
    ));
}

#[cfg(test)]
#[test]
fn search_reveals_preview_source_without_enabling_read_only_selection_expansion() {
    for source in ["```mermaid\ngraph TD\nA-->B\n```\n", "$$\nx + needle\n$$\n"] {
        let decorations = crate::markdown::parse_decorations(source);
        let preview = decorations
            .iter()
            .find_map(|d| d.block_preview.as_ref())
            .unwrap();
        for range in [
            preview.source_range.start..preview.source_range.start + 1,
            preview.source_range.end - 1..preview.source_range.end,
        ] {
            assert!(generated_preview_code_mode_active(
                preview,
                usize::MAX,
                0,
                &[],
                false,
                Some(&range)
            ));
        }
        let adjacent = preview.source_range.end..preview.source_range.end + 1;
        assert!(!generated_preview_code_mode_active(
            preview,
            usize::MAX,
            0,
            &[],
            false,
            Some(&adjacent)
        ));
        assert!(!generated_preview_code_mode_active(
            preview,
            usize::MAX,
            0,
            &[],
            false,
            None
        ));
    }
}

#[cfg(test)]
#[test]
fn markdown_tables_use_only_cell_selection_rendering() {
    let decorations = crate::markdown::parse_decorations("| A | B |\n| --- | --- |\n| 1 | 2 |");
    assert!(uses_table_cell_selection(
        &BlockType::Table,
        &decorations[0],
        false,
    ));
    assert!(!uses_table_cell_selection(
        &BlockType::Table,
        &decorations[0],
        true,
    ));

    let front_matter = crate::markdown::parse_decorations("---\ntitle: Moeka\n---");
    assert!(!uses_table_cell_selection(
        &BlockType::Table,
        &front_matter[1],
        false,
    ));

    let decorations = crate::markdown::parse_decorations(
        "| 功能 | 快捷键 |\n| --- | --- |\n| 加粗 | `Command / Ctrl + B` |",
    );
    assert!(uses_table_cell_selection(
        &BlockType::Table,
        &decorations[2],
        false
    ));
    let row = decorations[2].table_info.as_ref().unwrap();
    let cell = helpers::table_cell_decoration(&decorations[2], &row.cells[1], &row.cell_ranges[1]);
    assert!(
        cell.inline_spans
            .iter()
            .any(|span| span.kind == crate::markdown::InlineKind::InlineCode)
    );
}

fn snap_to_device_pixel(value: f32, window: &Window) -> f32 {
    let scale_factor = window.scale_factor().max(1.0);
    (value * scale_factor).round() / scale_factor
}

fn snap_pixel_width(value: f32, window: &Window) -> f32 {
    if value <= 0.0 {
        0.0
    } else {
        snap_to_device_pixel(value, window).max(1.0)
    }
}

fn snap_bounds_to_device_pixels(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    window: &Window,
) -> Bounds<Pixels> {
    let right = snap_to_device_pixel(left + width, window);
    let bottom = snap_to_device_pixel(top + height, window);
    let left = snap_to_device_pixel(left, window);
    let top = snap_to_device_pixel(top, window);
    Bounds::new(
        point(px(left), px(top)),
        size(px((right - left).max(0.0)), px((bottom - top).max(0.0))),
    )
}

fn paint_quads_with_optional_mask(
    quads: &mut Vec<PaintQuad>,
    mask: Option<Bounds<Pixels>>,
    window: &mut Window,
) {
    if let Some(bounds) = mask {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in quads.drain(..) {
                window.paint_quad(quad);
            }
        });
    } else {
        for quad in quads.drain(..) {
            window.paint_quad(quad);
        }
    }
}

pub(crate) struct EditorElement {
    pub editor: Entity<EditorView>,
    /// Which pane's document this element renders; its state is swapped into
    /// the root fields for the duration of prepaint/paint.
    pub pane_id: super::PaneId,
    pub caret_geometry: super::caret::SharedCaretGeometry,
}

pub(crate) struct EditorPrepaintState {
    hitbox: Hitbox,
    link_hitboxes: Vec<Hitbox>,
    horizontal_editor_clip_bounds: Option<Bounds<Pixels>>,
    lines: Vec<PrepaintLine>,
    indent_guide_quads: Vec<PaintQuad>,
    clipped_code_block_decorations: Vec<(Bounds<Pixels>, Vec<PaintQuad>)>,
    clipped_table_decorations: Vec<(Bounds<Pixels>, Vec<PaintQuad>)>,
    clipped_code_block_search_highlights: Vec<(Bounds<Pixels>, Vec<PaintQuad>)>,
    clipped_code_block_selections: Vec<(Bounds<Pixels>, Vec<PaintQuad>)>,
    clipped_table_search_highlights: Vec<(Bounds<Pixels>, Vec<PaintQuad>)>,
    clipped_table_selections: Vec<(Bounds<Pixels>, Vec<PaintQuad>)>,
    whitespace_dots: Vec<PaintQuad>,
    selection_quads: Vec<PaintQuad>,
    search_match_quads: Vec<PaintQuad>,
    diff_line_bgs: Vec<PaintQuad>,
    diff_inline_quads: Vec<PaintQuad>,
    inline_diff_gutter_bars: Vec<(Bounds<Pixels>, Hsla)>,
    phantom_text_lines: Vec<(WrappedLine, Point<Pixels>, Pixels)>,
    line_highlight: Option<PaintQuad>,
    quote_block_bgs: Vec<PaintQuad>,
    code_block_bgs: Vec<PaintQuad>,
    unordered_list_markers: Vec<(WrappedLine, Point<Pixels>, Pixels)>,
    marked_underlines: Vec<PaintQuad>,
    gutter_quads: Vec<PaintQuad>,
    gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    inline_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    gutter_fold_icons: Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    format_icons: Vec<(Bounds<Pixels>, Hsla)>,
    front_matter_type_icons: Vec<(Bounds<Pixels>, &'static str, Hsla)>,

    front_matter_group_texts: Vec<(Bounds<Pixels>, WrappedLine, Point<Pixels>, Pixels)>,
    front_matter_delete_hitboxes: Vec<Hitbox>,
    /// HR lines: (y_position, start_x, width, is_dashed). `***` renders dashed,
    /// `---`/`___` render solid.
    hr_lines: Vec<(f32, f32, f32, bool)>,
    /// Image previews to paint: (bounds, image_data)
    image_previews: Vec<(
        Bounds<Pixels>,
        Arc<RenderImage>,
        Pixels,
        Option<Bounds<Pixels>>,
    )>,
    image_borders: Vec<PaintQuad>,
    /// Table cell borders and backgrounds
    table_borders: Vec<PaintQuad>,
    table_border_paths: Vec<(Path<Pixels>, Hsla)>,
    table_bgs: Vec<PaintQuad>,
    table_icons: Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    /// Image URL-edit icons: (bounds, line_idx, alpha)
    image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)>,
    /// Blue wavy underlines for links when cmd is held
    link_underlines: Vec<PaintQuad>,
    inline_overlays: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    /// Per-cell shaped texts for table block rendering.
    table_cell_texts: Vec<(WrappedLine, Point<Pixels>, Pixels)>,
    code_block_copy_buttons: Vec<CodeBlockCopyButtonPaint>,
    inline_diff_revert_buttons: Vec<CodeBlockCopyButtonPaint>,
    code_block_copy_hitboxes: Vec<Hitbox>,
    format_trigger_hitboxes: Vec<(Hitbox, usize)>,
    front_matter_type_hitboxes: Vec<(Hitbox, usize)>,
    preview_zoom_buttons: Vec<CodeBlockCopyButtonPaint>,
    preview_zoom_hitboxes: Vec<Hitbox>,
    tooltips: Vec<(Hitbox, Bounds<Pixels>, SharedString)>,
}

struct CodeBlockGroupBounds {
    left: f32,
    top: f32,
    width: f32,
    bottom: f32,
}

fn code_block_horizontal_thumb_geometry(
    track_left: f32,
    track_width: f32,
    max_scroll: f32,
    scroll_offset: f32,
    min_thumb_width: f32,
) -> (f32, f32, f32) {
    let track_width = track_width.max(1.0);
    let max_scroll = max_scroll.max(0.0);
    let content_width = track_width + max_scroll;
    let thumb_width = (track_width * track_width / content_width)
        .clamp(min_thumb_width.min(track_width), track_width);
    let travel = (track_width - thumb_width).max(0.0);
    let progress = if max_scroll <= 0.0 {
        0.0
    } else {
        (scroll_offset / max_scroll).clamp(0.0, 1.0)
    };
    (track_left + travel * progress, thumb_width, travel)
}

fn code_block_text_viewport_geometry(
    background_left: f32,
    background_width: f32,
    left_inset: f32,
    right_padding: f32,
) -> (f32, f32, f32) {
    let left = background_left + left_inset;
    let right = (background_left + background_width - right_padding).max(left + 1.0);
    (left, (right - left).max(1.0), right)
}

fn code_block_text_clip_right(background_right: f32, content_right: f32, word_wrap: bool) -> f32 {
    if word_wrap {
        background_right
    } else {
        content_right
    }
}

#[cfg(test)]
#[test]
fn code_block_text_viewport_keeps_right_padding_with_or_without_line_numbers() {
    let (_, width_without_numbers, right_without_numbers) =
        code_block_text_viewport_geometry(100.0, 500.0, 20.0, 20.0);
    let (_, width_with_numbers, right_with_numbers) =
        code_block_text_viewport_geometry(100.0, 500.0, 52.0, 20.0);

    assert_eq!(right_without_numbers, 580.0);
    assert_eq!(right_with_numbers, 580.0);
    assert_eq!(width_without_numbers, 460.0);
    assert_eq!(width_with_numbers, 428.0);
    assert_eq!(
        helpers::code_block_soft_wrap_width(width_without_numbers, 8.0),
        452.0
    );
    assert_eq!(
        helpers::code_block_soft_wrap_width(width_with_numbers, 8.0),
        420.0
    );
    assert_eq!(code_block_text_clip_right(600.0, 580.0, true), 600.0);
    assert_eq!(code_block_text_clip_right(600.0, 580.0, false), 580.0);
}

fn non_markdown_horizontal_max_offset(
    unwrapped_width: f32,
    viewport_width: f32,
    right_padding: f32,
    caret_width: f32,
) -> f32 {
    (unwrapped_width + right_padding + caret_width - viewport_width).max(0.0)
}

#[cfg(test)]
#[test]
fn code_block_horizontal_thumb_aligns_with_background_edges() {
    let (left, width, travel) = code_block_horizontal_thumb_geometry(20.0, 200.0, 300.0, 0.0, 36.0);
    assert_eq!(left, 20.0);
    assert_eq!(width, 80.0);
    assert_eq!(travel, 120.0);

    let (left, width, _) = code_block_horizontal_thumb_geometry(20.0, 200.0, 300.0, 300.0, 36.0);
    assert_eq!(left + width, 220.0);

    let margin = 3.0;
    let background_left = 20.0;
    let background_width = 200.0;
    let track_left = background_left + margin;
    let track_width = background_width - margin * 2.0;
    let (left, _, _) =
        code_block_horizontal_thumb_geometry(track_left, track_width, 300.0, 0.0, 36.0);
    assert_eq!(left, background_left + margin);
    let (left, width, _) =
        code_block_horizontal_thumb_geometry(track_left, track_width, 300.0, 300.0, 36.0);
    assert_eq!(left + width, background_left + background_width - margin);
}

#[cfg(test)]
#[test]
fn non_markdown_horizontal_extent_keeps_right_padding_and_caret_visible() {
    assert_eq!(
        non_markdown_horizontal_max_offset(500.0, 320.0, 20.0, 2.0),
        202.0
    );
    assert_eq!(
        non_markdown_horizontal_max_offset(200.0, 320.0, 20.0, 2.0),
        0.0
    );
    // Source retains Markdown quote/list insets within the global text clip.
    let content_inset = 48.0;
    let width = 500.0;
    let max_offset = non_markdown_horizontal_max_offset(content_inset + width, 320.0, 20.0, 2.0);
    assert_eq!(max_offset, 250.0);
    assert_eq!(content_inset + width - max_offset + 20.0 + 2.0, 320.0);
}

struct CodeBlockCopyButtonPaint {
    quad: PaintQuad,
    label: Option<WrappedLine>,
    origin: Point<Pixels>,
    line_height: Pixels,
    icon: Option<(Bounds<Pixels>, &'static str, Hsla)>,
    trailing_labels: Vec<(WrappedLine, Point<Pixels>)>,
}

fn shape_preview_button(
    window: &Window,
    button_font: &Font,
    button_font_size: f32,
    button_padding_left_x: f32,
    button_padding_right_x: f32,
    button_padding_y: f32,
    button_text_line_height: f32,
    label_text: &str,
    text_color: Hsla,
) -> Option<(WrappedLine, f32, f32)> {
    let label_run = TextRun {
        len: label_text.len(),
        font: button_font.clone(),
        color: text_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let label_lines = window
        .text_system()
        .shape_text(
            label_text.to_string().into(),
            px(button_font_size),
            &[label_run],
            None,
            None,
        )
        .ok()?;
    let label = label_lines.into_iter().next()?;
    let button_width = f32::from(label.width()) + button_padding_left_x + button_padding_right_x;
    let button_height = button_text_line_height + button_padding_y * 2.0;
    Some((label, button_width, button_height))
}

fn paint_code_block_button(button: &CodeBlockCopyButtonPaint, window: &mut Window, cx: &mut App) {
    window.paint_quad(button.quad.clone());
    if let Some(label) = &button.label {
        label
            .paint(
                button.origin,
                button.line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
    }
    for (label, origin) in &button.trailing_labels {
        label
            .paint(
                *origin,
                button.line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
    }
    if let Some((icon_bounds, icon_path, color)) = &button.icon {
        window
            .paint_svg(
                *icon_bounds,
                (*icon_path).into(),
                TransformationMatrix::default(),
                *color,
                cx,
            )
            .ok();
    }
}

#[derive(Clone)]
struct PrepaintLine {
    wrapped: WrappedLine,
    /// Baseline-adjusted paint origin, never used for caret/IME/hit geometry.
    origin: Point<Pixels>,
    line_height: Pixels,
    clip_bounds: Option<Bounds<Pixels>>,
    /// Per-visual-line shaped segments for indented painting.
    segments: Option<Vec<(WrappedLine, Point<Pixels>)>>,
    /// True if this line is a table row rendered via per-cell block mode.
    is_table_block: bool,
}

pub(crate) struct LineDragGhost {
    pub(crate) bounds: Bounds<Pixels>,
    lines: Vec<PrepaintLine>,
    wrapped: Vec<(WrappedLine, Point<Pixels>, Pixels)>,
    shaped: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    backgrounds: Vec<PaintQuad>,
    foregrounds: Vec<PaintQuad>,
    svgs: Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    images: Vec<(
        Bounds<Pixels>,
        Arc<RenderImage>,
        Pixels,
        Option<Bounds<Pixels>>,
    )>,
    rules: Vec<(f32, f32, f32, bool)>,
    rule_color: Hsla,
}

impl LineDragGhost {
    fn capture(state: &EditorPrepaintState, bounds: Bounds<Pixels>, rule_color: Hsla) -> Self {
        let intersects =
            |other: Bounds<Pixels>| other.bottom() > bounds.top() && other.top() < bounds.bottom();
        let contains_y =
            |origin: Point<Pixels>| origin.y >= bounds.top() && origin.y < bounds.bottom();
        Self {
            bounds,
            lines: state
                .lines
                .iter()
                .filter(|line| !line.is_table_block && contains_y(line.origin))
                .cloned()
                .collect(),
            wrapped: state
                .table_cell_texts
                .iter()
                .chain(&state.unordered_list_markers)
                .filter(|(_, origin, _)| contains_y(*origin))
                .cloned()
                .collect(),
            shaped: state
                .gutter_labels
                .iter()
                .chain(&state.inline_overlays)
                .filter(|(_, origin, _)| contains_y(*origin) && origin.x >= bounds.left())
                .cloned()
                .collect(),
            backgrounds: state
                .quote_block_bgs
                .iter()
                .chain(&state.code_block_bgs)
                .chain(&state.table_bgs)
                .chain(&state.gutter_quads)
                .chain(&state.indent_guide_quads)
                .filter(|quad| intersects(quad.bounds))
                .cloned()
                .collect(),
            foregrounds: state
                .table_borders
                .iter()
                .chain(&state.image_borders)
                .chain(&state.link_underlines)
                .filter(|quad| intersects(quad.bounds))
                .cloned()
                .collect(),
            svgs: state
                .gutter_svgs
                .iter()
                .chain(&state.inline_svgs)
                .chain(&state.front_matter_type_icons)
                .filter(|(rect, _, _)| intersects(*rect))
                .cloned()
                .collect(),
            images: state
                .image_previews
                .iter()
                .filter(|(rect, _, _, _)| intersects(*rect))
                .cloned()
                .collect(),
            rules: state
                .hr_lines
                .iter()
                .filter(|(y, _, _, _)| px(*y) >= bounds.top() && px(*y) < bounds.bottom())
                .cloned()
                .collect(),
            rule_color,
        }
    }

    pub(crate) fn paint(&self, origin: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let offset = origin - self.bounds.origin;
        let translate = |mut bounds: Bounds<Pixels>| {
            bounds.origin += offset;
            bounds
        };
        window.with_content_mask(
            Some(ContentMask {
                bounds: Bounds::new(origin, self.bounds.size),
            }),
            |window| {
                for quad in &self.backgrounds {
                    let mut quad = quad.clone();
                    quad.bounds = translate(quad.bounds);
                    window.paint_quad(quad);
                }
                for (rect, path, color) in &self.svgs {
                    window
                        .paint_svg(
                            translate(*rect),
                            (*path).into(),
                            TransformationMatrix::default(),
                            *color,
                            cx,
                        )
                        .ok();
                }
                for (shaped, position, height) in &self.shaped {
                    shaped.paint(*position + offset, *height, window, cx).ok();
                }
                for (wrapped, position, height) in &self.wrapped {
                    wrapped
                        .paint(
                            *position + offset,
                            *height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        )
                        .ok();
                }
                for line in &self.lines {
                    let mask = line.clip_bounds.map(|bounds| ContentMask {
                        bounds: translate(bounds),
                    });
                    window.with_content_mask(mask, |window| {
                        if let Some(segments) = &line.segments {
                            for (wrapped, position) in segments {
                                wrapped
                                    .paint(
                                        *position + offset,
                                        line.line_height,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    )
                                    .ok();
                            }
                        } else {
                            line.wrapped
                                .paint(
                                    line.origin + offset,
                                    line.line_height,
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                )
                                .ok();
                        }
                    });
                }
                for (rect, image, radius, clip) in &self.images {
                    window.with_content_mask(
                        clip.map(|bounds| ContentMask {
                            bounds: translate(bounds),
                        }),
                        |window| {
                            window
                                .paint_image(
                                    translate(*rect),
                                    Corners::all(*radius),
                                    image.clone(),
                                    0,
                                    false,
                                )
                                .ok();
                        },
                    );
                }
                for quad in &self.foregrounds {
                    let mut quad = quad.clone();
                    quad.bounds = translate(quad.bounds);
                    window.paint_quad(quad);
                }
                for (y, left, width, dashed) in &self.rules {
                    let mut x = *left;
                    while x < left + width {
                        let length = if *dashed {
                            6.0_f32.min(left + width - x)
                        } else {
                            *width
                        };
                        window.paint_quad(fill(
                            Bounds::new(point(px(x), px(*y)) + offset, size(px(length), px(1.0))),
                            self.rule_color,
                        ));
                        x += if *dashed { 10.0 } else { *width };
                    }
                }
            },
        );
    }
}

/// Text viewport clipping is horizontal within the editor's vertical viewport.
/// A row-height mask would cut off tall fallback glyphs after baseline alignment.
/// Selection/background masks remain tied to their original logical row boxes.
fn horizontal_text_clip_bounds(
    mut clip_bounds: Bounds<Pixels>,
    viewport_bounds: Bounds<Pixels>,
) -> Bounds<Pixels> {
    clip_bounds.origin.y = viewport_bounds.origin.y;
    clip_bounds.size.height = viewport_bounds.size.height;
    clip_bounds
}

#[cfg(test)]
#[test]
fn text_clip_preserves_horizontal_scroll_limits_without_clipping_to_a_row() {
    let row_clip = Bounds::new(point(px(40.0), px(100.0)), size(px(200.0), px(20.0)));
    let viewport = Bounds::new(point(px(10.0), px(30.0)), size(px(400.0), px(300.0)));
    assert_eq!(
        horizontal_text_clip_bounds(row_clip, viewport),
        Bounds::new(point(px(40.0), px(30.0)), size(px(200.0), px(300.0))),
    );
}

fn diff_line_background(editor: &EditorView, line_idx: usize) -> Option<Hsla> {
    if let Some(diff) = editor.active_git_diff_state() {
        let row = diff.rows.get(line_idx)?;
        return match row.kind {
            GitDiffRowKind::Added => Some(editor.styles.selection.selection_diff_added_line_bg),
            GitDiffRowKind::Removed => Some(editor.styles.selection.selection_diff_removed_line_bg),
            GitDiffRowKind::Context => None,
        };
    }
    // In-place working-tree diff: added/modified buffer lines get the green
    // changed-line background. Removed lines are not in the buffer, so they are
    // surfaced separately as phantom rows when a hunk is expanded.
    if editor.inline_diff_line_is_changed(line_idx) {
        return Some(editor.styles.selection.selection_diff_added_line_bg);
    }
    None
}

fn diff_hunk_label_text_color(editor: &EditorView, line_idx: usize) -> Option<Hsla> {
    let row = editor.active_git_diff_state()?.rows.get(line_idx)?;
    row.is_hunk_label.then_some(
        editor
            .styles
            .editor
            .editor_diff_collapsed_line_label_text_color,
    )
}

fn diff_table_syntax_color_ranges(
    line_content: &str,
    table_info: &crate::markdown::TableLineInfo,
) -> Vec<Range<usize>> {
    let line_len = line_content.len();
    if table_info.is_separator {
        return (line_len > 0).then_some(0..line_len).into_iter().collect();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    for cell_range in &table_info.cell_ranges {
        let start = cell_range.start.min(line_len);
        let end = cell_range.end.min(line_len).max(start);
        if cursor < start {
            ranges.push(cursor..start);
        }
        cursor = end;
    }
    if cursor < line_len {
        ranges.push(cursor..line_len);
    }
    ranges
}

fn apply_display_text_color_ranges(
    runs: &mut Vec<TextRun>,
    display_text: &str,
    ranges: &[Range<usize>],
    color: Hsla,
) {
    if runs.is_empty() || display_text.is_empty() || ranges.is_empty() {
        return;
    }

    let mut byte_colors: Vec<Option<Hsla>> = vec![None; display_text.len()];
    for range in ranges {
        let start = range.start.min(display_text.len());
        let end = range.end.min(display_text.len()).max(start);
        for entry in byte_colors.iter_mut().take(end).skip(start) {
            *entry = Some(color);
        }
    }

    let original_runs = std::mem::take(runs);
    let mut styled_runs = Vec::with_capacity(original_runs.len());
    let mut run_start = 0usize;
    for run in original_runs {
        let run_end = run_start + run.len;
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

fn inset_bounds_vertically(bounds: Bounds<Pixels>, inset: Pixels) -> Bounds<Pixels> {
    let inset = f32::from(inset).max(0.0);
    if inset <= 0.0 {
        return bounds;
    }

    let height = f32::from(bounds.size.height);
    let inset = inset.min(height / 2.0);
    Bounds::new(
        point(bounds.origin.x, px(f32::from(bounds.origin.y) + inset)),
        size(bounds.size.width, px((height - inset * 2.0).max(0.0))),
    )
}

fn pad_bounds_horizontally(bounds: Bounds<Pixels>, padding: Pixels) -> Bounds<Pixels> {
    let padding = f32::from(padding).max(0.0);
    if padding <= 0.0 {
        return bounds;
    }

    Bounds::new(
        point(px(f32::from(bounds.origin.x) - padding), bounds.origin.y),
        size(
            px(f32::from(bounds.size.width) + padding * 2.0),
            bounds.size.height,
        ),
    )
}

fn cursor_aligned_vertical_inset(editor: &EditorView, line_height: f32) -> Pixels {
    let cursor_height = line_height * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
    px(((line_height - cursor_height) / 2.0).max(0.0))
}

fn push_cursor_aligned_highlight_quads(
    quads: &mut Vec<PaintQuad>,
    editor: &EditorView,
    display_text: &str,
    runs: &[TextRun],
    wrapped: &WrappedLine,
    disp_start: usize,
    disp_end: usize,
    content_x: f32,
    text_line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    leading_padding_px: f32,
    color: Hsla,
    letter_spacing: f32,
    window: &mut Window,
) {
    for bounds in helpers::build_highlight_bounds(
        display_text,
        runs,
        wrapped,
        disp_start,
        disp_end,
        content_x,
        text_line_top,
        line_height,
        font_size,
        indent_px,
        leading_padding_px,
        letter_spacing,
        window,
    ) {
        let vertical_inset = cursor_aligned_vertical_inset(editor, f32::from(bounds.size.height));
        quads.push(fill(inset_bounds_vertically(bounds, vertical_inset), color));
    }
}

fn raw_offset_to_display_offset(buffer_to_display: &[usize], raw_offset: usize) -> usize {
    buffer_to_display
        .get(raw_offset)
        .copied()
        .or_else(|| buffer_to_display.last().copied())
        .unwrap_or(0)
}

fn push_task_checkbox_paint(
    editor: &EditorView,
    block_type: &BlockType,
    prefix_collapsed: bool,
    display_text: &str,
    wrapped: &WrappedLine,
    font_size: f32,
    line_height: f32,
    content_x: f32,
    text_line_top: f32,
    line_top: f32,
    total_line_height: f32,
    collapsed_task_prefix_display_columns: usize,
    mapped_prefix_bytes: Option<usize>,
    checkbox_text_gap_px: f32,
    hit_line_idx: Option<usize>,
    gutter_quads: &mut Vec<PaintQuad>,
    gutter_svgs: &mut Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    checkbox_hit_areas: &mut Vec<(Bounds<Pixels>, usize)>,
) {
    if !prefix_collapsed {
        return;
    }
    let BlockType::TaskList(checked) = block_type else {
        return;
    };

    let checkbox_styles = &editor.styles.checkbox;
    let checkbox_size = font_size * checkbox_styles.checkbox_size_factor;
    let prefix_bytes = mapped_prefix_bytes
        .unwrap_or_else(|| {
            helpers::compute_display_prefix_bytes(
                display_text,
                block_type,
                prefix_collapsed,
                collapsed_task_prefix_display_columns,
            )
        })
        .min(display_text.len());
    let text_start_x = wrapped
        .position_for_index(prefix_bytes, px(line_height))
        .map(|pos| f32::from(pos.x))
        .unwrap_or(0.0);
    let checkbox_x = content_x + text_start_x - checkbox_text_gap_px - checkbox_size;
    let first_row_height = f32::from(wrapped.row_height(0, px(line_height)));
    let checkbox_y = text_line_top + (first_row_height - checkbox_size) / 2.0;
    let checkbox_bounds = Bounds::new(
        point(px(checkbox_x), px(checkbox_y)),
        size(px(checkbox_size), px(checkbox_size)),
    );
    push_checkbox_visuals(
        editor,
        *checked,
        checkbox_bounds,
        hit_line_idx,
        gutter_quads,
        gutter_svgs,
    );

    if let Some(line_idx) = hit_line_idx.filter(|_| !editor.active_tab_is_read_only()) {
        let hit_padding = editor.scaled(2.0);
        let hit_bounds = Bounds::new(
            point(px(checkbox_x - hit_padding), px(line_top)),
            size(px(checkbox_size + hit_padding * 2.0), px(total_line_height)),
        );
        checkbox_hit_areas.push((hit_bounds, line_idx));
    }
}

fn push_checkbox_visuals(
    editor: &EditorView,
    checked: bool,
    checkbox_bounds: Bounds<Pixels>,
    hit_line_idx: Option<usize>,
    gutter_quads: &mut Vec<PaintQuad>,
    gutter_svgs: &mut Vec<(Bounds<Pixels>, &'static str, Hsla)>,
) {
    let checkbox_styles = &editor.styles.checkbox;
    let checkbox_size = f32::from(checkbox_bounds.size.width);
    let checkbox_x = f32::from(checkbox_bounds.origin.x);
    let checkbox_y = f32::from(checkbox_bounds.origin.y);
    let corner_radius = px(editor.scaled(checkbox_styles.checkbox_radius_px));
    let checkbox_opacity =
        if hit_line_idx.is_some_and(|line_idx| editor.hovering_checkbox_line == Some(line_idx)) {
            checkbox_styles.checkbox_hover_opacity
        } else {
            checkbox_styles.checkbox_default_opacity
        };

    if checked {
        let mut checkbox_bg = checkbox_styles.checkbox_checked_bg;
        checkbox_bg.a *= checkbox_opacity;
        gutter_quads.push(quad(
            checkbox_bounds,
            Corners::all(corner_radius),
            checkbox_bg,
            px(0.0),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
        let icon_size = checkbox_size * checkbox_styles.checkbox_check_icon_scale;
        let icon_offset = (checkbox_size - icon_size) / 2.0;
        let mut icon_color = checkbox_styles.checkbox_check_icon_color;
        icon_color.a *= checkbox_opacity;
        gutter_svgs.push((
            Bounds::new(
                point(px(checkbox_x + icon_offset), px(checkbox_y + icon_offset)),
                size(px(icon_size), px(icon_size)),
            ),
            CHECK_SVG_PATH,
            icon_color,
        ));
    } else {
        let mut border_color = checkbox_styles.checkbox_border_color;
        border_color.a *= checkbox_opacity;
        gutter_quads.push(quad(
            checkbox_bounds,
            Corners::all(corner_radius),
            gpui::transparent_black(),
            px(editor.scaled(checkbox_styles.checkbox_border_width_px)),
            border_color,
            BorderStyle::default(),
        ));
    }
}

fn shape_front_matter_group_text(
    window: &mut Window,
    text: &str,
    font: &gpui::Font,
    color: Hsla,
    font_size: f32,
    letter_spacing: f32,
) -> Option<WrappedLine> {
    let run = TextRun {
        len: text.len().max(1),
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut shaped = window
        .text_system()
        .shape_text(
            if text.is_empty() {
                " ".into()
            } else {
                text.to_string().into()
            },
            px(font_size),
            &[run],
            None,
            None,
        )
        .ok()?
        .into_iter()
        .next()?;
    if letter_spacing != 0.0 {
        helpers::apply_letter_spacing(&mut shaped, letter_spacing);
    }
    Some(shaped)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_front_matter_group_paint(
    editor: &EditorView,
    plan: &helpers::FrontMatterGroupPlan,
    window: &mut Window,
    bounds: Bounds<Pixels>,
    wrapped: &WrappedLine,
    display_start: usize,
    content_x: f32,
    top: f32,
    line_height: f32,
    font_size: f32,
    content_width: f32,
    font: &gpui::Font,
    texts: &mut Vec<(Bounds<Pixels>, WrappedLine, Point<Pixels>, Pixels)>,
    hits: &mut Vec<super::input::FrontMatterTextHit>,
    deletes: &mut Vec<(Bounds<Pixels>, usize, usize)>,
    scroll_areas: &mut Vec<(Bounds<Pixels>, usize, f32)>,
    clipped_selections: &mut Vec<(Bounds<Pixels>, Vec<PaintQuad>)>,
) {
    use super::front_matter_groups::GroupKind;
    let buffer = &editor.buffer;
    let header = plan.group.header_line;

    let value_x = content_x + f32::from(wrapped.unwrapped_layout.x_for_index(display_start));
    let right = content_x + content_width;
    let viewport = bounds.intersect(&window.content_mask().bounds);
    let letter_spacing = editor.styles.editor.editor_letter_spacing_px;
    let selections: Vec<_> = editor
        .all_cursor_ranges()
        .into_iter()
        .map(|(head, anchor)| head.min(anchor)..head.max(anchor))
        .collect();
    let mut push_selections = |hit: &super::input::FrontMatterTextHit, display: &str, clip| {
        let quads: Vec<_> =
            helpers::front_matter_display_selection_ranges(display, &hit.source_map, &selections)
                .into_iter()
                .map(|span| {
                    let left = hit.shaped.unwrapped_layout.x_for_index(span.start);
                    let right = hit.shaped.unwrapped_layout.x_for_index(span.end);
                    fill(
                        Bounds::new(
                            hit.origin + point(left, px(0.0)),
                            size(right - left, px(line_height)),
                        ),
                        editor.theme.selection,
                    )
                })
                .collect();
        if !quads.is_empty() {
            clipped_selections.push((clip, quads));
        }
    };
    match plan.group.kind {
        GroupKind::Multiline => {
            if !plan.collapse_header {
                return;
            }
            let source = buffer.line_content(header);
            let Some(key) = editor
                .decorations
                .get(header)
                .and_then(|d| d.front_matter.as_ref())
                .and_then(|info| info.key_range.as_ref())
            else {
                return;
            };
            let end = source[key.end..]
                .find(':')
                .map(|col| key.end + col + 1)
                .unwrap_or(key.end);
            let label = &source[..end];
            let runs = super::front_matter_layout::label_runs(
                label,
                key.clone(),
                font,
                editor.styles.table.front_matter_key_text_color,
            );
            if let Some(mut shaped) = window
                .text_system()
                .shape_text(label.to_owned().into(), px(font_size), &runs, None, None)
                .ok()
                .and_then(|lines| lines.into_iter().next())
            {
                helpers::apply_letter_spacing(&mut shaped, letter_spacing);
                let origin = point(px(content_x), px(top));
                let clip = Bounds::new(
                    origin,
                    size(px((value_x - content_x).max(0.0)), px(line_height)),
                )
                .intersect(&viewport);
                if let Some(hit_bounds) = front_matter_type_hit_bounds(
                    Bounds::new(origin, size(shaped.width(), px(line_height))),
                    clip,
                ) {
                    let hit = super::input::FrontMatterTextHit {
                        bounds: hit_bounds,
                        origin,
                        shaped: shaped.clone(),
                        line_height,
                        source_map: helpers::front_matter_item_source_map(
                            label,
                            label,
                            buffer.line_start(header),
                        ),
                    };
                    push_selections(&hit, label, clip);
                    hits.push(hit);
                }
                texts.push((clip, shaped, origin, px(line_height)));
            }
        }
        GroupKind::List => {
            let clip = Bounds::new(
                point(px(value_x), px(top)),
                size(px((right - value_x).max(0.0)), px(line_height)),
            )
            .intersect(&viewport);
            if clip.size.width <= px(0.0) || clip.size.height <= px(0.0) {
                return;
            }
            let gap = editor.scaled(FRONT_MATTER_LIST_DELETE_GAP_PX);
            let delete_width = editor.scaled(FRONT_MATTER_LIST_DELETE_WIDTH_PX);
            let item_gap = editor.scaled(FRONT_MATTER_LIST_ITEM_GAP_PX);
            let source = buffer.text();
            let mut shaped_items = Vec::new();
            let mut extent = 0.0;
            for (index, item) in plan.group.items.iter().enumerate() {
                if let Some(shaped) = shape_front_matter_group_text(
                    window,
                    &item.display_text,
                    font,
                    editor.theme.text_primary,
                    font_size,
                    letter_spacing,
                ) {
                    let width = f32::from(shaped.width());
                    shaped_items.push((index, item, shaped, extent, width));
                    extent += width + gap + delete_width + item_gap;
                }
            }
            let max_offset = (extent - item_gap - f32::from(clip.size.width)).max(0.0);
            let offset = editor
                .front_matter_list_scroll_offsets
                .get(&header)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, max_offset);
            scroll_areas.push((clip, header, max_offset));
            let close = shape_front_matter_group_text(
                window,
                "×",
                font,
                editor.theme.text_secondary,
                font_size,
                0.0,
            );
            for (index, item, shaped, left, width) in shaped_items {
                let origin = point(px(value_x + left - offset), px(top));
                let text_bounds = Bounds::new(
                    origin,
                    size(px(width.max(editor.scaled(4.0))), px(line_height)),
                );
                if let Some(hit_bounds) = front_matter_type_hit_bounds(text_bounds, clip) {
                    let hit = super::input::FrontMatterTextHit {
                        bounds: hit_bounds,
                        origin,
                        shaped: shaped.clone(),
                        line_height,
                        source_map: helpers::front_matter_item_source_map(
                            source.get(item.source_range.clone()).unwrap_or(""),
                            &item.display_text,
                            item.source_range.start,
                        ),
                    };
                    push_selections(&hit, &item.display_text, clip);
                    hits.push(hit);
                    texts.push((clip, shaped, origin, px(line_height)));
                }
                let close_origin = origin + point(px(width + gap), px(0.0));
                let close_bounds =
                    Bounds::new(close_origin, size(px(delete_width), px(line_height)));
                if let Some(hit_bounds) = front_matter_type_hit_bounds(close_bounds, clip) {
                    if let Some(close) = &close {
                        texts.push((clip, close.clone(), close_origin, px(line_height)));
                    }
                    if !editor.active_tab_is_read_only() {
                        deletes.push((hit_bounds, header, index));
                    }
                }
            }
        }
    }
}

fn push_front_matter_boolean_paint(
    editor: &EditorView,
    decoration: &LineDecoration,
    line_content: &str,
    active_ranges: &[Range<usize>],
    display_text: &str,
    buffer_to_display: &[usize],
    wrapped: &WrappedLine,
    font_size: f32,
    line_height: f32,
    content_x: f32,
    text_line_top: f32,
    indent_px: f32,
    line_idx: usize,
    gutter_quads: &mut Vec<PaintQuad>,
    gutter_svgs: &mut Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    checkbox_hit_areas: &mut Vec<(Bounds<Pixels>, usize)>,
) {
    let Some(info) = decoration.front_matter.as_ref() else {
        return;
    };
    let crate::front_matter::FrontMatterValue::Boolean(checked) = info.value else {
        return;
    };
    if helpers::front_matter_row_is_active(active_ranges, line_content.len()) {
        return;
    }
    let (Some(&display_start), Some(&display_end)) = (
        buffer_to_display.get(info.value_range.start),
        buffer_to_display.get(info.value_range.end),
    ) else {
        return;
    };
    if display_start >= display_end || display_end > display_text.len() {
        return;
    }
    let position = if let Some(boundary_row) =
        helpers::wrap_boundary_byte_indices(wrapped, display_text)
            .iter()
            .position(|boundary| *boundary == display_start)
    {
        point(
            px(indent_px),
            wrapped.row_top(boundary_row + 1, px(line_height)),
        )
    } else if let Some(position) = wrapped.position_for_index(display_start, px(line_height)) {
        point(
            position.x + px(helpers::indent_for_y(indent_px, position.y, line_height)),
            position.y,
        )
    } else {
        return;
    };
    let checkbox_size = font_size * editor.styles.checkbox.checkbox_size_factor;
    let row_height = f32::from(wrapped.row_height(
        wrapped.row_for_y(position.y, px(line_height)),
        px(line_height),
    ));
    let checkbox_bounds = Bounds::new(
        point(
            px(content_x) + position.x,
            px(text_line_top + (row_height - checkbox_size) / 2.0) + position.y,
        ),
        size(px(checkbox_size), px(checkbox_size)),
    );
    push_checkbox_visuals(
        editor,
        checked,
        checkbox_bounds,
        Some(line_idx),
        gutter_quads,
        gutter_svgs,
    );
    if !editor.active_tab_is_read_only() {
        let padding = px(editor.scaled(2.0));
        let hit_bounds = Bounds::new(
            checkbox_bounds.origin - point(padding, padding),
            checkbox_bounds.size + size(padding * 2.0, padding * 2.0),
        );
        checkbox_hit_areas.push((hit_bounds, line_idx));
    }
}

fn push_unordered_list_marker_paint(
    editor: &EditorView,
    line_content: &str,
    block_type: &BlockType,
    prefix_collapsed: bool,
    wrapped: &WrappedLine,
    buffer_to_display: &[usize],
    font_size: f32,
    line_height: f32,
    content_x: f32,
    base_content_x: f32,
    text_line_top: f32,
    monospace_font: &Font,
    window: &mut Window,
    unordered_list_markers: &mut Vec<(WrappedLine, Point<Pixels>, Pixels)>,
) {
    if *block_type != BlockType::UnorderedList || !prefix_collapsed {
        return;
    }

    let marker_text = editor
        .unordered_list_display_marker_for_line(line_content)
        .trim_end()
        .to_string();
    if marker_text.is_empty() {
        return;
    }

    let marker_font_size = font_size
        + editor.scaled(
            editor
                .styles
                .editor
                .editor_unordered_list_marker_font_size_offset_px,
        );
    let mut marker_run = TextRun {
        len: marker_text.len(),
        font: monospace_font.clone(),
        color: editor.styles.editor.editor_list_marker_text_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    marker_run.font.weight = editor.styles.editor.editor_list_marker_font_weight;
    if let Ok(mut marker_wrapped_lines) = window.text_system().shape_text(
        marker_text.into(),
        px(marker_font_size),
        &[marker_run],
        None,
        None,
    ) && let Some(marker_wrapped) = marker_wrapped_lines.pop()
    {
        // The `>` prefix of a quoted list is concealed, so the marker's source
        // column is not its display column; map through the display index.
        let marker_display_index = raw_offset_to_display_offset(
            buffer_to_display,
            helpers::list_marker_indent(line_content),
        );
        let marker_anchor_x = if marker_display_index == 0 {
            base_content_x
        } else {
            content_x
                + f32::from(
                    wrapped
                        .position_for_index(marker_display_index, px(line_height))
                        .unwrap_or_default()
                        .x,
                )
        };
        let marker_offset = editor.scaled(
            editor
                .styles
                .editor
                .editor_unordered_list_marker_font_size_offset_px,
        );
        // Align the visible glyph, not its advance cell. Keep the hidden
        // prefix unchanged so child markers still align with their parent's text.
        let marker_ink_left = marker_wrapped
            .unwrapped_layout
            .runs
            .iter()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .filter_map(|glyph| {
                        let character = marker_wrapped.text.get(glyph.index..)?.chars().next()?;
                        let bounds = window
                            .text_system()
                            .typographic_bounds(run.font_id, px(marker_font_size), character)
                            .ok()?;
                        Some(f32::from(glyph.position.x + bounds.origin.x))
                    })
                    .collect::<Vec<_>>()
            })
            .reduce(f32::min)
            .unwrap_or(0.0);
        let marker_x =
            super::text_layout::unordered_list_marker_origin_x(marker_anchor_x, marker_ink_left);
        let marker_line_height = f32::from(wrapped.row_height(0, px(line_height)))
            + editor.scaled(
                editor
                    .styles
                    .editor
                    .editor_unordered_list_marker_line_height_offset_px,
            );
        unordered_list_markers.push((
            marker_wrapped,
            point(px(marker_x), px(text_line_top - marker_offset * 0.35)),
            px(marker_line_height),
        ));
    }
}

fn push_diff_inline_highlight_quads(
    quads: &mut Vec<PaintQuad>,
    spans: &[Range<usize>],
    display_text: &str,
    line_content_len: usize,
    buffer_to_display: &[usize],
    runs: &[TextRun],
    wrapped: &WrappedLine,
    content_x: f32,
    text_line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    fill_color: Hsla,
    border_color: Hsla,
    border_width_px: f32,
    horizontal_padding: Pixels,
    radius: Pixels,
    vertical_inset: Pixels,
    letter_spacing: f32,
    window: &mut Window,
) {
    if spans.is_empty() {
        return;
    }

    for span in spans {
        let start = span.start.min(line_content_len);
        let end = span.end.min(line_content_len);
        if start >= end {
            continue;
        }

        let disp_start = helpers::buffer_to_display_offset(buffer_to_display, start);
        let disp_end = helpers::buffer_to_display_offset(buffer_to_display, end);
        for bounds in helpers::build_highlight_bounds(
            display_text,
            runs,
            wrapped,
            disp_start,
            disp_end,
            content_x,
            text_line_top,
            line_height,
            font_size,
            indent_px,
            0.0,
            letter_spacing,
            window,
        ) {
            let bounds = pad_bounds_horizontally(bounds, horizontal_padding);
            let bounds = inset_bounds_vertically(bounds, vertical_inset);
            quads.push(quad(
                bounds,
                Corners::all(radius),
                fill_color,
                px(0.0),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
            quads.push(quad(
                bounds,
                Corners::all(radius),
                gpui::transparent_black(),
                px(border_width_px),
                border_color,
                BorderStyle::default(),
            ));
        }
    }
}

fn quote_block_colors(editor: &EditorView, kind: Option<CalloutKind>) -> (Hsla, Hsla) {
    let styles = &editor.styles.quote_block;
    match kind {
        Some(CalloutKind::Note) => (
            styles.quote_block_note_bg,
            styles.quote_block_note_border_color,
        ),
        Some(CalloutKind::Tip) => (
            styles.quote_block_tip_bg,
            styles.quote_block_tip_border_color,
        ),
        Some(CalloutKind::Important) => (
            styles.quote_block_important_bg,
            styles.quote_block_important_border_color,
        ),
        Some(CalloutKind::Warning) => (
            styles.quote_block_warning_bg,
            styles.quote_block_warning_border_color,
        ),
        Some(CalloutKind::Caution) => (
            styles.quote_block_caution_bg,
            styles.quote_block_caution_border_color,
        ),
        None => (styles.quote_block_bg, styles.quote_block_border_color),
    }
}

fn push_inline_code_background_quads(
    quads: &mut Vec<PaintQuad>,
    decoration: &crate::markdown::LineDecoration,
    display_text: &str,
    line_content_len: usize,
    buffer_to_display: &[usize],
    runs: &[TextRun],
    wrapped: &WrappedLine,
    content_x: f32,
    text_line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    fill_color: Hsla,
    horizontal_padding: Pixels,
    radius: Pixels,
    vertical_inset: Pixels,
    letter_spacing: f32,
    window: &mut Window,
) {
    for range in decoration
        .inline_spans
        .iter()
        .filter(|span| span.kind == crate::markdown::InlineKind::InlineCode)
        .map(|span| &span.range)
    {
        let start = range.start.min(line_content_len);
        let end = range.end.min(line_content_len);
        if start >= end {
            continue;
        }

        let disp_start = helpers::buffer_to_display_offset(buffer_to_display, start);
        let disp_end = helpers::buffer_to_display_offset(buffer_to_display, end);
        for bounds in helpers::build_highlight_bounds(
            display_text,
            runs,
            wrapped,
            disp_start,
            disp_end,
            content_x,
            text_line_top,
            line_height,
            font_size,
            indent_px,
            0.0,
            letter_spacing,
            window,
        ) {
            let bounds = pad_bounds_horizontally(bounds, horizontal_padding);
            let bounds = inset_bounds_vertically(bounds, vertical_inset);
            quads.push(quad(
                bounds,
                Corners::all(radius),
                fill_color,
                px(0.0),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }
    }
}

#[cfg(test)]
fn table_cell_trimmed_display_range(display_text: &str) -> Range<usize> {
    helpers::table_cell_trimmed_display_range(display_text)
}

#[cfg(test)]
#[test]
fn table_cell_trim_preserves_inline_math_placeholder_and_never_reverses() {
    let placeholder = "\u{a0}".repeat(10);
    assert_eq!(
        table_cell_trimmed_display_range(&placeholder),
        0..placeholder.len()
    );
    assert_eq!(table_cell_trimmed_display_range("   \t"), 4..4);
    assert_eq!(table_cell_trimmed_display_range("  value  "), 2..7);

    let mut decoration = crate::markdown::LineDecoration::default();
    decoration
        .display_replacements
        .push(crate::markdown::DisplayReplacement {
            source_range: 0..5,
            display_text: placeholder.clone(),
            trigger_range: 0..5,
        });
    let (display, display_to_buffer, buffer_to_display, _) = helpers::build_display_text(
        "$x+y$",
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
    let range = table_cell_trimmed_display_range(&display);
    assert_eq!(&display[range.clone()], placeholder);
    assert_eq!(display_to_buffer[range].len(), placeholder.len());
    assert!(!buffer_to_display.is_empty());
}

fn build_precise_table_layout(
    window: &mut Window,
    buffer: &Buffer,
    decorations: &[LineDecoration],
    image_cache: &HashMap<String, Option<Arc<RenderImage>>>,
    table_start: usize,
    table_end: usize,
    text_width: f32,
    cell_padding_x: f32,
    cell_padding_y: f32,
    base_char_width: f32,
    line_height: f32,
    body_space_width: f32,
    task_prefix_columns: usize,
    task_source_indent_step_chars: usize,
    handle_size: f32,
    body_font: &Font,
    body_cjk_font: &Font,
    monospace_font: &Font,
    heading_font: &Font,
    heading_cjk_font: &Font,
    italic_font: &Font,
    italic_cjk_font: &Font,
    font_size: f32,
    letter_spacing: f32,
    superscript_scale_factor: f32,
    subscript_scale_factor: f32,
    theme: &Theme,
    list_marker_text_color: Hsla,
    list_marker_font_weight: FontWeight,
    heading_marker_text_color: Hsla,
    syntax_focus: &helpers::TableCellSyntaxFocus,
) -> helpers::TableLayout {
    let mut layout = helpers::compute_table_layout(
        buffer,
        decorations,
        table_start,
        table_end,
        text_width,
        cell_padding_x,
        cell_padding_y,
        base_char_width,
        line_height,
        handle_size,
    );
    let cell_padding_x = layout.cell_padding_x;
    let prepare_cell = |content: &str,
                        decoration: &LineDecoration,
                        active_ranges: &[Range<usize>],
                        is_header: bool| {
        let (folded, d2b, b2d, collapsed) = helpers::build_display_text(
            content,
            decoration,
            active_ranges,
            false,
            None,
            0,
            task_prefix_columns,
            task_prefix_columns,
            task_source_indent_step_chars,
            0,
            superscript_scale_factor,
            subscript_scale_factor,
            superscript_scale_factor,
        );
        let range =
            helpers::table_cell_display_range(&folded, content, decoration, &b2d, active_ranges);
        let display = folded[range.clone()].to_string();
        let d2b = d2b[range.clone()].to_vec();
        let b2d = b2d
            .iter()
            .map(|index| index.saturating_sub(range.start).min(range.len()))
            .collect::<Vec<_>>();
        let mut runs = helpers::build_text_runs_for_display(
            &display,
            content,
            &d2b,
            decoration,
            active_ranges,
            false,
            collapsed,
            body_font,
            body_cjk_font,
            monospace_font,
            heading_font,
            heading_cjk_font,
            italic_font,
            italic_cjk_font,
            0,
            font_size,
            theme,
            crate::theme::with_alpha(theme.git_status_added, 0.24),
            crate::theme::with_alpha(theme.text_danger, 0.28),
            theme.link,
            FontWeight::BOLD,
            list_marker_text_color,
            list_marker_font_weight,
            heading_marker_text_color,
            None,
            false,
        );
        if is_header {
            for run in &mut runs {
                run.font.weight = FontWeight::BOLD;
            }
        }
        (display, d2b, b2d, collapsed, runs)
    };

    // Measure the rendered hard lines, not Markdown source length. Use the
    // inactive presentation so revealing link/math syntax never resizes columns.
    let mut desired_widths = vec![base_char_width; layout.col_count];
    let mut cell_sources = Vec::with_capacity(layout.visual_row_count);
    for (vi, cells) in layout.cells.iter_mut().enumerate() {
        let source_line = layout.visual_row_to_line[vi];
        let row_decoration = &decorations[source_line];
        let mut sources = Vec::with_capacity(cells.len());
        for cell in cells {
            let Some(info) = row_decoration.table_info.as_ref() else {
                continue;
            };
            let (content, range) = super::table_cell_render::content_with_range(
                info,
                buffer.line_content(source_line),
                cell.col,
            );
            cell.cell_range = range;
            let mut decoration =
                helpers::table_cell_decoration(row_decoration, &content, &cell.cell_range);
            helpers::add_inline_math_display_replacements(
                &mut decoration,
                &content,
                image_cache,
                body_space_width,
            );
            let (display, _, b2d, _, runs) =
                prepare_cell(&content, &decoration, &[], layout.is_header[vi]);
            if let Ok(mut lines) = window.text_system().shape_text(
                if display.is_empty() {
                    " ".into()
                } else {
                    display.clone().into()
                },
                px(font_size),
                &runs,
                None,
                None,
            ) {
                for ((line, range), fragment) in lines
                    .iter_mut()
                    .zip(helpers::hard_line_byte_ranges(&display))
                    .zip(&decoration.table_cell_fragments)
                {
                    helpers::apply_letter_spacing(line, letter_spacing);
                    helpers::keep_inline_math_placeholders_unwrapped(
                        line,
                        &decoration,
                        &content,
                        &b2d,
                        &[],
                        image_cache,
                        range.start,
                    );
                    let measured_end = super::table_cell_render::intrinsic_line_end(
                        &content, fragment, &b2d, &range,
                    );
                    let width = f32::from(line.unwrapped_layout.x_for_index(measured_end)).ceil();
                    desired_widths[cell.col] = desired_widths[cell.col].max(width);
                }
            }
            sources.push((content, decoration));
        }
        cell_sources.push(sources);
    }
    let total_padding = cell_padding_x * 2.0 * layout.col_count as f32;
    let available_content = (text_width - total_padding).max(layout.col_count as f32);
    let column_widths = super::table_column_widths::allocate(
        &desired_widths,
        available_content,
        base_char_width.max(1.0),
    );
    for cells in &mut layout.cells {
        let mut x = 0.0;
        for cell in cells {
            cell.x = x;
            cell.width = column_widths[cell.col] + cell_padding_x * 2.0;
            x += cell.width;
        }
    }
    layout.total_width = column_widths.iter().sum::<f32>() + total_padding;

    let mut row_heights = vec![line_height + cell_padding_y * 2.0; layout.visual_row_count];
    for vi in 0..layout.visual_row_count {
        let Some(source_line) = layout.visual_row_to_line.get(vi).copied() else {
            continue;
        };
        let line_start_offset = buffer.line_start(source_line);
        let mut row_height = line_height + cell_padding_y * 2.0;

        if let Some(row_cells) = layout.cells.get_mut(vi) {
            for cell_layout in row_cells.iter_mut() {
                let Some((cell_content, cell_decoration)) = cell_sources[vi].get(cell_layout.col)
                else {
                    continue;
                };
                let mut cell_active_ranges = helpers::table_cell_active_ranges(
                    line_start_offset,
                    &cell_layout.cell_range,
                    syntax_focus,
                    cell_decoration,
                );
                let inner_width = (cell_layout.width - cell_padding_x * 2.0).max(1.0);
                if let Ok(prepared) = super::table_cell_render::prepare_cell(
                    window,
                    cell_content,
                    cell_decoration,
                    &mut cell_active_ranges,
                    inner_width,
                    font_size,
                    letter_spacing,
                    line_height,
                    image_cache,
                    |active| {
                        prepare_cell(
                            cell_content,
                            cell_decoration,
                            active,
                            layout.is_header.get(vi).copied().unwrap_or(false)
                                && !layout.is_front_matter,
                        )
                    },
                ) {
                    let (display, d2b, b2d, prefix_collapsed, _) = prepared.display;
                    cell_layout.display_text = display;
                    cell_layout.prefix_collapsed = prefix_collapsed;
                    cell_layout.display_to_buffer = d2b;
                    cell_layout.buffer_to_display = b2d;
                    let cell_text_height = prepared
                        .lines
                        .iter()
                        .map(|line| f32::from(line.size(px(line_height)).height))
                        .sum::<f32>();
                    row_height = row_height.max(cell_text_height + cell_padding_y * 2.0);
                }
            }
        }

        row_heights[vi] = row_height;
    }

    let mut current_y = 0.0_f32;
    for (vi, row) in layout.cells.iter_mut().enumerate() {
        let row_height = row_heights
            .get(vi)
            .copied()
            .unwrap_or(line_height + cell_padding_y * 2.0);
        for cell in row.iter_mut() {
            cell.y = current_y;
            cell.height = row_height;
        }
        current_y += row_height;
    }
    layout.row_heights = row_heights;
    layout.total_height = current_y;
    layout
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

fn push_whitespace_dot_quads(
    dots: &mut Vec<PaintQuad>,
    text: &str,
    wrapped: &WrappedLine,
    origin: Point<Pixels>,
    line_height: f32,
    dot_size: f32,
    dot_color: Hsla,
    skip_prefix_bytes: usize,
) {
    let dot_radius = px(dot_size / 2.0);
    for (index, character) in text.char_indices() {
        if index < skip_prefix_bytes {
            continue;
        }
        if character != ' ' {
            continue;
        }
        let Some(start) = wrapped.position_for_index(index, px(line_height)) else {
            continue;
        };
        let next_index = index + character.len_utf8();
        let end = wrapped
            .position_for_index(next_index.min(text.len()), px(line_height))
            .unwrap_or(start);
        let start_x = f32::from(start.x);
        let start_y = f32::from(start.y);
        let end_x = if f32::from(end.y) == start_y {
            f32::from(end.x)
        } else {
            start_x + line_height * 0.45
        };
        let width = (end_x - start_x).abs().max(dot_size);
        let dot_x = f32::from(origin.x) + start_x + (width - dot_size) * 0.5;
        let row_height = f32::from(
            wrapped.row_height(wrapped.row_for_y(start.y, px(line_height)), px(line_height)),
        );
        let dot_y = f32::from(origin.y) + start_y + (row_height - dot_size) * 0.5;
        dots.push(quad(
            Bounds::new(
                point(px(dot_x), px(dot_y)),
                size(px(dot_size), px(dot_size)),
            ),
            Corners::all(dot_radius),
            dot_color,
            px(0.0),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
    }
}

fn compute_wrap_boundary_upstream_xs(
    window: &mut Window,
    wrapped: &WrappedLine,
    display_text: &str,
    runs: &[TextRun],
    font_size: f32,
    line_height: f32,
    letter_spacing: f32,
) -> HashMap<usize, f32> {
    let mut upstream_xs = HashMap::new();
    let mut segment_start = 0;

    for boundary in helpers::wrap_boundary_byte_indices(wrapped, display_text) {
        let segment_text = &display_text[segment_start..boundary];
        if segment_text.contains('\u{a0}') {
            upstream_xs.insert(
                boundary,
                f32::from(
                    wrapped.unwrapped_layout.x_for_index(boundary)
                        - wrapped.unwrapped_layout.x_for_index(segment_start),
                ),
            );
            segment_start = boundary;
            continue;
        }
        let segment_runs = helpers::extract_runs_for_range(runs, segment_start, boundary);
        let segment_shared: SharedString = if segment_text.is_empty() {
            " ".into()
        } else {
            segment_text.to_string().into()
        };

        if let Ok(segment_lines) = window.text_system().shape_text(
            segment_shared,
            px(font_size),
            &segment_runs,
            None,
            None,
        ) && let Some(mut segment_wrapped) = segment_lines.into_iter().next()
        {
            if letter_spacing != 0.0 {
                helpers::apply_letter_spacing(&mut segment_wrapped, letter_spacing);
            }
            let segment_end_x = segment_wrapped
                .position_for_index(segment_text.len(), px(line_height))
                .map(|pos| f32::from(pos.x))
                .unwrap_or(0.0);
            upstream_xs.insert(boundary, segment_end_x);
        }

        segment_start = boundary;
    }

    upstream_xs
}

fn upstream_wrap_boundary_position(
    wrapped: &WrappedLine,
    display_text: &str,
    disp_col: usize,
    upstream_xs: &HashMap<usize, f32>,
    line_height: f32,
) -> Option<Point<Pixels>> {
    let upstream_x = *upstream_xs.get(&disp_col)?;
    let previous_boundary = display_text[..disp_col]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0);
    let previous_pos = wrapped.position_for_index(previous_boundary, px(line_height))?;
    Some(point(px(upstream_x), previous_pos.y))
}

fn downstream_wrap_boundary_position(
    wrapped: &WrappedLine,
    display_text: &str,
    disp_col: usize,
    line_height: f32,
) -> Option<Point<Pixels>> {
    let downstream_visual_idx = helpers::wrap_boundary_byte_indices(wrapped, display_text)
        .iter()
        .position(|boundary| *boundary == disp_col)?
        + 1;
    Some(point(
        px(0.0),
        wrapped.row_top(downstream_visual_idx, px(line_height)),
    ))
}

fn push_gutter_line_number_label(
    gutter_labels: &mut Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    window: &mut Window,
    line_number: usize,
    is_active: bool,
    font: &Font,
    font_size: f32,
    gutter_right_x: f32,
    text_line_top: f32,
    line_height: f32,
    vertical_offset: f32,
    right_gap: f32,
    text_color: Hsla,
    active_text_color: Hsla,
) {
    let line_number = line_number.to_string();
    let line_number_run = TextRun {
        len: line_number.len(),
        font: font.clone(),
        color: if is_active {
            active_text_color
        } else {
            text_color
        },
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(
        line_number.into(),
        px(font_size),
        &[line_number_run],
        None,
    );
    let label_x = gutter_right_x - right_gap - f32::from(shaped.width);
    let label_y = (text_line_top - vertical_offset).round();
    gutter_labels.push((shaped, point(px(label_x), px(label_y)), px(line_height)));
}

fn push_code_block_line_number_label(
    gutter_labels: &mut Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    window: &mut Window,
    line_number: usize,
    is_active: bool,
    font: &Font,
    font_size: f32,
    left_x: f32,
    text_line_top: f32,
    line_height: f32,
    vertical_offset: f32,
    text_color: Hsla,
    active_text_color: Hsla,
) {
    let line_number = line_number.to_string();
    let line_number_run = TextRun {
        len: line_number.len(),
        font: font.clone(),
        color: if is_active {
            active_text_color
        } else {
            text_color
        },
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(
        line_number.into(),
        px(font_size),
        &[line_number_run],
        None,
    );
    let label_y = (text_line_top - vertical_offset).round();
    gutter_labels.push((shaped, point(px(left_x), px(label_y)), px(line_height)));
}

fn leading_indent_columns(line: &str) -> usize {
    let mut columns = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            _ => break,
        }
    }
    columns
}

pub(super) fn detect_indent_guide_step(buffer: &crate::buffer::Buffer, line_count: usize) -> usize {
    detect_indent_guide_step_in_range(buffer, 0, line_count)
}

fn detect_indent_guide_step_in_range(
    buffer: &crate::buffer::Buffer,
    start_line: usize,
    end_line: usize,
) -> usize {
    let mut previous_indent = None;
    let mut detected_step = usize::MAX;

    for line_idx in start_line..end_line.min(buffer.line_count()) {
        let line = buffer.line_content(line_idx);
        if line.trim().is_empty() {
            continue;
        }

        let indent = leading_indent_columns(line);
        if let Some(previous) = previous_indent
            && indent > previous
        {
            detected_step = detected_step.min(indent - previous);
        }
        previous_indent = Some(indent);
    }

    if detected_step == usize::MAX {
        4
    } else {
        detected_step.clamp(1, 8)
    }
}

fn push_indent_guide_quads(
    quads: &mut Vec<PaintQuad>,
    line_content: &str,
    content_x: f32,
    line_top: f32,
    total_line_height: f32,
    char_width: f32,
    indent_step_columns: usize,
    guide_width: f32,
    dash_len: f32,
    gap_len: f32,
    guide_color: Hsla,
) {
    if indent_step_columns == 0 || line_content.trim().is_empty() {
        return;
    }

    let indent_columns = leading_indent_columns(line_content);
    let guide_count = indent_columns / indent_step_columns;
    if guide_count == 0 {
        return;
    }

    let level_width = indent_step_columns as f32 * char_width;
    if gap_len <= 0.0 {
        for level in 0..guide_count {
            let guide_x = content_x + level as f32 * level_width;
            quads.push(fill(
                Bounds::new(
                    point(px(guide_x), px(line_top)),
                    size(px(guide_width), px(total_line_height.max(0.0))),
                ),
                guide_color,
            ));
        }
        return;
    }

    let segment_step = (dash_len + gap_len).max(1.0);
    for level in 0..guide_count {
        let guide_x = content_x + level as f32 * level_width;
        let mut y = line_top;
        let line_bottom = line_top + total_line_height;
        while y < line_bottom {
            let height = dash_len.min(line_bottom - y);
            quads.push(fill(
                Bounds::new(point(px(guide_x), px(y)), size(px(guide_width), px(height))),
                guide_color,
            ));
            y += segment_step;
        }
    }
}

fn inline_overlay_prefers_latin_font(ch: char) -> bool {
    matches!(
        ch,
        '\u{0000}'..='\u{024F}'
            | '\u{1E00}'..='\u{1EFF}'
            | '\u{2000}'..='\u{206F}'
            | '\u{20A0}'..='\u{20CF}'
    )
}

fn inline_overlay_runs(
    text: &str,
    latin_font: &Font,
    cjk_font: &Font,
    color: Hsla,
) -> Vec<TextRun> {
    text.chars()
        .map(|ch| TextRun {
            len: ch.len_utf8(),
            font: if inline_overlay_prefers_latin_font(ch) {
                latin_font.clone()
            } else {
                cjk_font.clone()
            },
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect()
}

#[derive(Clone, Copy)]
enum InlineOverlayVerticalAlign {
    Top(f32),
    Bottom(f32),
    Offset(f32),
}

fn push_inline_text_overlay(
    overlays: &mut Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    window: &mut Window,
    wrapped: &WrappedLine,
    overlay_text: &str,
    visible_range: Range<usize>,
    latin_font: &Font,
    cjk_font: &Font,
    color: Hsla,
    font_size: f32,
    scale: f32,
    vertical_align: InlineOverlayVerticalAlign,
    anchor_right: bool,
    content_x: f32,
    text_line_top: f32,
    indent_px: f32,
    line_height: f32,
) {
    if visible_range.start >= visible_range.end || overlay_text.is_empty() {
        return;
    }

    let Some(pos_start) = wrapped.position_for_index(visible_range.start, px(line_height)) else {
        return;
    };
    let Some(pos_end) = wrapped.position_for_index(visible_range.end, px(line_height)) else {
        return;
    };
    if pos_start.y != pos_end.y {
        return;
    }

    let overlay_runs = inline_overlay_runs(overlay_text, latin_font, cjk_font, color);
    let overlay_font_size = (font_size * scale).max(8.0);
    let shaped = window.text_system().shape_line(
        SharedString::from(overlay_text.to_string()),
        px(overlay_font_size),
        &overlay_runs,
        None,
    );
    let indent_start = helpers::indent_for_y(indent_px, pos_start.y, line_height);
    let reserved_width = f32::from(pos_end.x - pos_start.x).max(0.0);
    let overlay_width = f32::from(shaped.width);
    let horizontal_offset = if anchor_right {
        (reserved_width - overlay_width).max(0.0)
    } else {
        0.0
    };
    let overlay_line_height = px((overlay_font_size * 1.15).max(overlay_font_size + 1.0));
    let line_top = text_line_top + f32::from(pos_start.y);
    let row_height = f32::from(wrapped.row_height(
        wrapped.row_for_y(pos_start.y, px(line_height)),
        px(line_height),
    ));
    let line_bottom = line_top + row_height;
    let overlay_origin_y = match vertical_align {
        InlineOverlayVerticalAlign::Top(inset) => line_top + inset,
        InlineOverlayVerticalAlign::Bottom(inset) => {
            line_bottom - f32::from(overlay_line_height) - inset
        }
        InlineOverlayVerticalAlign::Offset(offset) => {
            line_top + (row_height - line_height) / 2.0 + offset
        }
    };
    let overlay_origin = point(
        px(content_x + indent_start + horizontal_offset) + pos_start.x,
        px(overlay_origin_y),
    );
    overlays.push((shaped, overlay_origin, overlay_line_height));
}

fn push_rendered_inline_overlays(
    editor: &EditorView,
    window: &mut Window,
    decoration: &crate::markdown::LineDecoration,
    line_content: &str,
    display_text: &str,
    buffer_to_display: &[usize],
    active_ranges: &[Range<usize>],
    wrapped: &WrappedLine,
    body_font: &Font,
    body_cjk_font: &Font,
    theme: &Theme,
    font_size: f32,
    content_x: f32,
    text_line_top: f32,
    indent_px: f32,
    line_height: f32,
    inline_svgs: &mut Vec<(Bounds<Pixels>, &'static str, Hsla)>,
    inline_overlays: &mut Vec<(ShapedLine, Point<Pixels>, Pixels)>,
) {
    if let Some(callout_kind) = decoration.callout_kind
        && let Some(replacement) = decoration.display_replacements.iter().find(|replacement| {
            line_content
                .get(replacement.source_range.clone())
                .is_some_and(|source| source.trim_start().starts_with("[!"))
        })
        && !helpers::active_ranges_intersect(active_ranges, &replacement.trigger_range)
    {
        let disp_start =
            helpers::buffer_to_display_offset(buffer_to_display, replacement.source_range.start);
        let disp_end = (disp_start + callout_icon_placeholder_len(&replacement.display_text))
            .min(display_text.len());
        if let (Some(pos_start), Some(pos_end)) = (
            wrapped.position_for_index(disp_start, px(line_height)),
            wrapped.position_for_index(disp_end, px(line_height)),
        ) && pos_start.y == pos_end.y
        {
            let indent_start = helpers::indent_for_y(indent_px, pos_start.y, line_height);
            // Left-align the callout icon (no left gap) and size it to fill the
            // placeholder minus a small style-defined gap to the label, so the
            // icon→label spacing is controlled from style.rs.
            let callout_gap = editor.scaled(
                editor
                    .styles
                    .quote_block
                    .quote_block_callout_icon_label_gap_px,
            );
            let display_width = f32::from(pos_end.x - pos_start.x);
            let quote_styles = &editor.styles.quote_block;
            let icon_size = callout_icon_size(
                display_width - callout_gap,
                font_size,
                line_height,
                quote_styles.quote_block_callout_icon_size_factor,
                editor.scaled(quote_styles.quote_block_callout_icon_min_px),
                quote_styles.quote_block_callout_icon_max_line_height_factor,
            );
            let icon_x = content_x + indent_start + f32::from(pos_start.x);
            let row_height = f32::from(wrapped.row_height(
                wrapped.row_for_y(pos_start.y, px(line_height)),
                px(line_height),
            ));
            let icon_y = text_line_top + f32::from(pos_start.y) + (row_height - icon_size) / 2.0;
            inline_svgs.push((
                Bounds::new(
                    point(px(icon_x), px(icon_y)),
                    size(px(icon_size), px(icon_size)),
                ),
                callout_icon_path(callout_kind),
                editor.styles.quote_block.quote_block_callout_icon_color,
            ));
        }
    }

    for superscript_range in &decoration.superscript_spans {
        if !helpers::inline_range_is_collapsed(decoration, superscript_range, active_ranges) {
            continue;
        }
        let disp_start =
            helpers::buffer_to_display_offset(buffer_to_display, superscript_range.start);
        let disp_end = helpers::buffer_to_display_offset(buffer_to_display, superscript_range.end);
        push_inline_text_overlay(
            inline_overlays,
            window,
            wrapped,
            line_content
                .get(superscript_range.clone())
                .unwrap_or_default(),
            disp_start..disp_end,
            body_font,
            body_cjk_font,
            theme.syntax_marker,
            font_size,
            editor.styles.editor.editor_superscript_scale_factor,
            InlineOverlayVerticalAlign::Top(
                editor.scaled(editor.styles.editor.editor_superscript_offset_px),
            ),
            false,
            content_x,
            text_line_top,
            indent_px,
            line_height,
        );
    }

    for subscript_range in &decoration.subscript_spans {
        if !helpers::inline_range_is_collapsed(decoration, subscript_range, active_ranges) {
            continue;
        }
        let disp_start =
            helpers::buffer_to_display_offset(buffer_to_display, subscript_range.start);
        let disp_end = helpers::buffer_to_display_offset(buffer_to_display, subscript_range.end);
        push_inline_text_overlay(
            inline_overlays,
            window,
            wrapped,
            line_content
                .get(subscript_range.clone())
                .unwrap_or_default(),
            disp_start..disp_end,
            body_font,
            body_cjk_font,
            theme.syntax_marker,
            font_size,
            editor.styles.editor.editor_subscript_scale_factor,
            InlineOverlayVerticalAlign::Bottom(
                editor.scaled(editor.styles.editor.editor_subscript_offset_px),
            ),
            false,
            content_x,
            text_line_top,
            indent_px,
            line_height,
        );
    }

    if decoration.footnote_definition_label.is_none() {
        for footnote_reference in &decoration.footnote_references {
            if !helpers::inline_range_is_collapsed(
                decoration,
                &footnote_reference.collapsed_range,
                active_ranges,
            ) {
                continue;
            }
            let disp_start = helpers::buffer_to_display_offset(
                buffer_to_display,
                footnote_reference.collapsed_range.start,
            );
            let disp_end = helpers::buffer_to_display_offset(
                buffer_to_display,
                footnote_reference.collapsed_range.end,
            );
            push_inline_text_overlay(
                inline_overlays,
                window,
                wrapped,
                line_content
                    .get(footnote_reference.collapsed_range.clone())
                    .unwrap_or_default(),
                disp_start..disp_end,
                body_font,
                body_cjk_font,
                theme.link,
                font_size,
                editor.styles.editor.editor_superscript_scale_factor,
                InlineOverlayVerticalAlign::Offset(-line_height * 0.14),
                false,
                content_x,
                text_line_top,
                indent_px,
                line_height,
            );
        }
    }
}

fn inline_math_image_bounds(
    placeholder: Bounds<Pixels>,
    natural_size: gpui::Size<Pixels>,
) -> Bounds<Pixels> {
    Bounds::new(
        point(
            placeholder.origin.x,
            placeholder.origin.y + (placeholder.size.height - natural_size.height) / 2.0,
        ),
        natural_size,
    )
}

#[cfg(test)]
#[test]
fn inline_math_keeps_intrinsic_size_without_horizontal_padding() {
    for (line_height, image_height, expected_y) in [
        (24.0, 12.0, 106.0),
        (60.0, 60.0, 100.0),
        (80.0, 60.0, 110.0),
    ] {
        let bounds = inline_math_image_bounds(
            Bounds::new(point(px(20.0), px(100.0)), size(px(66.0), px(line_height))),
            size(px(61.0), px(image_height)),
        );
        assert_eq!(bounds.origin, point(px(20.0), px(expected_y)));
        assert_eq!(bounds.size, size(px(61.0), px(image_height)));
        assert!(bounds.bottom() <= px(100.0 + line_height));
    }
}

fn push_inline_math_images(
    decoration: &crate::markdown::LineDecoration,
    line_content: &str,
    display_text: &str,
    buffer_to_display: &[usize],
    active_ranges: &[Range<usize>],
    display_offset: usize,
    wrapped: &WrappedLine,
    image_cache: &HashMap<String, Option<Arc<RenderImage>>>,
    content_x: f32,
    text_line_top: f32,
    indent_px: f32,
    line_height: f32,
    clip_bounds: Option<Bounds<Pixels>>,
    images: &mut Vec<(
        Bounds<Pixels>,
        Arc<RenderImage>,
        Pixels,
        Option<Bounds<Pixels>>,
    )>,
) {
    for source_range in &decoration.inline_math_spans {
        let trigger_range =
            source_range.start.saturating_sub(1)..(source_range.end + 1).min(line_content.len());
        if helpers::active_ranges_intersect(active_ranges, &trigger_range) {
            continue;
        }
        let Some(source_text) = line_content.get(source_range.clone()) else {
            continue;
        };
        let cache_key = helpers::inline_math_cache_key(source_text);
        let Some(Some(image)) = image_cache.get(&cache_key) else {
            continue;
        };
        let Some(display_range) =
            helpers::inline_math_display_range(decoration, source_range, buffer_to_display)
        else {
            continue;
        };
        if display_range.start < display_offset
            || display_range.end > display_offset + display_text.len()
        {
            continue;
        }
        let display_start = display_range.start - display_offset;
        let display_end = display_range.end - display_offset;
        let Some(start) =
            downstream_wrap_boundary_position(wrapped, display_text, display_start, line_height)
                .or_else(|| wrapped.position_for_index(display_start, px(line_height)))
        else {
            continue;
        };
        let source_width = f32::from(
            wrapped.unwrapped_layout.x_for_index(display_end)
                - wrapped.unwrapped_layout.x_for_index(display_start),
        );
        let image_size = image.size(0);
        let (natural_width, natural_height) = helpers::intrinsic_generated_preview_size_for_kind(
            image_size.width.0 as f32,
            image_size.height.0 as f32,
            crate::markdown::BlockPreviewKind::Math,
        );
        if source_width <= 0.0 || natural_width <= 0.0 || natural_height <= 0.0 {
            continue;
        }

        let row_indent = helpers::indent_for_y(indent_px, start.y, line_height);
        let source_x = content_x + row_indent + f32::from(start.x);
        let source_y = text_line_top + f32::from(start.y);
        images.push((
            inline_math_image_bounds(
                Bounds::new(
                    point(px(source_x), px(source_y)),
                    size(
                        px(source_width),
                        wrapped.row_height(
                            wrapped.row_for_y(start.y, px(line_height)),
                            px(line_height),
                        ),
                    ),
                ),
                size(px(natural_width), px(natural_height)),
            ),
            image.clone(),
            px(0.0),
            clip_bounds,
        ));
    }
}

fn non_markdown_cached_line_can_reuse(
    cached: &LineDisplayInfo,
    current_wrap_width: Option<f32>,
) -> bool {
    const WIDTH_TOLERANCE_PX: f32 = 0.5;

    match current_wrap_width {
        Some(current_wrap_width) => {
            cached.wrap_width_px.is_some_and(|cached_wrap_width| {
                (cached_wrap_width - current_wrap_width).abs() <= WIDTH_TOLERANCE_PX
            }) || (cached.visual_line_count <= 1
                && cached.unwrapped_width_px <= current_wrap_width + WIDTH_TOLERANCE_PX)
        }
        None => cached.wrap_width_px.is_none() || cached.visual_line_count <= 1,
    }
}

// Retain shaping measurements across mode/scrollbar reconciliation attempts,
// but never publish intermediate geometry. Ordinary scrolling stays single-pass;
// three passes bound reflow work even for large documents.
struct EditorPrepaintFrame {
    reconcile_scroll: bool,
    pass: usize,
    previews_prepared: bool,
    offscreen_wrap_reflows: usize,
    generated_preview_height_changed: bool,
    explicit_caret_reveal_requested: bool,
    cursor_reveal_requested: bool,
}

impl EditorPrepaintFrame {
    const MAX_PASSES: usize = 3;
    const OFFSCREEN_WRAP_REFLOW_BUDGET: usize = 48;

    fn can_retry(&self) -> bool {
        self.reconcile_scroll && self.pass + 1 < Self::MAX_PASSES
    }

    fn allows_fallback_caret_preservation(&self) -> bool {
        !self.generated_preview_height_changed || self.explicit_caret_reveal_requested
    }

    fn should_retry(
        &self,
        painted_scroll: f32,
        target_scroll: f32,
        geometry_changed: bool,
    ) -> bool {
        self.can_retry()
            && (geometry_changed || !self.scroll_is_reconciled(painted_scroll, target_scroll))
    }

    fn scroll_is_reconciled(&self, painted_scroll: f32, target_scroll: f32) -> bool {
        !self.reconcile_scroll || (target_scroll - painted_scroll).abs() <= 0.5
    }
}

fn reconciled_editor_prepaint<T>(
    reconcile_scroll: bool,
    mut prepaint: impl FnMut(&mut EditorPrepaintFrame) -> Result<T, ()>,
) -> T {
    let mut frame = EditorPrepaintFrame {
        reconcile_scroll,
        pass: 0,
        previews_prepared: false,
        offscreen_wrap_reflows: EditorPrepaintFrame::OFFSCREEN_WRAP_REFLOW_BUDGET,
        generated_preview_height_changed: false,
        explicit_caret_reveal_requested: false,
        cursor_reveal_requested: false,
    };
    loop {
        match prepaint(&mut frame) {
            Ok(state) => return state,
            Err(()) => {
                assert!(frame.can_retry(), "final editor prepaint must be accepted");
                frame.pass += 1;
            }
        }
    }
}

#[cfg(test)]
mod passive_preview_reflow_tests {
    use super::*;
    use crate::editor::tests::test_line_display_info;
    use crate::markdown::parse_decorations;

    fn anchor_line(decorations: &[crate::markdown::LineDecoration]) -> usize {
        decorations
            .iter()
            .position(|decoration| {
                decoration
                    .block_preview
                    .as_ref()
                    .is_some_and(|p| p.is_anchor)
                    || !decoration.inline_math_spans.is_empty()
            })
            .expect("generated preview or inline math")
    }

    #[test]
    fn table_search_centers_the_painted_row_after_upstream_remeasurement() {
        for initial_scroll in [0.0, 9_000.0] {
            let mut scroll = initial_scroll;
            let mut painted_match_top = 0.0;
            let viewport_top = 80.0;
            let viewport_height = 600.0;
            let row_height = 24.0;
            let mut attempts = 0;
            reconciled_editor_prepaint(true, |frame| {
                attempts += 1;
                // The first layout measures previews above this table. Its
                // new prefix cache is not the origin used by this paint pass.
                let table_top = if frame.pass == 0 { 1_000.0 } else { 1_800.0 };
                painted_match_top = viewport_top + table_top + 2_400.0 - scroll;
                let geometry = TableCursorGeometry::from_paint(
                    100,
                    Bounds::new(
                        point(px(20.0), px(painted_match_top)),
                        size(px(2.0), px(row_height)),
                    ),
                    3,
                    painted_match_top..painted_match_top + row_height,
                    viewport_top,
                    scroll,
                );
                let painted_scroll = scroll;
                let (top, bottom) = geometry.vertical_bounds();
                scroll = ((top + bottom - viewport_height) * 0.5).max(0.0);
                if frame.should_retry(painted_scroll, scroll, false) {
                    return Err(());
                }
                assert!(frame.scroll_is_reconciled(painted_scroll, scroll));
                Ok(())
            });
            assert_eq!(attempts, 3);
            assert_eq!(
                painted_match_top + row_height * 0.5 - viewport_top,
                viewport_height * 0.5,
            );
            assert_eq!(scroll, 3_912.0);
        }
    }

    #[test]
    fn scroll_requests_survive_rejected_and_exhausted_layout_attempts() {
        let mut center_confirm_frames = 1usize;
        let mut typing_guard = Some(PendingTypingScrollGuard {
            line: 30,
            visual_row: 2,
            visual_geometry_ready: true,
            cursor_top: Some(1_200.0),
            viewport_height: 600.0,
            scroll_offset: 900.0,
        });
        let mut scroll = 900.0;
        let mut attempts = 0;
        // A growing preview changes the target on every attempt and exhausts
        // the bounded transaction. Neither request may be consumed yet.
        reconciled_editor_prepaint(true, |frame| {
            attempts += 1;
            let painted = scroll;
            let target = painted + 100.0;
            if frame.should_retry(painted, target, false) {
                scroll = target;
                return Err(());
            }
            if frame.scroll_is_reconciled(painted, target) {
                center_confirm_frames = center_confirm_frames.saturating_sub(1);
                typing_guard = None;
            }
            Ok(())
        });
        assert_eq!(attempts, EditorPrepaintFrame::MAX_PASSES);
        assert_eq!(center_confirm_frames, 1);
        assert!(typing_guard.is_some());

        attempts = 0;
        // The next frame has settled geometry. Its correction must survive
        // rejection, then be consumed exactly once by the painted attempt.
        reconciled_editor_prepaint(true, |frame| {
            attempts += 1;
            assert_eq!(center_confirm_frames, 1);
            assert!(typing_guard.is_some());
            let painted = scroll;
            let target = 1_300.0;
            if frame.should_retry(painted, target, false) {
                scroll = target;
                return Err(());
            }
            if frame.scroll_is_reconciled(painted, target) {
                center_confirm_frames = center_confirm_frames.saturating_sub(1);
                typing_guard = None;
            }
            Ok(())
        });
        assert_eq!(attempts, 2);
        assert_eq!(scroll, 1_300.0);
        assert_eq!(center_confirm_frames, 0);
        assert!(typing_guard.is_none());
    }

    #[test]
    fn wheel_offset_survives_passive_mermaid_and_math_growth() {
        for source in [
            "```mermaid\ngraph TD\nA-->B\n```\n",
            "$$\nx + y\n$$\n",
            "text $x + y$\n",
        ] {
            let decorations = parse_decorations(source);
            let line = anchor_line(&decorations);
            let updates = [(line, Some(test_line_display_info(&[600.0], 0.0)))];
            let scroll_before_wheel = 1_000.0;
            let mut scroll = scroll_before_wheel - 100.0;
            let viewport_height = 600.0;
            let previous_caret_bottom = 1_474.0;
            assert!(previous_caret_bottom <= scroll + viewport_height);
            let measured_caret_bottom = previous_caret_bottom + 300.0;
            let fallback_correction = measured_caret_bottom - (scroll + viewport_height);
            assert_eq!(fallback_correction, 274.0);
            assert_eq!(
                preview_height_anchor_delta(300.0, 600.0, 1_200.0, scroll as f64),
                0.0
            );
            reconciled_editor_prepaint(false, |frame| {
                frame.generated_preview_height_changed =
                    generated_preview_heights_changed(&decorations, &updates, |_| 300.0);
                assert!(frame.generated_preview_height_changed);
                // This is the final fallback's actual gate, not the explicit
                // scroll-to-cursor/navigation path. It must never run here.
                if frame.allows_fallback_caret_preservation() {
                    scroll += fallback_correction;
                }
                Ok(())
            });
            assert_eq!(scroll, 900.0);
            assert!(scroll < scroll_before_wheel);
        }
    }

    #[test]
    fn collapsed_mermaid_and_math_source_rows_count_even_without_anchor_changes() {
        for source in ["```mermaid\ngraph TD\nA-->B\n```\n", "$$\nx + y\n$$\n"] {
            let decorations = parse_decorations(source);
            let line = decorations
                .iter()
                .position(|decoration| {
                    decoration
                        .block_preview
                        .as_ref()
                        .is_some_and(|p| !p.is_anchor)
                })
                .expect("collapsed source row");
            // Only a non-anchor early-return row changes. Anchor-only detection
            // or detection inside the normal line-height branch would miss it.
            let updates = [(line, Some(test_line_display_info(&[], 0.0)))];
            assert!(generated_preview_heights_changed(
                &decorations,
                &updates,
                |_| 24.0
            ));
            assert!(!generated_preview_heights_changed(
                &decorations,
                &updates,
                |_| 0.0
            ));
            let expanded = [(line, Some(test_line_display_info(&[24.0], 0.0)))];
            assert!(generated_preview_heights_changed(
                &decorations,
                &expanded,
                |_| 0.0
            ));
        }
    }

    #[test]
    fn unchanged_previews_and_ordinary_text_reflow_keep_existing_caret_fallback() {
        let decorations = parse_decorations("text $x$\n");
        let line = anchor_line(&decorations);
        let updates = [(line, Some(test_line_display_info(&[24.0], 0.0)))];
        assert!(!generated_preview_heights_changed(
            &decorations,
            &updates,
            |_| 24.0
        ));
        assert!(!generated_preview_heights_changed(
            &decorations,
            &updates,
            |_| 24.001
        ));
        assert!(generated_preview_heights_changed(
            &decorations,
            &updates,
            |_| 24.125
        ));
        assert!(!generated_preview_heights_changed(
            &decorations,
            &[(line, None)],
            |_| 24.0
        ));
        let plain = parse_decorations("ordinary text\n");
        assert!(!generated_preview_heights_changed(&plain, &updates, |_| {
            600.0
        }));
        reconciled_editor_prepaint(false, |frame| {
            assert!(frame.allows_fallback_caret_preservation());
            Ok(())
        });
    }

    #[test]
    fn passive_preview_reflow_does_not_suppress_above_viewport_compensation() {
        let mut scroll = 900.0;
        let previous_caret_top = 1_450.0;
        let previous_screen_y = previous_caret_top - scroll;
        reconciled_editor_prepaint(false, |frame| {
            frame.generated_preview_height_changed = true;
            assert!(!frame.allows_fallback_caret_preservation());
            // The anchor path remains independent of fallback caret policy.
            scroll += preview_height_anchor_delta(300.0, 600.0, 850.0, scroll as f64);
            assert_eq!(scroll, 1_200.0);
            assert_eq!(previous_caret_top + 300.0 - scroll, previous_screen_y);
            assert_eq!(
                preview_height_anchor_delta(600.0, 600.0, 1_150.0, scroll as f64),
                0.0
            );
            Ok(())
        });
    }

    #[test]
    fn explicit_typing_and_caret_reveal_keep_fallback_after_guards_are_consumed() {
        let mut attempts = 0;
        reconciled_editor_prepaint(true, |frame| {
            attempts += 1;
            if frame.pass == 0 {
                frame.explicit_caret_reveal_requested = true;
                frame.generated_preview_height_changed = true;
                return Err(());
            }
            // Even if a typing/reveal guard was consumed on the first attempt,
            // its user intent must survive the transaction's final paint pass.
            assert!(frame.allows_fallback_caret_preservation());
            Ok(())
        });
        assert_eq!(attempts, 2);
    }

    #[test]
    fn passive_preview_flag_survives_retries_but_not_subsequent_frames() {
        let mut attempts = 0;
        reconciled_editor_prepaint(true, |frame| {
            attempts += 1;
            if frame.pass == 0 {
                frame.generated_preview_height_changed = true;
                return Err(());
            }
            // The next attempt may emit identical heights; that must not
            // re-enable the fallback within this same accepted frame.
            assert!(!frame.allows_fallback_caret_preservation());
            Ok(())
        });
        assert_eq!(attempts, 2);
        reconciled_editor_prepaint(false, |frame| {
            assert!(!frame.generated_preview_height_changed);
            assert!(!frame.explicit_caret_reveal_requested);
            assert!(frame.allows_fallback_caret_preservation());
            Ok(())
        });
    }
}

#[cfg(test)]
mod mode_prepaint_tests {
    use super::*;

    #[test]
    fn document_mode_prepaint_publishes_only_reconciled_paint_and_input_geometry() {
        // All of these coordinates are produced together by the real prepaint
        // pass. A late scroll correction must discard the entire collection,
        // not translate just text while leaving hit areas or the caret behind.
        let mut scroll = 1000.0;
        let mut attempts = 0;
        let mut publications = 0;
        let (paint_bounds, hit_bounds, selection_bounds, caret) =
            reconciled_editor_prepaint(true, |frame| {
                attempts += 1;
                let painted_scroll = scroll;
                let bounds = Bounds::new(
                    point(px(80.0), px(2200.0 - painted_scroll)),
                    size(px(600.0), px(28.0)),
                );
                let caret = super::super::caret::CaretGeometrySnapshot {
                    cursor_offset: 42,
                    scroll_offset: painted_scroll,
                    paints: vec![super::super::caret::CaretPaint {
                        bounds,
                        color: gpui::transparent_black(),
                        force_visible: true,
                    }],
                    ..Default::default()
                };
                // Measured wrapping moves the source row; the desired visual
                // position (300px) was captured before changing presentation.
                scroll = 1900.0;
                if frame.should_retry(painted_scroll, scroll, false) {
                    return Err(());
                }
                publications += 1;
                Ok((bounds, bounds, bounds, caret))
            });
        assert_eq!(attempts, 2);
        assert_eq!(publications, 1);
        assert_eq!(paint_bounds.origin.y, px(300.0));
        assert_eq!(hit_bounds, paint_bounds);
        assert_eq!(selection_bounds, paint_bounds);
        assert_eq!(caret.paints[0].bounds, paint_bounds);
        assert_eq!(caret.scroll_offset, scroll);
    }

    #[test]
    fn document_mode_prepaint_remeasures_phantom_padding_without_a_scroll_delta() {
        let mut measured_phantom_height = 0.0;
        let mut attempts = 0;
        let painted_height = reconciled_editor_prepaint(true, |frame| {
            attempts += 1;
            let height_used_by_layout = measured_phantom_height;
            let changed = measured_phantom_height != 84.0;
            measured_phantom_height = 84.0;
            if frame.should_retry(900.0, 900.0, changed) {
                return Err(());
            }
            Ok(height_used_by_layout)
        });
        assert_eq!(attempts, 2);
        assert_eq!(painted_height, 84.0);
    }

    #[test]
    fn document_mode_prepaint_bounds_attempts_and_retains_per_frame_work_budget() {
        let mut attempts = 0;
        let mut preview_builds = 0;
        let mut offscreen_reflows = 0;
        let mut scroll = 0.0;
        let painted_scroll = reconciled_editor_prepaint(true, |frame| {
            attempts += 1;
            if !frame.previews_prepared {
                preview_builds += 1;
                frame.previews_prepared = true;
            }
            let reflows = frame.offscreen_wrap_reflows.min(32);
            offscreen_reflows += reflows;
            frame.offscreen_wrap_reflows -= reflows;
            let painted_scroll = scroll;
            // Model an unstable layout. The last pass must keep the offset it
            // actually painted; unfinished correction waits for another frame.
            if frame.can_retry() {
                scroll += 120.0;
            }
            if frame.should_retry(painted_scroll, scroll, true) {
                return Err(());
            }
            Ok(painted_scroll)
        });
        assert_eq!(attempts, EditorPrepaintFrame::MAX_PASSES);
        assert_eq!(preview_builds, 1);
        assert_eq!(
            offscreen_reflows,
            EditorPrepaintFrame::OFFSCREEN_WRAP_REFLOW_BUDGET
        );
        assert_eq!(painted_scroll, scroll);
    }

    #[test]
    fn document_mode_prepaint_does_not_retry_ordinary_frames_or_pending_previews() {
        for mode_anchor in [false, true] {
            let mut attempts = 0;
            reconciled_editor_prepaint(mode_anchor, |frame| {
                attempts += 1;
                // No geometry changed while an async preview is in flight.
                assert!(!frame.should_retry(900.0, 900.0, false));
                if !mode_anchor {
                    assert!(!frame.should_retry(900.0, 1100.0, true));
                }
                Ok(())
            });
            assert_eq!(attempts, 1);
        }
    }

    #[test]
    fn scrollbar_reflow_reconciles_paint_and_hit_geometry_without_unbounded_work() {
        let mut scroll = 500.0;
        let mut attempts = 0;
        // A stationary drag's new range maps the pointer to a different offset.
        let (paint_scroll, hit_scroll) = reconciled_editor_prepaint(true, |frame| {
            attempts += 1;
            let painted = scroll;
            scroll = 1_200.0;
            if frame.should_retry(painted, scroll, false) {
                return Err(());
            }
            Ok((painted, painted))
        });
        assert_eq!(attempts, 2);
        assert_eq!((paint_scroll, hit_scroll), (scroll, scroll));
        assert!(attempts <= EditorPrepaintFrame::MAX_PASSES);
    }

    #[test]
    fn document_mode_prepaint_ignores_subpixel_corrections() {
        reconciled_editor_prepaint(true, |frame| {
            assert!(!frame.should_retry(900.0, 900.5, false));
            assert!(frame.should_retry(900.0, 900.51, false));
            assert!(frame.should_retry(900.0, 899.49, false));
            Ok(())
        });
    }
}

impl EditorElement {
    fn prepaint_document(
        &mut self,
        bounds: Bounds<Pixels>,
        frame: &mut EditorPrepaintFrame,
        caret_was_visible: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<EditorPrepaintState, ()> {
        // Capture intent before the reveal/typing guards are consumed. Retain it
        // across transactional retries, but never across separate frames.
        frame.explicit_caret_reveal_requested |= {
            let editor = self.editor.read(cx);
            editor.pending_typing_scroll_guard.is_some()
                || editor.pending_click_anchor.is_some()
                || editor.pending_scroll_to_cursor
                || editor.pending_center_cursor_in_view
                || editor.center_cursor_confirm_frames > 0
        };
        frame.cursor_reveal_requested |= self.editor.read(cx).pending_scroll_to_cursor;
        // Resolve a deferred scroll-to-cursor before reading layout state, when
        // the per-line geometry cache is already valid (the common case after an
        // edit like pasting long text). Otherwise the layout loop below positions
        // every line at the OLD scroll offset and the deferred scroll only runs
        // afterwards (followed by `cx.notify()`), so the first frame paints at the
        // stale offset and the next frame jumps to the corrected one. Resolving it
        // up front makes this frame paint at the final offset with no jump.
        self.editor.update(cx, |editor, _| {
            editor.pending_scroll_to_cursor |= frame.cursor_reveal_requested;
            if !editor.pending_scroll_to_cursor
                || editor.pending_center_cursor_in_view
                || editor.pending_preview_reveal.is_some()
                || (editor.table_cursor_cell().is_some() && !editor.active_tab_is_diff())
                // A live typing guard means the end-of-prepaint restore will
                // cancel this scroll when the cursor stays on its visual
                // line; resolving it here first would snap the view to the
                // viewport edge and then bounce back, jittering every
                // keystroke typed near the top/bottom edge bands.
                || editor.pending_typing_scroll_guard.is_some()
            {
                return;
            }
            let line_count = editor.buffer.line_count();
            let viewport_width: f32 = bounds.size.width.into();
            let expanded_layout = editor.editor_uses_expanded_layout();
            let editor_width = if expanded_layout {
                viewport_width
            } else {
                viewport_width.min(editor.max_editor_width())
            };
            let text_width = (editor_width - editor.padding_left() - editor.padding_right())
                .max(editor.scaled(120.0));
            let geometry_valid = editor.line_display_info.len() == line_count
                && (editor.last_text_width - text_width).abs() < 0.5;
            if !geometry_valid {
                return;
            }
            editor.last_viewport_height = bounds.size.height.into();
            editor.layout_dirty = false;
            editor.scroll_to_cursor();
        });

        // Embedded editors track their container height exactly: sync the
        // viewport and re-clamp the scroll before the layout loop reads it, so
        // container resizes never leave the content over-scrolled (content
        // pushed out of the top of the view, blank space below the last line).
        // Read-only transcripts additionally follow the bottom while attached.
        self.editor.update(cx, |editor, _| {
            if editor.embedded_editor {
                editor.last_viewport_height = bounds.size.height.into();
                if editor.embedded_read_only && editor.embedded_stick_to_bottom {
                    let painted = editor.embedded_content_height.get();
                    if painted > 0.0 {
                        editor.scroll_offset = (painted - editor.last_viewport_height).max(0.0);
                    }
                }
                editor.clamp_scroll();
            }
        });

        self.editor.update(cx, |editor, _| {
            if !editor.uses_lazy_markdown_decorations() {
                return;
            }
            let viewport_height: f32 = bounds.size.height.into();
            let overscan = viewport_height * 0.75;
            let start = editor.line_at_y((editor.scroll_offset - overscan).max(0.0));
            let end = editor
                .line_at_y(editor.scroll_offset + viewport_height + overscan)
                .saturating_add(1);
            editor.ensure_lazy_markdown_decorations(start..end);
            editor.record_large_document_syntax_viewport(start..end);
        });

        // Preview creation may synchronously replace placeholder geometry. Do it
        // before shaping, once per frame rather than once per retry.
        if frame.reconcile_scroll && !frame.previews_prepared {
            self.editor.update(cx, |editor, cx| {
                editor.last_viewport_height = bounds.size.height.into();
                editor.last_viewport_width = bounds.size.width.into();
                editor.ensure_visible_generated_block_previews(cx);
            });
            frame.previews_prepared = true;
        }

        let editor = self.editor.read(cx);
        let front_matter_group_plans =
            editor.front_matter_group_plan_for_width(f32::from(bounds.size.width));
        let collapsed_front_matter_range = editor.collapsed_front_matter_range();
        let front_matter_fold_control_line = super::fold::front_matter_fold_control_line(
            &editor.decorations,
            &front_matter_group_plans,
            collapsed_front_matter_range.is_some(),
        );
        let front_matter_summary_row = editor.front_matter_summary_line();
        let front_matter_delimiters_visible = editor.front_matter_delimiters_are_visible();
        let theme = &editor.theme;
        let buffer = &editor.buffer;
        let cursor = &editor.cursor;
        let decorations = &editor.decorations;
        let scroll_offset = editor.scroll_offset;
        let caret_cursor_offset = cursor.offset;
        let cursor_line = editor.cursor_line();
        let cursor_table_reveal_range = frame
            .explicit_caret_reveal_requested
            .then(|| decorations.get(cursor_line)?.table_info.as_ref())
            .flatten()
            .filter(|table| !table.is_front_matter)
            .map(|table| table.table_start_line..table.table_end_line);

        let viewport_height: f32 = bounds.size.height.into();
        let viewport_width: f32 = bounds.size.width.into();
        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let expanded_layout = editor.editor_uses_expanded_layout();
        let editor_width = if expanded_layout {
            viewport_width
        } else {
            viewport_width.min(editor.max_editor_width())
        };
        let centered_left = origin_x + ((viewport_width - editor_width) / 2.0).max(0.0);
        let editor_left = if expanded_layout {
            origin_x
        } else {
            centered_left
        };
        let text_width = (editor_width - editor.padding_left() - editor.padding_right())
            .max(editor.scaled(120.0));
        let wrap_width = text_width;
        let non_markdown_nowrap = editor.editor_uses_horizontal_text_scroll();
        let non_markdown_horizontal_state = editor.non_markdown_horizontal_scroll_state();
        let non_markdown_horizontal_offset = if non_markdown_nowrap {
            non_markdown_horizontal_state.offset
        } else {
            0.0
        };
        let non_markdown_content_left = editor_left + editor.padding_left();
        let horizontal_editor_clip_bounds = non_markdown_nowrap.then(|| {
            Bounds::new(
                point(px(non_markdown_content_left), px(origin_y)),
                size(px(text_width), px(viewport_height)),
            )
        });
        let mut non_markdown_horizontal_extent = 0.0_f32;
        let gutter_right_x = editor.gutter_line_number_right_x(editor_left);
        let gutter_icon_column_left = editor.gutter_icon_column_left(editor_left);
        let gutter_icon_column_width = editor.gutter_icon_column_width();

        let mut text_baseline_cache = Vec::new();
        let mut lines = Vec::new();
        let mut indent_guide_quads = Vec::new();
        let mut clipped_code_block_decorations = Vec::new();
        let mut clipped_table_decorations = Vec::new();
        let mut clipped_code_block_search_highlights = Vec::new();
        let mut clipped_code_block_selections = Vec::new();
        let mut clipped_table_search_highlights = Vec::new();
        let mut clipped_table_selections = Vec::new();
        let mut selection_quads = Vec::new();
        let mut search_match_quads: Vec<PaintQuad> = Vec::new();
        let mut line_highlight = None;
        let mut quote_block_bgs = Vec::new();
        let mut code_block_bgs = Vec::new();
        let mut whitespace_dots: Vec<PaintQuad> = Vec::new();
        let mut cursor_paints: Vec<super::caret::CaretPaint> = Vec::new();
        let mut table_cursor_paint_bounds = None;
        let mut marked_underlines = Vec::new();
        let mut link_underlines: Vec<PaintQuad> = Vec::new();
        let mut link_hitboxes: Vec<Hitbox> = Vec::new();

        let line_count = buffer.line_count();
        let has_valid_cache = editor.line_display_info.len() == line_count
            && (editor.last_text_width - text_width).abs() < 0.5;
        let wrap_width_changed = (editor.last_text_width - text_width).abs() >= 0.5;
        const OFFSCREEN_WRAP_REFLOW_BUDGET_PER_FRAME: usize =
            EditorPrepaintFrame::OFFSCREEN_WRAP_REFLOW_BUDGET;
        const WINDOWED_PREPAINT_OVERSCAN_FACTOR: f32 = 0.75;
        const WINDOWED_PREPAINT_CURSOR_CONTEXT_LINES: usize = 8;
        let active_file_kind = editor.active_file_kind();
        let active_tab_is_markdown = editor.active_tab_uses_markdown_editor();
        let use_windowed_prepaint = (!wrap_width_changed || editor.uses_sparse_line_geometry())
            && active_file_kind.is_some_and(|file_kind| {
                if active_tab_is_markdown {
                    has_valid_cache || editor.uses_lazy_markdown_decorations()
                } else {
                    file_kind.supports_text_buffer()
                }
            });
        let (render_start_line, render_end_line) = if use_windowed_prepaint && line_count > 0 {
            let overscan_height = viewport_height * WINDOWED_PREPAINT_OVERSCAN_FACTOR;
            let mut start = editor
                .line_at_y((scroll_offset - overscan_height).max(0.0))
                .saturating_sub(WINDOWED_PREPAINT_CURSOR_CONTEXT_LINES);
            let mut end = editor
                .line_at_y(scroll_offset + viewport_height + overscan_height)
                .saturating_add(1);

            if editor.editor_wraps_to_available_width() {
                end = end.saturating_add(OFFSCREEN_WRAP_REFLOW_BUDGET_PER_FRAME);
            }

            if active_tab_is_markdown {
                if let Some(table) = &cursor_table_reveal_range {
                    start = start.min(table.start);
                    end = end.max(table.end);
                }
                let mut expanded = true;
                while expanded {
                    expanded = false;
                    let scan_start = start.min(line_count);
                    let scan_end = end.min(line_count);
                    for line_idx in scan_start..scan_end {
                        let Some(decoration) = decorations.get(line_idx) else {
                            continue;
                        };

                        if matches!(
                            decoration.block_type,
                            BlockType::CodeBlock | BlockType::CodeBlockFence
                        ) {
                            let mut group_start = line_idx;
                            while group_start > 0
                                && matches!(
                                    decorations
                                        .get(group_start - 1)
                                        .map(|decoration| &decoration.block_type),
                                    Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                                )
                            {
                                group_start -= 1;
                            }

                            let mut group_end = line_idx.saturating_add(1);
                            while group_end < line_count
                                && matches!(
                                    decorations
                                        .get(group_end)
                                        .map(|decoration| &decoration.block_type),
                                    Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                                )
                            {
                                group_end += 1;
                            }

                            if group_start < start {
                                start = group_start;
                                expanded = true;
                            }
                            if group_end > end {
                                end = group_end;
                                expanded = true;
                            }
                        }

                        if let Some(table_info) = decoration.table_info.as_ref()
                            && (!table_info.is_front_matter || !front_matter_group_plans.is_empty())
                        {
                            if table_info.table_start_line < start {
                                start = table_info.table_start_line;
                                expanded = true;
                            }
                            if table_info.table_end_line > end {
                                end = table_info.table_end_line;
                                expanded = true;
                            }
                        }

                        if let Some(preview) = decoration.block_preview.as_ref() {
                            if preview.start_line < start {
                                start = preview.start_line;
                                expanded = true;
                            }
                            let preview_end = preview.end_line.saturating_add(1);
                            if preview_end > end {
                                end = preview_end;
                                expanded = true;
                            }
                        }
                    }
                }
            }

            (start.min(line_count), end.min(line_count))
        } else {
            (0, line_count)
        };
        let render_start_y = if render_start_line == 0 {
            editor.padding_top() as f64
        } else {
            editor.line_y_offset_precise(render_start_line)
        };
        let mut y = origin_y + (render_start_y - scroll_offset as f64) as f32;
        let mut _last_line_height = 0.0_f32;
        let mut scroll_anchor_delta = 0.0_f32;
        let suppress_scroll_anchor_compensation = editor.editor_scrollbar_drag.is_some()
            || editor.pending_preview_reveal.is_some()
            || editor.document_mode_scroll_anchor_is_current()
            || editor.pending_click_anchor.is_some()
            || editor.pending_center_cursor_in_view
            || editor.center_cursor_confirm_frames > 0
            || frame.explicit_caret_reveal_requested
            || (editor.pending_scroll_to_cursor && editor.pending_typing_scroll_guard.is_none());

        // Collect all cursor selection ranges (primary + extra)
        let mut all_sel_ranges: Vec<std::ops::Range<usize>> =
            Vec::with_capacity(1 + editor.extra_cursors.len());
        let sel_range = cursor.selection_range();
        let _has_selection = cursor.has_selection();
        all_sel_ranges.push(sel_range.clone());
        for ec in &editor.extra_cursors {
            all_sel_ranges.push(ec.selection_range());
        }
        // Collect all cursor offsets for painting cursor bars
        let mut all_cursor_offsets: Vec<usize> = Vec::with_capacity(1 + editor.extra_cursors.len());
        all_cursor_offsets.push(cursor.offset);
        for ec in &editor.extra_cursors {
            all_cursor_offsets.push(ec.offset);
        }
        let syntax_expansion_allowed = editor.markdown_syntax_expansion_allowed();
        let search_reveal_range = editor.current_search_source_range();
        let table_syntax_focus = editor.table_cell_syntax_focus();
        let any_selection = all_sel_ranges.iter().any(|r| r.start != r.end);
        let mut active_line_numbers: HashSet<usize> = HashSet::new();
        for &offset in &all_cursor_offsets {
            active_line_numbers.insert(buffer.offset_to_line_col(offset).0);
        }
        for range in all_sel_ranges
            .iter()
            .filter(|range| range.start != range.end)
        {
            let start_line = buffer.offset_to_line_col(range.start).0;
            let end_offset = range.end.saturating_sub(1).min(buffer.len());
            let end_line = buffer.offset_to_line_col(end_offset).0;
            let visible_start = start_line.max(render_start_line);
            let visible_end = end_line.min(render_end_line.saturating_sub(1));
            for line in visible_start..=visible_end {
                active_line_numbers.insert(line);
            }
        }

        let mut gutter_quads = Vec::new();
        let mut gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla)> = Vec::new();
        let mut inline_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla)> = Vec::new();
        let mut gutter_fold_icons: Vec<(Bounds<Pixels>, &'static str, Hsla)> = Vec::new();
        let mut format_icons: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
        let mut front_matter_type_icons = Vec::new();

        let mut front_matter_type_areas_local = Vec::new();
        let mut front_matter_group_texts = Vec::new();
        let mut front_matter_text_areas = Vec::new();
        let mut front_matter_delete_areas = Vec::new();
        let mut front_matter_list_scroll_areas = Vec::new();
        let mut checkbox_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut image_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut generated_preview_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut generated_preview_image_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut generated_preview_zoom_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut generated_preview_action_areas_local: Vec<(
            Bounds<Pixels>,
            usize,
            GeneratedPreviewAction,
        )> = Vec::new();
        let mut image_preview_zoom_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut display_info_updates: Vec<(usize, Option<LineDisplayInfo>)> =
            Vec::with_capacity(render_end_line.saturating_sub(render_start_line));
        let mut hr_lines: Vec<(f32, f32, f32, bool)> = Vec::new();
        let mut image_previews: Vec<(
            Bounds<Pixels>,
            Arc<RenderImage>,
            Pixels,
            Option<Bounds<Pixels>>,
        )> = Vec::new();
        let mut image_borders: Vec<PaintQuad> = Vec::new();
        let mut table_borders: Vec<PaintQuad> = Vec::new();
        let mut table_border_paths: Vec<(Path<Pixels>, Hsla)> = Vec::new();
        let mut table_bgs: Vec<PaintQuad> = Vec::new();
        let table_icons: Vec<(Bounds<Pixels>, &'static str, Hsla)> = Vec::new();
        let mut diff_line_bgs: Vec<PaintQuad> = Vec::new();
        let mut diff_inline_quads: Vec<PaintQuad> = Vec::new();
        // In-place (working-tree) diff: left-edge indicator bars plus the
        // per-visible-line geometry used to position them and their hit areas.
        let mut inline_diff_gutter_bars: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
        let mut inline_diff_line_geometry: Vec<(usize, f32, f32)> = Vec::new();
        let mut inline_diff_gutter_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut inline_diff_hunk_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut inline_diff_revert_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut inline_diff_revert_buttons: Vec<CodeBlockCopyButtonPaint> = Vec::new();
        // Read-only removed rows shaped for expanded inline-diff hunks.
        let mut phantom_text_lines: Vec<(WrappedLine, Point<Pixels>, Pixels)> = Vec::new();
        // Measured total height of the soft-wrapped removed rows anchored above /
        // below each line, written back so `block_padding_for_line` reserves the
        // right amount on the next frame.
        let mut phantom_above_heights: HashMap<usize, f32> = HashMap::new();
        let mut phantom_below_heights: HashMap<usize, f32> = HashMap::new();
        // Per-removed-row geometry for read-only character-level selection.
        let mut phantom_row_hits_local: Vec<PhantomRowHit> = Vec::new();
        let mut table_cell_texts: Vec<(WrappedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut table_click_areas_local: Vec<(Bounds<Pixels>, usize, usize, usize)> = Vec::new();
        let mut table_col_handle_areas_local: Vec<(Bounds<Pixels>, usize, usize)> = Vec::new();
        let mut table_row_handle_areas_local: Vec<(Bounds<Pixels>, usize, usize)> = Vec::new();
        let mut table_boundary_caret_areas_local: Vec<(Bounds<Pixels>, TableBoundaryCaret)> =
            Vec::new();
        let mut table_layout_cache: HashMap<usize, helpers::TableLayout> = HashMap::new();
        let mut image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)> = Vec::new();
        let mut image_edit_hit_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut gutter_fold_hit_areas_local: Vec<(Bounds<Pixels>, usize, bool)> = Vec::new();
        let mut format_line_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut format_trigger_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut line_format_toolbar_geometries_local = Vec::new();
        let mut code_block_group_bounds: HashMap<usize, CodeBlockGroupBounds> = HashMap::new();
        let mut code_block_horizontal_extents: HashMap<usize, f32> = HashMap::new();
        let mut code_block_horizontal_scrollbars_local: Vec<CodeBlockHorizontalScrollbarGeometry> =
            Vec::new();
        let mut code_block_hover_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut code_block_copy_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut code_block_display_action_areas_local: Vec<(
            Bounds<Pixels>,
            usize,
            CodeBlockDisplayAction,
        )> = Vec::new();
        let mut code_block_copy_buttons: Vec<CodeBlockCopyButtonPaint> = Vec::new();
        let mut image_zoom_button_targets: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut preview_zoom_buttons: Vec<CodeBlockCopyButtonPaint> = Vec::new();
        let mut preview_zoom_hitboxes = Vec::new();
        let mut generated_preview_tooltips: Vec<(Hitbox, Bounds<Pixels>, SharedString)> =
            Vec::new();
        let mut editor_icon_tooltips: Vec<(Bounds<Pixels>, SharedString)> = Vec::new();
        let mut unordered_list_markers: Vec<(WrappedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut inline_overlays: Vec<(ShapedLine, Point<Pixels>, Pixels)> = Vec::new();

        let hidden_lines: HashSet<usize> = {
            let mut set = HashSet::new();
            for &fold_line in &editor.folded_sections {
                if collapsed_front_matter_range
                    .as_ref()
                    .is_some_and(|range| fold_line == range.start - 1)
                {
                    continue;
                }
                if let Some(range) = editor.fold_range_for_line(fold_line) {
                    for l in range {
                        set.insert(l);
                    }
                }
            }

            set
        };

        let text_style = window.text_style();
        let editor_styles = &editor.styles.editor;
        let configured_font_family = |configured: &'static str| {
            if configured == crate::platform::fonts::SYSTEM_UI {
                text_style.font().family.clone()
            } else {
                configured.into()
            }
        };
        let body_cjk_fallbacks = |configured: &'static str| {
            (configured != editor_styles.editor_body_cjk_font_family).then(|| {
                FontFallbacks::from_fonts(vec![
                    editor_styles.editor_body_cjk_font_family.to_string(),
                ])
            })
        };
        let mut monospace_font = text_style.font();
        monospace_font.family = configured_font_family(editor_styles.editor_monospace_font_family);
        monospace_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_monospace_font_family);

        let mut body_font = text_style.font();
        body_font.family = configured_font_family(editor_styles.editor_body_latin_font_family);
        body_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_body_latin_font_family);

        let mut body_cjk_font = text_style.font();
        body_cjk_font.family = configured_font_family(editor_styles.editor_body_cjk_font_family);
        body_cjk_font.fallbacks = None;

        let mut heading_font = text_style.font();
        heading_font.family =
            configured_font_family(editor_styles.editor_heading_latin_font_family);
        heading_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_heading_latin_font_family);

        let mut heading_cjk_font = text_style.font();
        heading_cjk_font.family =
            configured_font_family(editor_styles.editor_heading_cjk_font_family);
        heading_cjk_font.fallbacks = None;

        let mut italic_font = text_style.font();
        italic_font.family = configured_font_family(editor_styles.editor_italic_latin_font_family);
        italic_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_italic_latin_font_family);

        let mut italic_cjk_font = text_style.font();
        italic_cjk_font.family =
            configured_font_family(editor_styles.editor_italic_cjk_font_family);
        italic_cjk_font.fallbacks = Some(FontFallbacks::from_fonts(vec![
            editor_styles.editor_body_cjk_font_family.to_string(),
        ]));

        let base_char_width = {
            let test_run = TextRun {
                len: 1,
                font: monospace_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(test_lines) = window.text_system().shape_text(
                "x".into(),
                px(editor.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                test_lines
                    .first()
                    .map(|line| f32::from(line.width()))
                    .unwrap_or_else(|| editor.base_font_size() * 0.6)
            } else {
                editor.base_font_size() * 0.6
            }
        };

        if buffer.text().is_empty()
            && editor.active_tab_is_markdown()
            && !(editor.embedded_editor && editor.embedded_read_only)
        {
            let placeholder = "键入 / 命令或右键插入格式...";
            let placeholder_runs = inline_overlay_runs(
                placeholder,
                &body_font,
                &body_cjk_font,
                crate::theme::with_alpha(
                    theme.text_secondary,
                    editor.styles.editor.editor_empty_placeholder_opacity,
                ),
            );
            let shaped = window.text_system().shape_line(
                SharedString::from(placeholder),
                px(editor.base_font_size()),
                &placeholder_runs,
                None,
            );
            inline_overlays.push((
                shaped,
                point(px(editor_left + editor.padding_left()), px(y)),
                px(editor.visual_height_for_line(0)),
            ));
        }
        let body_space_width = {
            let test_run = TextRun {
                len: 1,
                font: body_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(test_lines) = window.text_system().shape_text(
                " ".into(),
                px(editor.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                test_lines
                    .first()
                    .map(|line| f32::from(line.width()).max(1.0))
                    .unwrap_or_else(|| base_char_width.max(1.0))
            } else {
                base_char_width.max(1.0)
            }
        };
        let show_non_markdown_indent_guides = editor.active_tab_is_non_markdown_text();
        let indent_guide_step_columns = if show_non_markdown_indent_guides {
            editor.cached_non_markdown_indent_guide_step()
        } else {
            0
        };
        let indent_guide_color = editor.styles.editor.editor_indent_guide_color;
        let indent_guide_width = editor.scaled(editor.styles.editor.editor_indent_guide_width_px);
        let indent_guide_dash_len =
            editor.scaled(editor.styles.editor.editor_indent_guide_dash_length_px);
        let indent_guide_gap_len =
            editor.scaled(editor.styles.editor.editor_indent_guide_gap_length_px);
        let non_markdown_cached_wrap_width = if editor.active_tab_is_non_markdown_text()
            && editor.editor_wraps_to_available_width()
        {
            let letter_spacing = editor_styles.editor_letter_spacing_px;
            Some(if letter_spacing > 0.0 {
                wrap_width * editor.base_font_size() / (editor.base_font_size() + letter_spacing)
            } else {
                wrap_width
            })
        } else {
            None
        };
        let defer_offscreen_wrap_reflow = editor.active_tab_is_non_markdown_text()
            && editor.editor_wraps_to_available_width()
            && wrap_width_changed;
        let mut deferred_offscreen_wrap_lines = false;
        let mut remaining_offscreen_wrap_reflows = if editor.active_tab_is_non_markdown_text()
            && editor.editor_wraps_to_available_width()
            && !wrap_width_changed
        {
            frame.offscreen_wrap_reflows
        } else {
            0
        };
        let initial_offscreen_wrap_reflows = remaining_offscreen_wrap_reflows;

        let image_cache = &editor.image_cache;
        let window_scale_factor = window.scale_factor().max(1.0);
        let table_cell_padding_px = editor.scaled(editor.styles.table.table_cell_padding_px);
        let table_cell_padding_chars = if table_cell_padding_px <= 0.0 {
            0
        } else {
            (table_cell_padding_px / base_char_width).ceil().max(1.0) as usize
        };
        let checkbox_size_px =
            editor.base_font_size() * editor.styles.checkbox.checkbox_size_factor;
        let checkbox_text_gap_px = editor.scaled(editor.styles.checkbox.checkbox_text_gap_px);
        let task_source_indent_step_chars =
            editor.styles.checkbox.checkbox_parent_prefix_chars as usize;
        let collapsed_task_prefix_display_columns = helpers::task_prefix_display_columns(
            body_space_width,
            checkbox_size_px,
            checkbox_text_gap_px,
        );
        let checkbox_indent_step_display_columns = collapsed_task_prefix_display_columns;

        let is_code_block_type = |i: usize| {
            matches!(
                decorations.get(i).map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            )
        };

        let code_block_group_range = |line_idx: usize| {
            if !is_code_block_type(line_idx) {
                return None;
            }

            let mut start = line_idx;
            while start > 0 && is_code_block_type(start - 1) {
                start -= 1;
            }

            let mut end = line_idx;
            while end + 1 < line_count && is_code_block_type(end + 1) {
                end += 1;
            }

            Some((start, end))
        };

        let active_code_fence_group = if syntax_expansion_allowed
            && cursor_line < line_count
            && matches!(
                decorations.get(cursor_line).map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            ) {
            code_block_group_range(cursor_line)
        } else {
            None
        };
        let active_front_matter_group = syntax_expansion_allowed
            .then(|| decorations.get(cursor_line))
            .flatten()
            .and_then(|decoration| decoration.table_info.as_ref())
            .filter(|table_info| table_info.is_front_matter)
            .map(|table_info| {
                (
                    table_info.table_start_line,
                    table_info.table_end_line.saturating_sub(1),
                )
            });
        let active_markdown_indent_guide_step_columns = active_code_fence_group
            .map(|(start, end)| detect_indent_guide_step_in_range(buffer, start, end + 1))
            .or_else(|| {
                active_front_matter_group
                    .map(|(start, end)| detect_indent_guide_step_in_range(buffer, start, end + 1))
            })
            .unwrap_or(0);

        let selection_match_ranges = editor.selection_match_ranges();
        let bracket_match_ranges = editor.bracket_match_ranges();

        for line_idx in render_start_line..render_end_line {
            // Fold geometry already measures collapsed interior rows as zero, including
            // rows outside the shaping cache. The summary row and delimiters stay painted.
            if editor.front_matter_line_is_hidden(line_idx) {
                continue;
            }
            let group_plan = front_matter_group_plans
                .iter()
                .find(|plan| plan.group.source_lines.contains(&line_idx));
            let is_cursor_table_anchor = cursor_table_reveal_range
                .as_ref()
                .is_some_and(|table| table.start == line_idx);
            let is_cursor_line_early = line_idx == cursor_line || is_cursor_table_anchor;
            let line_has_block_preview = decorations
                .get(line_idx)
                .and_then(|decoration| decoration.block_preview.as_ref())
                .is_some();
            let cached_line = editor
                .line_display_info
                .get(line_idx)
                .and_then(|info| info.as_ref());
            let mut cached_height_before_reflow = None;
            let mut cached_line_above_viewport = false;
            if line_has_block_preview {
                let previous_height = editor.visual_height_for_line(line_idx);
                cached_height_before_reflow = Some(previous_height);
                cached_line_above_viewport = editor.line_y_offset_precise(line_idx)
                    + previous_height as f64
                    <= scroll_offset as f64;
            }
            let can_reuse_non_markdown_cached_line = editor.active_tab_is_non_markdown_text()
                && !is_cursor_table_anchor
                && !line_has_block_preview
                && !editor.inline_diff_is_phantom_anchor(line_idx)
                && cached_line.is_some_and(|cached| {
                    non_markdown_cached_line_can_reuse(cached, non_markdown_cached_wrap_width)
                });
            if !is_cursor_line_early && !line_has_block_preview && !hidden_lines.contains(&line_idx)
            {
                if let Some(cached) = cached_line {
                    let cached_h = cached.total_height();
                    let est_top = y - scroll_anchor_delta;
                    let est_bottom = est_top + cached_h;
                    cached_height_before_reflow = Some(cached_h);
                    cached_line_above_viewport = est_bottom < origin_y;
                    let is_offscreen =
                        est_bottom < origin_y || est_top > origin_y + viewport_height;
                    let is_stale_offscreen_non_markdown_line = editor
                        .active_tab_is_non_markdown_text()
                        && editor.editor_wraps_to_available_width()
                        && is_offscreen
                        && !can_reuse_non_markdown_cached_line;
                    let should_defer_stale_offscreen_non_markdown_line =
                        is_stale_offscreen_non_markdown_line
                            && (defer_offscreen_wrap_reflow
                                || remaining_offscreen_wrap_reflows == 0);
                    let can_skip_offscreen = if editor.active_tab_is_non_markdown_text() {
                        can_reuse_non_markdown_cached_line
                            || should_defer_stale_offscreen_non_markdown_line
                    } else if wrap_width_changed {
                        // Markdown resize: the wrap width changed, so every cached
                        // line's shaping is stale. Re-shaping the entire document on
                        // each resize frame costs hundreds of milliseconds on large
                        // files, so instead reuse the stale cached height for
                        // offscreen lines (only the visible window re-shapes at the
                        // new width). Offscreen lines converge to the new width as
                        // they scroll back into view — the same transient the
                        // non-markdown deferred-reflow path accepts.
                        true
                    } else {
                        has_valid_cache
                    };

                    if is_offscreen && can_skip_offscreen {
                        if should_defer_stale_offscreen_non_markdown_line {
                            deferred_offscreen_wrap_lines = true;
                        }
                        // The cached display info for this offscreen line is already
                        // stored in `editor.line_display_info[line_idx]` and is being
                        // left unchanged, so there is no need to clone it into
                        // `display_info_updates` (which would just write the identical
                        // value back). On very large documents the window can cover
                        // tens of thousands of skipped lines per frame, and cloning a
                        // `LineDisplayInfo` (String + several Vecs + HashMap +
                        // WrappedLine) for each was the dominant prepaint cost.
                        y += cached_h;
                        continue;
                    }

                    if is_stale_offscreen_non_markdown_line
                        && !defer_offscreen_wrap_reflow
                        && remaining_offscreen_wrap_reflows > 0
                    {
                        remaining_offscreen_wrap_reflows -= 1;
                    }
                }
            }

            let decoration = decorations.get(line_idx).cloned().unwrap_or_default();
            let mut decoration = decoration;
            // Overlay in-place working-tree diff highlights onto the live buffer
            // line. Removed spans stay empty (removed content is shown as phantom
            // rows when a hunk is expanded), only added/changed chars highlight.
            if let Some(added_spans) = editor.inline_diff_added_spans_for_line(line_idx) {
                decoration.diff_added_spans = added_spans;
            }
            let block_type = &decoration.block_type;
            let font_size = editor.font_size_for_block(block_type);
            let line_height = editor.line_height_for_block(block_type);
            let (block_padding_top, block_padding_bottom) = editor.block_padding_for_line(line_idx);

            if hidden_lines.contains(&line_idx)
                || group_plan.is_some_and(|plan| plan.hides_line(line_idx))
            {
                let line_content = buffer.line_content(line_idx);
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|ls| ls.into_iter().next());
                display_info_updates.push((
                    line_idx,
                    wrapped_line.map(|wl| {
                        let unwrapped_width_px = f32::from(wl.unwrapped_layout.width);
                        LineDisplayInfo {
                            display_text: String::new(),
                            display_to_buffer: Vec::new(),
                            buffer_to_display: vec![0; line_content.len()],
                            base_runs: Vec::new(),
                            font_size,
                            line_height: 0.0,
                            visual_line_count: 0,
                            wrapped_line: wl,
                            wrap_width_px: None,
                            unwrapped_width_px,
                            wrap_boundary_upstream_xs: HashMap::new(),
                            extra_height: 0.0,
                            indent_px: 0.0,
                            content_x: editor_left + editor.padding_left(),
                            horizontal_clip_right_x: None,
                            selection_padding_left_px: 0.0,
                        }
                    }),
                ));
                continue;
            }
            let is_cursor_line = line_idx == cursor_line;
            let estimated_total_line_height =
                line_height + block_padding_top + block_padding_bottom;
            let estimated_line_top = y - scroll_anchor_delta;
            let should_defer_uncached_offscreen_non_markdown_line = editor
                .active_tab_is_non_markdown_text()
                && !is_cursor_line
                && !line_has_block_preview
                && cached_line.is_none()
                && estimated_line_top > origin_y + viewport_height
                && (!editor.editor_wraps_to_available_width()
                    || defer_offscreen_wrap_reflow
                    || remaining_offscreen_wrap_reflows == 0);

            if should_defer_uncached_offscreen_non_markdown_line {
                if editor.editor_wraps_to_available_width() {
                    deferred_offscreen_wrap_lines = true;
                }
                display_info_updates.push((line_idx, None));
                y += estimated_total_line_height;
                continue;
            }

            let line_content = buffer.line_content(line_idx);
            let line_start_offset = buffer.line_start(line_idx);
            let line_end_offset = buffer.line_end(line_idx);
            let cursor_in_table = decoration
                .table_info
                .as_ref()
                .is_some_and(|table| table_group_expands_neighboring_rows(table, cursor_line));
            let heading_content_start = matches!(decoration.block_type, BlockType::Heading(_))
                .then(|| {
                    editor
                        .line_content_start_col(line_idx)
                        .min(line_content.len())
                });
            let ime_active_on_line = editor.marked_range.as_ref().is_some_and(|marked| {
                marked.start < line_end_offset && marked.end > line_start_offset
            });
            let cursor_col = if editor.cursor.has_selection()
                && matches!(decoration.block_type, BlockType::Heading(_))
            {
                // Only expand heading syntax if the cursor itself is on this heading line,
                // not when selection merely passes through.
                if is_cursor_line {
                    Some(cursor.offset.saturating_sub(line_start_offset))
                } else {
                    None
                }
            } else if editor.is_selecting && editor.cursor.has_selection() {
                let sel = editor.cursor.selection_range();
                if sel.start >= line_start_offset && sel.end <= line_end_offset {
                    let sel_start_col = sel.start - line_start_offset;
                    let sel_end_col = sel.end - line_start_offset;
                    let within_syntax = decoration.inline_spans.iter().any(|span| {
                        let prefix_len = match span.kind {
                            crate::markdown::InlineKind::Bold => 2,
                            crate::markdown::InlineKind::Italic => 1,
                            crate::markdown::InlineKind::Strikethrough => 2,
                            crate::markdown::InlineKind::Underline => 2,
                            crate::markdown::InlineKind::InlineCode => 1,
                            crate::markdown::InlineKind::Link
                            | crate::markdown::InlineKind::Image => {
                                if span.kind == crate::markdown::InlineKind::Image {
                                    2
                                } else {
                                    1
                                }
                            }
                        };
                        let full_start = span.range.start.saturating_sub(prefix_len);
                        let full_end = decoration
                            .syntax_ranges
                            .iter()
                            .filter(|sr| sr.range.end > span.range.end)
                            .map(|sr| sr.range.end)
                            .max()
                            .unwrap_or(span.range.end);
                        sel_start_col >= full_start && sel_end_col <= full_end
                    });
                    if within_syntax {
                        Some(cursor.offset.saturating_sub(line_start_offset))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if ime_active_on_line {
                if let Some(content_start) = heading_content_start {
                    if is_cursor_line {
                        Some(
                            cursor
                                .offset
                                .saturating_sub(line_start_offset)
                                .max(content_start),
                        )
                    } else {
                        None
                    }
                } else {
                    Some(line_content.len())
                }
            } else if is_cursor_line {
                Some(cursor.offset.saturating_sub(line_start_offset))
            } else if cursor_in_table {
                Some(line_content.len())
            } else {
                None
            };
            let mut active_ranges: Vec<std::ops::Range<usize>> =
                if editor.is_selecting && editor.selection_drag_has_moved {
                    Vec::new()
                } else {
                    all_sel_ranges
                        .iter()
                        .filter(|range| range.start != range.end)
                        .filter_map(|range| {
                            if range.end <= line_start_offset || range.start >= line_end_offset {
                                return None;
                            }
                            let start = range
                                .start
                                .saturating_sub(line_start_offset)
                                .min(line_content.len());
                            let end = range
                                .end
                                .saturating_sub(line_start_offset)
                                .min(line_content.len());
                            Some(start..end)
                        })
                        .collect()
                };
            if editor.is_selecting
                && editor.selection_drag_has_moved
                && editor.cursor.has_selection()
            {
                let (anchor_line, anchor_col) = buffer.offset_to_line_col(editor.cursor.anchor);
                let (head_line, head_col) = buffer.offset_to_line_col(editor.cursor.offset);
                if anchor_line == line_idx && head_line == line_idx {
                    for syntax_range in &decoration.syntax_ranges {
                        let anchor_inside = anchor_col >= syntax_range.trigger_range.start
                            && anchor_col <= syntax_range.trigger_range.end;
                        let head_inside = head_col >= syntax_range.trigger_range.start
                            && head_col <= syntax_range.trigger_range.end;
                        if anchor_inside && head_inside {
                            active_ranges.push(syntax_range.trigger_range.clone());
                            break;
                        }
                    }
                }
            }
            for marked in editor.marked_ranges() {
                if heading_content_start.is_some() && ime_active_on_line {
                    continue;
                }
                if marked.end <= line_start_offset || marked.start >= line_end_offset {
                    continue;
                }
                let mut start = marked
                    .start
                    .saturating_sub(line_start_offset)
                    .min(line_content.len());
                let end = marked
                    .end
                    .saturating_sub(line_start_offset)
                    .min(line_content.len());
                if let Some(content_start) = heading_content_start {
                    if end <= content_start {
                        continue;
                    }
                    start = start.max(content_start);
                }
                active_ranges.push(start..end);
            }
            // Read-only views remain selectable, but should keep Markdown
            // markers folded while a selection changes so wrapping and height
            // do not shift underneath the pointer.
            if !syntax_expansion_allowed {
                active_ranges.clear();
            }
            if syntax_expansion_allowed
                && (!editor.is_selecting || !editor.selection_drag_has_moved)
                && active_ranges.is_empty()
                && !editor.suppress_fold_for_empty_selection_drag
                && let Some(col) = cursor_col
            {
                let clamped = col.min(line_content.len());
                active_ranges.push(clamped..clamped);
            }

            if group_plan.is_some_and(|plan| plan.active) {
                active_ranges.push(0..line_content.len());
            }
            editor.append_search_reveal_range(line_idx, &mut active_ranges);
            helpers::expand_source_syntax(
                editor.source_mode,
                line_content.len(),
                &mut active_ranges,
            );

            // Hide all non-first-row table lines (rendered as block on the first row)
            let hide_table_line = matches!(block_type, BlockType::Table)
                && decoration.table_info.as_ref().is_some_and(|ti| {
                    // Hide separator rows and all data rows.
                    // Only the first row (header) is kept as anchor for real tables.
                    !ti.is_front_matter && (ti.is_separator || !ti.is_header)
                });
            if hide_table_line {
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next());
                display_info_updates.push((
                    line_idx,
                    wrapped_line.map(|wrapped_line| {
                        let unwrapped_width_px = f32::from(wrapped_line.unwrapped_layout.width);
                        LineDisplayInfo {
                            display_text: String::new(),
                            display_to_buffer: Vec::new(),
                            buffer_to_display: vec![0; line_content.len()],
                            base_runs: Vec::new(),
                            font_size,
                            line_height: 0.0,
                            visual_line_count: 0,
                            wrapped_line,
                            wrap_width_px: None,
                            unwrapped_width_px,
                            wrap_boundary_upstream_xs: HashMap::new(),
                            extra_height: 0.0,
                            indent_px: 0.0,
                            content_x: editor_left + editor.padding_left(),
                            horizontal_clip_right_x: None,
                            selection_padding_left_px: 0.0,
                        }
                    }),
                ));
                continue;
            }

            let is_front_matter_line = decoration
                .table_info
                .as_ref()
                .is_some_and(|ti| ti.is_front_matter);

            let (front_matter_content_left, front_matter_wrap_width) =
                front_matter_content_geometry(
                    is_front_matter_line
                        && (front_matter_summary_row == Some(line_idx)
                            || !decoration
                                .table_info
                                .as_ref()
                                .is_some_and(|table| table.is_separator)),
                    editor_left + editor.padding_left(),
                    wrap_width,
                    editor.scaled(1.0),
                );

            let table_width_chars = if *block_type == BlockType::Table && !is_front_matter_line {
                Some((text_width / base_char_width) as usize)
            } else {
                None
            };

            if editor.active_tab_is_non_markdown_text()
                && !is_cursor_line_early
                && can_reuse_non_markdown_cached_line
                && let Some(cached) = cached_line
            {
                let font_size = cached.font_size;
                let line_height = cached.line_height;
                let visual_line_count = cached.visual_line_count;
                let text_line_height = cached.text_height();
                let total_line_height = cached.total_height();
                let line_top = y - scroll_anchor_delta;
                let line_bottom = line_top + total_line_height;
                let text_line_top = line_top;
                let content_x =
                    editor_left + editor.padding_left() - non_markdown_horizontal_offset;
                if non_markdown_nowrap {
                    non_markdown_horizontal_extent =
                        non_markdown_horizontal_extent.max(non_markdown_horizontal_max_offset(
                            cached.unwrapped_width_px,
                            text_width,
                            editor.scaled(editor.styles.editor.editor_wrap_right_padding_px),
                            editor.styles.cursor.cursor_width_px.max(1.0),
                        ));
                }

                display_info_updates.push((
                    line_idx,
                    Some(LineDisplayInfo {
                        display_text: cached.display_text.clone(),
                        display_to_buffer: cached.display_to_buffer.clone(),
                        buffer_to_display: cached.buffer_to_display.clone(),
                        base_runs: cached.base_runs.clone(),
                        font_size,
                        line_height,
                        visual_line_count,
                        wrapped_line: cached.wrapped_line.clone(),
                        wrap_width_px: cached.wrap_width_px,
                        unwrapped_width_px: cached.unwrapped_width_px,
                        wrap_boundary_upstream_xs: cached.wrap_boundary_upstream_xs.clone(),
                        extra_height: cached.extra_height,
                        indent_px: cached.indent_px,
                        content_x,
                        horizontal_clip_right_x: if non_markdown_nowrap {
                            Some(non_markdown_content_left + text_width)
                        } else {
                            cached.horizontal_clip_right_x
                        },
                        selection_padding_left_px: cached.selection_padding_left_px,
                    }),
                ));

                if (line_bottom < origin_y || line_top > origin_y + viewport_height)
                    && !is_cursor_line_early
                {
                    y += total_line_height;
                    _last_line_height = total_line_height;
                    continue;
                }
                inline_diff_line_geometry.push((line_idx, line_top, total_line_height));

                let mut runs = cached.base_runs.clone();
                helpers::apply_search_match_text_styles(
                    &mut runs,
                    &cached.display_text,
                    &cached.display_to_buffer,
                    line_start_offset,
                    &editor.search_matches,
                    editor.search_current_index,
                    editor.styles.search.search_match_text_color,
                    editor.styles.search.search_match_active_text_color,
                );
                if editor.current_file_path == editor.workspace_search_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &cached.display_text,
                        &cached.display_to_buffer,
                        line_start_offset,
                        &editor.workspace_search_highlight_ranges,
                        editor.workspace_search_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                if editor.current_file_path == editor.outline_flash_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &cached.display_text,
                        &cached.display_to_buffer,
                        line_start_offset,
                        &editor.outline_flash_highlight_ranges,
                        editor.outline_flash_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                let origin = point(px(content_x), px(text_line_top));
                if show_non_markdown_indent_guides {
                    push_indent_guide_quads(
                        &mut indent_guide_quads,
                        line_content,
                        content_x,
                        line_top,
                        total_line_height,
                        base_char_width,
                        indent_guide_step_columns,
                        indent_guide_width,
                        indent_guide_dash_len,
                        indent_guide_gap_len,
                        indent_guide_color,
                    );
                }
                let is_active_line_number = active_line_numbers.contains(&line_idx);
                let skip_block_line_highlight = decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|ti| !ti.is_front_matter && line_idx == ti.table_start_line);
                if editor.app_settings.show_whitespace {
                    push_whitespace_dot_quads(
                        &mut whitespace_dots,
                        &cached.display_text,
                        &cached.wrapped_line,
                        origin,
                        line_height,
                        editor.scaled(editor.styles.editor.editor_whitespace_dot_size_px),
                        editor.styles.editor.editor_whitespace_dot_color,
                        0,
                    );
                }

                let gutter_styles = &editor.styles.gutter;
                push_gutter_line_number_label(
                    &mut gutter_labels,
                    window,
                    line_idx + 1,
                    is_active_line_number,
                    &monospace_font,
                    editor.scaled(gutter_styles.gutter_line_number_font_size_px),
                    gutter_right_x,
                    text_line_top,
                    line_height,
                    editor.scaled(gutter_styles.gutter_line_number_vertical_offset_px),
                    editor.scaled(gutter_styles.gutter_line_number_right_gap_px),
                    gutter_styles.gutter_line_number_text_color,
                    gutter_styles.gutter_line_number_active_text_color,
                );

                let fold_range = if hidden_lines.contains(&line_idx) {
                    None
                } else {
                    editor.fold_range_for_line(line_idx)
                };
                if fold_range.is_some() {
                    let fi_size = editor.scaled(gutter_styles.gutter_fold_icon_size_px);
                    let fi_hit_padding = editor.scaled(gutter_styles.gutter_fold_hit_padding_px);
                    let fi_x = editor.gutter_icon_x(editor_left, fi_size);
                    let fi_y = text_line_top
                        + (f32::from(cached.wrapped_line.row_height(0, px(line_height))) - fi_size)
                            / 2.0;
                    let reveal = editor.gutter_fold_reveal.max(0.0);
                    let is_folded = editor.folded_sections.contains(&line_idx);
                    let is_hovered = editor.hovering_fold_line == Some(line_idx)
                        || gutter_fold_row_hovered(
                            editor.hovering_gutter_fold_area,
                            window.mouse_position().y,
                            line_top,
                            total_line_height,
                        );
                    let fi_alpha = if is_folded {
                        gutter_styles.gutter_fold_folded_opacity
                    } else if is_hovered {
                        gutter_styles.gutter_fold_hover_opacity * reveal
                    } else {
                        0.0
                    };
                    if fi_alpha > 0.0 {
                        let icon_bounds =
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size)));
                        let mut fi_color = if is_folded {
                            gutter_styles.gutter_fold_folded_color
                        } else {
                            gutter_styles.gutter_fold_hover_color
                        };
                        fi_color.a = fi_alpha;
                        gutter_fold_icons.push((
                            icon_bounds,
                            if is_folded {
                                FOLD_SVG_PATH
                            } else {
                                FOLD_EXPANDED_SVG_PATH
                            },
                            fi_color,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        Bounds::new(
                            point(px(gutter_icon_column_left - fi_hit_padding), px(line_top)),
                            size(
                                px(gutter_icon_column_width + fi_hit_padding * 2.0),
                                px(total_line_height),
                            ),
                        ),
                        line_idx,
                        false,
                    ));
                }

                if let Some(diff_bg) = diff_line_background(editor, line_idx) {
                    diff_line_bgs.push(fill(
                        Bounds::new(
                            point(px(origin_x), px(line_top)),
                            size(px(viewport_width), px(total_line_height)),
                        ),
                        diff_bg,
                    ));
                }

                let selection_styles = &editor.styles.selection;
                push_inline_code_background_quads(
                    &mut diff_inline_quads,
                    &decoration,
                    &cached.display_text,
                    line_content.len(),
                    &cached.buffer_to_display,
                    &runs,
                    &cached.wrapped_line,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    cached.indent_px,
                    editor.styles.editor.editor_inline_code_bg,
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_x_padding_px)),
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_radius_px)),
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_y_inset_px)),
                    editor_styles.editor_letter_spacing_px,
                    window,
                );
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.diff_removed_spans,
                    &cached.display_text,
                    line_content.len(),
                    &cached.buffer_to_display,
                    &runs,
                    &cached.wrapped_line,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    cached.indent_px,
                    selection_styles.selection_diff_removed_inline_bg,
                    selection_styles.selection_diff_removed_inline_border_color,
                    editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                    px(0.0),
                    px(0.0),
                    px(0.0),
                    editor_styles.editor_letter_spacing_px,
                    window,
                );
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.diff_added_spans,
                    &cached.display_text,
                    line_content.len(),
                    &cached.buffer_to_display,
                    &runs,
                    &cached.wrapped_line,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    cached.indent_px,
                    selection_styles.selection_diff_added_inline_bg,
                    selection_styles.selection_diff_added_inline_border_color,
                    editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                    px(0.0),
                    px(0.0),
                    px(0.0),
                    editor_styles.editor_letter_spacing_px,
                    window,
                );

                if editor.selected_handle.is_none()
                    && is_cursor_line
                    && !any_selection
                    && editor.extra_cursors.is_empty()
                    && !skip_block_line_highlight
                    && editor.app_settings.highlight_line_background
                    && !editor.embedded_editor
                {
                    let highlight_vertical_inset = if matches!(block_type, BlockType::Heading(_)) {
                        f32::from(cursor_aligned_vertical_inset(editor, line_height))
                    } else {
                        0.0
                    };
                    let highlight_top = if matches!(block_type, BlockType::Heading(_)) {
                        text_line_top + highlight_vertical_inset
                    } else {
                        line_top
                    };
                    let highlight_height = if matches!(block_type, BlockType::Heading(_)) {
                        (line_height - highlight_vertical_inset * 2.0).max(0.0)
                    } else {
                        total_line_height
                    };
                    line_highlight = Some(fill(
                        Bounds::new(
                            point(px(origin_x), px(highlight_top)),
                            size(px(viewport_width), px(highlight_height)),
                        ),
                        theme.line_highlight,
                    ));
                }

                if decoration.table_info.is_none() {
                    for sr in &all_sel_ranges {
                        if sr.start == sr.end {
                            continue;
                        }
                        let sel_start_on_line = sr.start.max(line_start_offset);
                        let sel_end_on_line = sr.end.min(line_end_offset);

                        if sel_start_on_line < sel_end_on_line {
                            let buf_start_col = sel_start_on_line - line_start_offset;
                            let buf_end_col = sel_end_on_line - line_start_offset;
                            let disp_start = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_start_col,
                            );
                            let disp_end = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_end_col,
                            );
                            let selection_spans_line =
                                sr.start < line_start_offset || sr.end > line_end_offset;
                            let selection_leading_padding_left_px =
                                if matches!(
                                    block_type,
                                    BlockType::CodeBlock | BlockType::CodeBlockFence
                                ) || decoration.block_preview.is_some()
                                {
                                    0.0
                                } else {
                                    cached.selection_padding_left_px
                                };
                            if matches!(block_type, BlockType::Heading(_)) && selection_spans_line {
                                push_cursor_aligned_highlight_quads(
                                    &mut selection_quads,
                                    editor,
                                    &cached.display_text,
                                    &runs,
                                    &cached.wrapped_line,
                                    disp_start,
                                    disp_end,
                                    content_x,
                                    text_line_top,
                                    line_height,
                                    font_size,
                                    cached.indent_px,
                                    selection_leading_padding_left_px,
                                    theme.selection,
                                    editor_styles.editor_letter_spacing_px,
                                    window,
                                );
                            } else {
                                selection_quads.extend(helpers::build_selection_quads(
                                    &cached.display_text,
                                    &runs,
                                    &cached.wrapped_line,
                                    disp_start,
                                    disp_end,
                                    content_x,
                                    text_line_top,
                                    line_height,
                                    font_size,
                                    cached.indent_px,
                                    selection_leading_padding_left_px,
                                    theme.selection,
                                    editor_styles.editor_letter_spacing_px,
                                    window,
                                ));
                            }
                        }
                    }
                }

                if !editor.search_matches.is_empty() {
                    let match_bg = editor.styles.search.search_match_bg;
                    let active_bg = editor.styles.search.search_match_active_bg;
                    let border_color = editor.styles.search.search_match_border_color;
                    let border_width =
                        editor.scaled(editor.styles.search.search_match_border_width_px);
                    let search_line_top = text_line_top + editor.block_padding_for_line(line_idx).0;
                    let (match_base, line_matches) = helpers::matches_overlapping_range(
                        &editor.search_matches,
                        line_start_offset,
                        line_end_offset,
                    );
                    for (match_offset, sr) in line_matches.iter().enumerate() {
                        let is_active =
                            editor.search_current_index == Some(match_base + match_offset);
                        let sel_start_on_line = sr.start.max(line_start_offset);
                        let sel_end_on_line = sr.end.min(line_end_offset);
                        if sel_start_on_line < sel_end_on_line {
                            let buf_start_col = sel_start_on_line - line_start_offset;
                            let buf_end_col = sel_end_on_line - line_start_offset;
                            push_diff_inline_highlight_quads(
                                &mut search_match_quads,
                                &[buf_start_col..buf_end_col],
                                &cached.display_text,
                                line_content.len(),
                                &cached.buffer_to_display,
                                &runs,
                                &cached.wrapped_line,
                                content_x,
                                search_line_top,
                                line_height,
                                font_size,
                                cached.indent_px,
                                if is_active { active_bg } else { match_bg },
                                border_color,
                                border_width,
                                px(0.0),
                                px(3.0),
                                px((line_height - font_size).max(0.0) * 0.25),
                                editor_styles.editor_letter_spacing_px,
                                window,
                            );
                        }
                    }
                }

                if editor.current_file_path == editor.workspace_search_highlight_path {
                    let match_bg = editor.styles.search.search_match_bg;
                    let active_bg = editor.styles.search.search_match_active_bg;
                    for (match_idx, sr) in
                        editor.workspace_search_highlight_ranges.iter().enumerate()
                    {
                        let is_active =
                            editor.workspace_search_highlight_current_index == Some(match_idx);
                        let sel_start_on_line = sr.start.max(line_start_offset);
                        let sel_end_on_line = sr.end.min(line_end_offset);
                        if sel_start_on_line < sel_end_on_line {
                            let buf_start_col = sel_start_on_line - line_start_offset;
                            let buf_end_col = sel_end_on_line - line_start_offset;
                            let disp_start = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_start_col,
                            );
                            let disp_end = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_end_col,
                            );
                            push_cursor_aligned_highlight_quads(
                                &mut search_match_quads,
                                editor,
                                &cached.display_text,
                                &runs,
                                &cached.wrapped_line,
                                disp_start,
                                disp_end,
                                content_x,
                                line_top,
                                line_height,
                                font_size,
                                cached.indent_px,
                                0.0,
                                if is_active { active_bg } else { match_bg },
                                editor_styles.editor_letter_spacing_px,
                                window,
                            );
                        }
                    }
                }
                if editor.current_file_path == editor.outline_flash_highlight_path {
                    let match_bg = editor.styles.search.search_match_bg;
                    let active_bg = editor.styles.search.search_match_active_bg;
                    for (match_idx, sr) in editor.outline_flash_highlight_ranges.iter().enumerate()
                    {
                        let is_active =
                            editor.outline_flash_highlight_current_index == Some(match_idx);
                        let sel_start_on_line = sr.start.max(line_start_offset);
                        let sel_end_on_line = sr.end.min(line_end_offset);
                        if sel_start_on_line < sel_end_on_line {
                            let buf_start_col = sel_start_on_line - line_start_offset;
                            let buf_end_col = sel_end_on_line - line_start_offset;
                            let disp_start = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_start_col,
                            );
                            let disp_end = helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_end_col,
                            );
                            push_cursor_aligned_highlight_quads(
                                &mut search_match_quads,
                                editor,
                                &cached.display_text,
                                &runs,
                                &cached.wrapped_line,
                                disp_start,
                                disp_end,
                                content_x,
                                text_line_top,
                                line_height,
                                font_size,
                                cached.indent_px,
                                0.0,
                                if is_active { active_bg } else { match_bg },
                                editor_styles.editor_letter_spacing_px,
                                window,
                            );
                        }
                    }
                }

                if !bracket_match_ranges.is_empty() {
                    let mut line_spans: Vec<std::ops::Range<usize>> = Vec::new();
                    for range in &bracket_match_ranges {
                        if range.end > line_start_offset && range.start < line_end_offset {
                            let start = range.start.max(line_start_offset) - line_start_offset;
                            let end = range.end.min(line_end_offset) - line_start_offset;
                            if start < end {
                                line_spans.push(start..end);
                            }
                        }
                    }
                    if !line_spans.is_empty() {
                        push_diff_inline_highlight_quads(
                            &mut diff_inline_quads,
                            &line_spans,
                            &cached.display_text,
                            line_content.len(),
                            &cached.buffer_to_display,
                            &runs,
                            &cached.wrapped_line,
                            content_x,
                            text_line_top,
                            line_height,
                            font_size,
                            cached.indent_px,
                            gpui::transparent_black(),
                            editor.styles.editor.editor_bracket_match_border,
                            editor.scaled(editor.styles.editor.editor_match_border_width_px),
                            px(0.0),
                            px(0.0),
                            px(0.0),
                            editor_styles.editor_letter_spacing_px,
                            window,
                        );
                    }
                }

                if editor.selected_handle.is_none()
                    && editor.phantom_selection.is_none()
                    && window.is_window_active()
                    && editor.focus_handle.is_focused(window)
                    && editor.focused_sidebar_path.is_none()
                {
                    for &cur_offset in &all_cursor_offsets {
                        if cur_offset < line_start_offset || cur_offset > line_end_offset {
                            continue;
                        }
                        let buf_cursor_col = cur_offset.saturating_sub(line_start_offset);
                        let buf_cursor_col = buf_cursor_col.min(line_content.len());
                        let disp_cursor_col = if buf_cursor_col >= line_content.len() {
                            cached.display_text.len()
                        } else {
                            helpers::buffer_to_display_offset(
                                &cached.buffer_to_display,
                                buf_cursor_col,
                            )
                        };

                        let cursor_pos = cached
                            .wrapped_line
                            .position_for_index(disp_cursor_col, px(line_height))
                            .unwrap_or_default();
                        let cursor_pos = match editor.wrap_boundary_affinity {
                            Some(WrapBoundaryAffinity {
                                offset: affinity_offset,
                                kind: WrapBoundaryAffinityKind::Upstream,
                            }) if affinity_offset == cur_offset => upstream_wrap_boundary_position(
                                &cached.wrapped_line,
                                &cached.display_text,
                                disp_cursor_col,
                                &cached.wrap_boundary_upstream_xs,
                                line_height,
                            )
                            .unwrap_or(cursor_pos),
                            Some(WrapBoundaryAffinity {
                                offset: affinity_offset,
                                kind: WrapBoundaryAffinityKind::Downstream,
                            }) if affinity_offset == cur_offset => {
                                downstream_wrap_boundary_position(
                                    &cached.wrapped_line,
                                    &cached.display_text,
                                    disp_cursor_col,
                                    line_height,
                                )
                                .unwrap_or(cursor_pos)
                            }
                            _ => cursor_pos,
                        };
                        let cursor_x = super::caret::text_caret_x(
                            &cached.display_text,
                            disp_cursor_col,
                            cursor_pos,
                            &cached.wrapped_line.unwrapped_layout,
                            window.text_system(),
                            |index| {
                                cached
                                    .wrapped_line
                                    .position_for_index(index, px(line_height))
                            },
                        );
                        let row_height = f32::from(cached.wrapped_line.row_height(
                            cached.wrapped_line.row_for_y(cursor_pos.y, px(line_height)),
                            px(line_height),
                        ));
                        let cursor_height =
                            row_height * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
                        let cursor_top = text_line_top
                            + f32::from(cursor_pos.y)
                            + (row_height - cursor_height) / 2.0;
                        let cursor_width = px(editor.styles.cursor.cursor_width_px);
                        cursor_paints.push(super::caret::CaretPaint {
                            bounds: Bounds::new(
                                point(
                                    px(content_x)
                                        + super::caret::centered_caret_left(cursor_x, cursor_width),
                                    px(cursor_top),
                                ),
                                size(cursor_width, px(cursor_height)),
                            ),
                            color: theme.cursor,
                            force_visible: editor.is_selecting,
                        });
                    }
                }

                if let Some(ref marked) = editor.marked_range {
                    let mark_start_on_line = marked.start.max(line_start_offset);
                    let mark_end_on_line = marked.end.min(line_end_offset);
                    if mark_start_on_line < mark_end_on_line {
                        let buf_start = mark_start_on_line - line_start_offset;
                        let buf_end = mark_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&cached.buffer_to_display, buf_start);
                        let disp_end =
                            helpers::buffer_to_display_offset(&cached.buffer_to_display, buf_end);
                        for bounds in helpers::build_highlight_bounds(
                            &cached.display_text,
                            &runs,
                            &cached.wrapped_line,
                            disp_start,
                            disp_end,
                            content_x,
                            text_line_top,
                            line_height,
                            font_size,
                            cached.indent_px,
                            0.0,
                            editor_styles.editor_letter_spacing_px,
                            window,
                        ) {
                            marked_underlines.push(fill(
                                Bounds::new(
                                    point(bounds.left(), bounds.bottom() - px(2.0)),
                                    size(bounds.size.width, px(1.0)),
                                ),
                                theme.text_primary,
                            ));
                        }
                    }
                }

                lines.push(PrepaintLine {
                    wrapped: cached.wrapped_line.clone(),
                    origin: helpers::baseline_aligned_text_origin(
                        &cached.wrapped_line,
                        origin,
                        px(line_height),
                        helpers::stable_text_baseline(
                            &helpers::line_baseline_font(
                                &decoration,
                                true,
                                &body_font,
                                &heading_font,
                                &monospace_font,
                            ),
                            px(font_size),
                            px(line_height),
                            &mut text_baseline_cache,
                            window,
                        ),
                    ),
                    line_height: px(line_height),
                    clip_bounds: non_markdown_nowrap.then(|| {
                        horizontal_text_clip_bounds(
                            Bounds::new(
                                point(px(non_markdown_content_left), px(text_line_top)),
                                size(px(text_width), px(text_line_height)),
                            ),
                            bounds,
                        )
                    }),
                    segments: None,
                    is_table_block: false,
                });

                y += total_line_height;
                _last_line_height = total_line_height;
                continue;
            }
            let active_tab_is_diff = editor.active_tab_is_diff();
            let front_matter_separator_line = decoration
                .table_info
                .as_ref()
                .is_some_and(|table| table.is_front_matter && table.is_separator);
            let code_block_syntax_active = if front_matter_separator_line {
                front_matter_delimiters_visible
            } else {
                active_code_fence_group
                    .is_some_and(|(start, end)| line_idx >= start && line_idx <= end)
                    || editor.search_reveals_code_fence_group(line_idx)
                    || (is_front_matter_line
                        && helpers::front_matter_row_is_active(&active_ranges, line_content.len()))
            };
            let diff_table_syntax_line = active_tab_is_diff
                && decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|ti| !ti.is_front_matter);
            let cursor_in_preview_source = |preview: &crate::markdown::BlockPreview| {
                (preview.start_line..=preview.end_line).contains(&cursor_line)
                    && preview.source_range.start <= cursor.offset
                    && cursor.offset <= preview.source_range.end
            };
            let selection_intersects_preview_source = |preview: &crate::markdown::BlockPreview| {
                all_sel_ranges.iter().any(|range| {
                    range.start < preview.source_range.end && range.end > preview.source_range.start
                })
            };

            let block_preview_code_mode =
                decoration.block_preview.as_ref().is_some_and(|preview| {
                    generated_preview_code_mode_active(
                        preview,
                        cursor_line,
                        cursor.offset,
                        &all_sel_ranges,
                        syntax_expansion_allowed,
                        search_reveal_range.as_ref(),
                    )
                });
            let block_preview_active = block_preview_code_mode
                || (syntax_expansion_allowed
                    && active_tab_is_diff
                    && decoration.block_preview.is_some());
            let collapsed_block_preview = decoration
                .block_preview
                .as_ref()
                .filter(|preview| !editor.folded_sections.contains(&preview.start_line))
                .filter(|_| !block_preview_active);
            let active_block_preview = decoration
                .block_preview
                .as_ref()
                .filter(|preview| !editor.folded_sections.contains(&preview.start_line))
                .filter(|_| block_preview_active)
                .filter(|preview| {
                    if block_preview_code_mode
                        || cursor_in_preview_source(preview)
                        || selection_intersects_preview_source(preview)
                    {
                        line_idx == preview.start_line
                    } else {
                        line_idx == preview.end_line
                    }
                });
            if let Some(preview) = collapsed_block_preview
                && !preview.is_anchor
            {
                if !suppress_scroll_anchor_compensation && cached_line_above_viewport {
                    scroll_anchor_delta -= cached_height_before_reflow.unwrap_or(0.0);
                }
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next());
                display_info_updates.push((
                    line_idx,
                    wrapped_line.map(|wrapped_line| {
                        let unwrapped_width_px = f32::from(wrapped_line.unwrapped_layout.width);
                        LineDisplayInfo {
                            display_text: String::new(),
                            display_to_buffer: Vec::new(),
                            buffer_to_display: vec![0; line_content.len()],
                            base_runs: Vec::new(),
                            font_size,
                            line_height: 0.0,
                            visual_line_count: 0,
                            wrapped_line,
                            wrap_width_px: None,
                            unwrapped_width_px,
                            wrap_boundary_upstream_xs: HashMap::new(),
                            extra_height: 0.0,
                            indent_px: 0.0,
                            content_x: editor_left + editor.padding_left(),
                            horizontal_clip_right_x: None,
                            selection_padding_left_px: 0.0,
                        }
                    }),
                ));
                continue;
            }
            let mut render_decoration_override = diff_table_syntax_line.then(|| {
                let mut render_decoration = crate::markdown::LineDecoration::default();
                render_decoration.block_type = BlockType::CodeBlock;
                render_decoration.diff_added_spans = decoration.diff_added_spans.clone();
                render_decoration.diff_removed_spans = decoration.diff_removed_spans.clone();
                render_decoration
            });
            if render_decoration_override.is_none()
                && !decoration.inline_math_spans.is_empty()
                && (*block_type != BlockType::Table || active_tab_is_diff)
            {
                let mut dynamic_decoration = decoration.clone();
                let placeholder_unit_width =
                    body_space_width * (font_size / editor.base_font_size()).max(0.1);
                if helpers::add_inline_math_display_replacements(
                    &mut dynamic_decoration,
                    line_content,
                    image_cache,
                    placeholder_unit_width,
                ) {
                    render_decoration_override = Some(dynamic_decoration);
                }
            }
            if is_front_matter_line {
                let dynamic_decoration =
                    render_decoration_override.get_or_insert_with(|| decoration.clone());
                super::front_matter::apply_date_time_format(
                    dynamic_decoration,
                    editor.app_settings.date_time_format(),
                );
                helpers::apply_front_matter_group_display(
                    dynamic_decoration,
                    line_idx,
                    group_plan,
                    line_content,
                );
            }
            let render_decoration = render_decoration_override.as_ref().unwrap_or(&decoration);

            let front_matter_summary = editor.front_matter_fold_summary(line_idx);
            let (mut display_text, mut display_to_buffer, buffer_to_display, prefix_collapsed) =
                if let Some(summary) = front_matter_summary.as_deref() {
                    helpers::build_front_matter_fold_display(line_content, summary)
                } else if collapsed_block_preview.is_some() {
                    (
                        String::new(),
                        Vec::new(),
                        vec![0; line_content.len()],
                        false,
                    )
                } else {
                    helpers::build_display_text(
                        line_content,
                        render_decoration,
                        &active_ranges,
                        code_block_syntax_active,
                        table_width_chars,
                        table_cell_padding_chars,
                        collapsed_task_prefix_display_columns,
                        checkbox_indent_step_display_columns,
                        editor.styles.checkbox.checkbox_parent_prefix_chars as usize,
                        0,
                        editor.styles.editor.editor_superscript_scale_factor,
                        editor.styles.editor.editor_subscript_scale_factor,
                        editor.styles.editor.editor_superscript_scale_factor,
                    )
                };
            editor.apply_list_display_styles(
                line_content,
                render_decoration,
                &mut display_text,
                prefix_collapsed,
            );

            let runs = if collapsed_block_preview.is_some() {
                vec![TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }]
            } else {
                let mut runs = helpers::build_text_runs_for_display_with_language(
                    &display_text,
                    line_content,
                    &display_to_buffer,
                    render_decoration,
                    &active_ranges,
                    code_block_syntax_active,
                    prefix_collapsed,
                    &body_font,
                    &body_cjk_font,
                    &monospace_font,
                    &heading_font,
                    &heading_cjk_font,
                    &italic_font,
                    &italic_cjk_font,
                    collapsed_task_prefix_display_columns,
                    font_size,
                    theme,
                    editor.styles.selection.selection_diff_added_inline_bg,
                    editor.styles.selection.selection_diff_removed_inline_bg,
                    editor.styles.wiki_link.wiki_link_text_color,
                    editor.styles.wiki_link.wiki_link_text_weight,
                    editor.styles.editor.editor_list_marker_text_color,
                    editor.styles.editor.editor_list_marker_font_weight,
                    editor.styles.editor.editor_heading_marker_text_color,
                    Some(editor.styles.table.front_matter_key_text_color),
                    editor.active_tab_is_non_markdown_text(),
                    editor.source_highlight_language(),
                );
                if front_matter_summary.is_some() {
                    for run in &mut runs {
                        run.color = theme.syntax_marker;
                    }
                } else if is_front_matter_line
                    && !front_matter_delimiters_visible
                    && render_decoration
                        .table_info
                        .as_ref()
                        .is_some_and(|table| table.is_separator)
                {
                    // Unfocused delimiters keep their measured row but draw no ink.
                    for run in &mut runs {
                        run.color = gpui::transparent_black();
                    }
                }
                helpers::apply_search_match_text_styles(
                    &mut runs,
                    &display_text,
                    &display_to_buffer,
                    line_start_offset,
                    &editor.search_matches,
                    editor.search_current_index,
                    editor.styles.search.search_match_text_color,
                    editor.styles.search.search_match_active_text_color,
                );
                if editor.current_file_path == editor.workspace_search_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &display_text,
                        &display_to_buffer,
                        line_start_offset,
                        &editor.workspace_search_highlight_ranges,
                        editor.workspace_search_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                if editor.current_file_path == editor.outline_flash_highlight_path {
                    helpers::apply_search_match_text_styles(
                        &mut runs,
                        &display_text,
                        &display_to_buffer,
                        line_start_offset,
                        &editor.outline_flash_highlight_ranges,
                        editor.outline_flash_highlight_current_index,
                        editor.styles.search.search_match_text_color,
                        editor.styles.search.search_match_active_text_color,
                    );
                }
                if diff_table_syntax_line && let Some(table_info) = decoration.table_info.as_ref() {
                    let syntax_ranges = diff_table_syntax_color_ranges(line_content, table_info);
                    apply_display_text_color_ranges(
                        &mut runs,
                        &display_text,
                        &syntax_ranges,
                        theme.syntax_marker,
                    );
                }
                if let Some(text_color) = diff_hunk_label_text_color(editor, line_idx) {
                    for run in &mut runs {
                        run.color = text_color;
                    }
                }
                runs
            };

            let display_shared: SharedString = if display_text.is_empty() {
                " ".into()
            } else {
                display_text.clone().into()
            };

            let is_list_type = matches!(
                block_type,
                BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
            );
            let prefix_bytes = if is_front_matter_line {
                render_decoration
                    .front_matter
                    .as_ref()
                    .and_then(|info| buffer_to_display.get(info.value_range.start))
                    .copied()
                    .unwrap_or(0)
            } else if is_list_type {
                helpers::compute_display_prefix_bytes(
                    &display_text,
                    block_type,
                    prefix_collapsed,
                    collapsed_task_prefix_display_columns,
                )
            } else {
                0
            };
            let wrap_indent_px = if prefix_bytes > 0 && prefix_bytes <= display_text.len() {
                let prefix_text = &display_text[..prefix_bytes];
                let prefix_runs = helpers::extract_runs_for_range(&runs, 0, prefix_bytes);
                let prefix_shared: SharedString = if prefix_text.is_empty() {
                    " ".into()
                } else {
                    prefix_text.to_string().into()
                };
                window
                    .text_system()
                    .shape_text(prefix_shared, px(font_size), &prefix_runs, None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next())
                    .map(|line| f32::from(line.width()))
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            let letter_spacing = editor_styles.editor_letter_spacing_px;

            let is_quote_line = editor.is_quote_line(line_idx);
            let quote_styles = &editor.styles.quote_block;
            let quote_border_width = if is_quote_line {
                editor.scaled(quote_styles.quote_block_border_width_px)
            } else {
                0.0
            };
            let quote_padding_left = if is_quote_line {
                editor.scaled(quote_styles.quote_block_padding_left_px)
            } else {
                0.0
            };
            let quote_padding_right = if is_quote_line {
                editor.scaled(quote_styles.quote_block_padding_right_px)
            } else {
                0.0
            };
            let quote_inner_left_padding = quote_border_width + quote_padding_left;
            let quote_horizontal_padding = quote_inner_left_padding + quote_padding_right;
            // A quote nested in a list keeps that list's indentation, which the
            // concealed `>` prefix would otherwise collapse to the left margin.
            let quote_indent_px = if is_quote_line {
                let leading_ws =
                    line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
                leading_ws as f32 * body_space_width
            } else {
                0.0
            };
            // Quoted rows carrying their own marker step one level deeper. Only
            // the text moves; the quote bar and fill stay put.
            let quote_content_indent_px = if is_quote_line
                && matches!(
                    block_type,
                    BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
                ) {
                editor.scaled(quote_styles.quote_block_line_syntax_indent_px)
            } else {
                0.0
            };

            let code_mode_split_gap = editor.scaled(14.0);
            let code_mode_text_width = ((text_width - code_mode_split_gap) * 0.5).max(80.0);
            let code_block_group = (*block_type == BlockType::CodeBlock)
                .then(|| code_block_group_range(line_idx))
                .flatten();
            let code_block_line_number = code_block_group.and_then(|(group_start, group_end)| {
                editor
                    .code_block_line_numbers_enabled(group_start)
                    .then(|| {
                        let number = line_idx.saturating_sub(group_start).max(1);
                        let max_number = group_end.saturating_sub(group_start + 1).max(1);
                        let digits = max_number.to_string().len() as f32;
                        let gutter = &editor.styles.gutter;
                        let code_block = &editor.styles.code_block;
                        let width = editor
                            .scaled(code_block.code_block_line_number_padding_left_px)
                            + digits * editor.scaled(gutter.gutter_line_number_font_size_px) * 0.7
                            + editor.scaled(gutter.gutter_line_number_right_gap_px);
                        (number, width)
                    })
            });
            let code_block_line_number_width = code_block_line_number
                .map(|(_, width)| width)
                .unwrap_or(0.0);
            let code_block_right_padding =
                editor.scaled(editor.styles.code_block.code_block_wrap_right_padding_px);
            let code_block_background_left = editor_left + editor.padding_left();
            let code_block_background_width = if block_preview_code_mode {
                code_mode_text_width
            } else {
                text_width
            };
            let code_block_left_inset = quote_indent_px
                + quote_inner_left_padding
                + editor.scaled(editor.styles.code_block.code_block_text_padding_left_px)
                + code_block_line_number_width;
            let (code_block_content_left, code_block_viewport_width, code_block_content_right) =
                code_block_text_viewport_geometry(
                    code_block_background_left,
                    code_block_background_width,
                    code_block_left_inset,
                    quote_padding_right + code_block_right_padding,
                );
            let base_wrap_width = if is_front_matter_line {
                front_matter_wrap_width
            } else if block_preview_code_mode {
                helpers::code_block_soft_wrap_width(code_block_viewport_width, base_char_width)
            } else if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence) {
                helpers::code_block_soft_wrap_width(code_block_viewport_width, base_char_width)
            } else if is_quote_line {
                (text_width - quote_horizontal_padding - quote_indent_px - quote_content_indent_px)
                    .max(editor.scaled(80.0))
            } else {
                wrap_width
            };

            let effective_wrap_width = if is_front_matter_line {
                base_wrap_width
            } else if wrap_indent_px > 0.0 {
                (base_wrap_width - wrap_indent_px).max(base_wrap_width * 0.3)
            } else {
                base_wrap_width
            };

            let code_block_text_viewport_width = code_block_group
                .map(|_| code_block_viewport_width)
                .unwrap_or(effective_wrap_width);

            let code_block_word_wrap = code_block_group
                .map(|(group_start, _)| editor.code_block_word_wrap_enabled(group_start))
                .unwrap_or(true);
            let wrap_width_param = if *block_type == BlockType::Table && !is_front_matter_line {
                None
            } else if !editor.editor_wraps_to_available_width() || !code_block_word_wrap {
                None
            } else {
                Some(px(effective_wrap_width))
            };

            let Ok(mut wrapped_lines) = window.text_system().shape_text(
                display_shared,
                px(font_size),
                &runs,
                wrap_width_param,
                None,
            ) else {
                display_info_updates.push((line_idx, None));
                y += estimated_total_line_height;
                continue;
            };

            if letter_spacing != 0.0 {
                for w in wrapped_lines.iter_mut() {
                    helpers::apply_letter_spacing(w, letter_spacing);
                }
            }

            for wrapped in &mut wrapped_lines {
                helpers::keep_inline_math_placeholders_unwrapped(
                    wrapped,
                    render_decoration,
                    line_content,
                    &buffer_to_display,
                    &active_ranges,
                    image_cache,
                    0,
                );
                helpers::apply_inline_math_row_heights(
                    wrapped,
                    render_decoration,
                    line_content,
                    &buffer_to_display,
                    &active_ranges,
                    image_cache,
                    line_height,
                    0,
                );
            }
            let metadata_indent = if is_front_matter_line {
                editor.front_matter_columns.and_then(|columns| {
                    wrapped_lines.first_mut().and_then(|wrapped| {
                        super::front_matter_layout::align_and_wrap(
                            wrapped,
                            render_decoration,
                            &buffer_to_display,
                            helpers::front_matter_row_is_active(&active_ranges, line_content.len()),
                            columns,
                            wrap_width_param.map(|_| base_wrap_width),
                            font_size * editor.styles.checkbox.checkbox_size_factor,
                        )
                    })
                })
            } else {
                None
            };
            let Some(wrapped) = wrapped_lines.first() else {
                display_info_updates.push((line_idx, None));
                y += estimated_total_line_height;
                continue;
            };
            let code_block_horizontal_offset = code_block_group
                .filter(|_| !code_block_word_wrap)
                .map(|(group_start, _)| {
                    let caret_width = editor.styles.cursor.cursor_width_px.max(1.0);
                    let max_offset = (f32::from(wrapped.unwrapped_layout.width)
                        + code_block_right_padding
                        + caret_width
                        - code_block_text_viewport_width)
                        .max(0.0);
                    code_block_horizontal_extents
                        .entry(group_start)
                        .and_modify(|current| *current = current.max(max_offset))
                        .or_insert(max_offset);
                    editor
                        .code_block_display_override(group_start)
                        .horizontal_offset
                })
                .unwrap_or(0.0);
            if metadata_indent.is_some() {
                super::front_matter_layout::align_stacked_padding_hits(
                    wrapped,
                    render_decoration,
                    &buffer_to_display,
                    &mut display_to_buffer,
                );
            }
            let indent_px = if let Some(indent) = metadata_indent {
                indent
            } else if prefix_bytes > 0 && prefix_bytes <= display_text.len() {
                if is_front_matter_line {
                    super::front_matter::wrap_indent(
                        f32::from(wrapped.unwrapped_layout.x_for_index(prefix_bytes)),
                        wrap_width_param.map(|_| base_wrap_width),
                    )
                } else {
                    wrapped
                        .position_for_index(prefix_bytes, px(line_height))
                        .map(|position| f32::from(position.x))
                        .unwrap_or(0.0)
                }
            } else {
                0.0
            };
            let visual_line_count = wrapped.wrap_boundaries().len() + 1;
            let text_line_height = f32::from(wrapped.size(px(line_height)).height);
            let image_gap_top = editor.scaled(editor.styles.image_preview.image_gap_top_px);
            let image_gap_bottom = editor.scaled(editor.styles.image_preview.image_gap_bottom_px);
            let image_styles = &editor.styles.image_preview;
            let image_corner_radius = px(editor.scaled(image_styles.image_border_radius_px));
            let image_border_width = px(editor.scaled(image_styles.image_active_border_width_px));

            let image_active = decoration.image_path.is_some()
                && (editor.search_reveals_line(line_idx)
                    || (syntax_expansion_allowed
                        && helpers::is_image_syntax_active(
                            &decoration,
                            line_content,
                            line_start_offset,
                            cursor.offset,
                            &sel_range,
                        )));

            let image_extra_height = if let Some(ref path) = decoration.image_path {
                if editor.folded_image_previews.contains(&line_idx) {
                    0.0
                } else if let Some(Some(img_data)) = image_cache.get(path) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0 && img_h > 0.0 {
                        let (_, display_h) = helpers::compute_image_preview_size(
                            img_w,
                            img_h,
                            text_width,
                            window_scale_factor,
                        );
                        display_h + image_gap_top + image_gap_bottom
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };
            // `![](url)` lines without alt text conceal to an empty display
            // string while the syntax is inactive. Keep their caption row's
            // layout height stable though: dropping it to zero was introduced
            // in a recent UI tweak and makes cursor navigation above/below
            // image previews visibly jump as focus toggles the syntax row.
            let image_caption_hidden =
                image_extra_height > 0.0 && !image_active && display_text.trim().is_empty();
            let rendered_block_preview = if block_preview_code_mode {
                None
            } else if block_preview_active {
                active_block_preview
            } else {
                collapsed_block_preview.filter(|preview| preview.is_anchor)
            };
            // Hoisted once so the background, layout (extra-height) and draw
            // paths share identical geometry: (fitted_w, fitted_h, bg_w, bg_h).
            let generated_preview_layout = rendered_block_preview.and_then(|preview| {
                let Some(Some(img_data)) = image_cache.get(&preview.cache_key) else {
                    return None;
                };
                let img_size = img_data.size(0);
                let img_w = img_size.width.0 as f32;
                let img_h = img_size.height.0 as f32;
                if img_w <= 0.0 || img_h <= 0.0 {
                    return None;
                }
                Some(helpers::fit_generated_preview_size(
                    img_w,
                    img_h,
                    preview.kind,
                    text_width,
                    editor.scaled(editor.styles.code_block.generated_preview_min_height_px),
                    editor.scaled(editor.styles.code_block.generated_preview_max_height_px),
                    editor.scaled(editor.styles.code_block.generated_preview_padding_x_px),
                    editor.scaled(editor.styles.code_block.generated_preview_padding_y_px),
                ))
            });
            let generated_preview_extra_height = generated_preview_layout
                .map(|(_, _, _, bg_h)| bg_h.max(text_line_height) - text_line_height)
                .unwrap_or(0.0);

            // Compute table block extra height for header line (table rendered as block below)
            let table_extra_height = if *block_type == BlockType::Table && !active_tab_is_diff {
                if let Some(ref ti) = decoration.table_info {
                    if ti.is_header && !ti.is_front_matter {
                        let layout = table_layout_cache
                            .entry(ti.table_start_line)
                            .or_insert_with(|| {
                                let table_styles = &editor.styles.table;
                                build_precise_table_layout(
                                    window,
                                    buffer,
                                    &decorations,
                                    image_cache,
                                    ti.table_start_line,
                                    ti.table_end_line,
                                    text_width,
                                    editor.scaled(table_styles.table_cell_padding_px),
                                    editor.scaled(table_styles.table_cell_padding_px) * 0.6,
                                    base_char_width,
                                    line_height,
                                    body_space_width,
                                    super::table_cell_render::task_prefix_columns(
                                        base_char_width + letter_spacing,
                                        checkbox_size_px + letter_spacing.max(0.0),
                                        checkbox_text_gap_px,
                                    ),
                                    task_source_indent_step_chars,
                                    editor.scaled(table_styles.table_border_track_width_px),
                                    &body_font,
                                    &body_cjk_font,
                                    &monospace_font,
                                    &heading_font,
                                    &heading_cjk_font,
                                    &italic_font,
                                    &italic_cjk_font,
                                    font_size,
                                    letter_spacing,
                                    editor.styles.editor.editor_superscript_scale_factor,
                                    editor.styles.editor.editor_subscript_scale_factor,
                                    theme,
                                    editor.styles.editor.editor_list_marker_text_color,
                                    editor.styles.editor.editor_list_marker_font_weight,
                                    editor.styles.editor.editor_heading_marker_text_color,
                                    &table_syntax_focus,
                                )
                            })
                            .clone();
                        // Subtract line_height since the header line's text area
                        // (which is hidden via is_table_block) already occupies one line_height.
                        (layout.total_height - line_height).max(0.0)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let extra_height = block_padding_top
                + block_padding_bottom
                + image_extra_height
                + generated_preview_extra_height
                + table_extra_height;
            let total_line_height = text_line_height + extra_height;

            if !suppress_scroll_anchor_compensation {
                if line_has_block_preview || !decoration.inline_math_spans.is_empty() {
                    let previous_height = cached_height_before_reflow
                        .unwrap_or_else(|| editor.visual_height_for_line(line_idx));
                    scroll_anchor_delta += preview_height_anchor_delta(
                        previous_height,
                        total_line_height,
                        // Classify against the OLD geometry, not the running
                        // painted y: an earlier visible preview may have shrunk
                        // and moved this row above the viewport during reflow.
                        editor.line_y_offset_precise(line_idx) + previous_height as f64,
                        scroll_offset as f64,
                    );
                } else if editor.active_tab_is_non_markdown_text()
                    && editor.editor_wraps_to_available_width()
                    && !can_reuse_non_markdown_cached_line
                    && cached_line_above_viewport
                    && let Some(cached_h) = cached_height_before_reflow
                {
                    scroll_anchor_delta += total_line_height - cached_h;
                }
            }

            let line_top = y - scroll_anchor_delta;
            let line_bottom = line_top + total_line_height;
            // The inline image preview is painted above its syntax text, so the
            // text row is pushed down by the preview's reserved height. Keep
            // this in sync with `EditorView::text_y_offset_for_line`.
            let text_line_top = line_top + block_padding_top + image_extra_height;
            if line_bottom >= origin_y && line_top <= origin_y + viewport_height {
                inline_diff_line_geometry.push((line_idx, line_top, total_line_height));
            }
            let base_content_x = if is_front_matter_line {
                front_matter_content_left
            } else if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence) {
                code_block_content_left
            } else if is_quote_line {
                editor_left
                    + editor.padding_left()
                    + quote_indent_px
                    + quote_content_indent_px
                    + quote_inner_left_padding
            } else {
                editor_left + editor.padding_left()
            };
            let content_x = if matches!(block_type, BlockType::TaskList(_)) && prefix_collapsed {
                let target_prefix_width = helpers::task_prefix_target_width_px(
                    line_content,
                    font_size * editor.styles.checkbox.checkbox_size_factor,
                    checkbox_text_gap_px,
                    task_source_indent_step_chars,
                )
                .unwrap_or(indent_px);
                base_content_x + (target_prefix_width - indent_px)
            } else {
                base_content_x
            };
            if non_markdown_nowrap {
                let caret_width = editor.styles.cursor.cursor_width_px.max(1.0);
                let right_padding =
                    editor.scaled(editor.styles.editor.editor_wrap_right_padding_px);
                let right_edge = content_x - non_markdown_content_left
                    + f32::from(wrapped.unwrapped_layout.width);
                non_markdown_horizontal_extent =
                    non_markdown_horizontal_extent.max(non_markdown_horizontal_max_offset(
                        right_edge,
                        text_width,
                        right_padding,
                        caret_width,
                    ));
            }
            let content_x =
                content_x - code_block_horizontal_offset - non_markdown_horizontal_offset;
            // Image-only lines render their syntax as a caption under the
            // (centred) preview, centred both while collapsed and while the
            // cursor expands it for editing. Wrapped captions stay
            // left-aligned, as does the syntax when the preview is
            // gutter-folded (or not loaded) and no image is shown above.
            let image_caption_centered = decoration.image_path.is_some()
                && image_extra_height > 0.0
                && !image_caption_hidden
                && wrapped.wrap_boundaries().is_empty()
                && helpers::image_syntax_range(line_content).is_some_and(|range| {
                    line_content[..range.start].trim().is_empty()
                        && line_content[range.end..].trim().is_empty()
                });
            let content_x = if image_caption_centered {
                let caption_width = wrapped
                    .position_for_index(display_text.len(), px(line_height))
                    .map(|pos| f32::from(pos.x))
                    .unwrap_or_else(|| f32::from(wrapped.unwrapped_layout.width));
                content_x + ((text_width - caption_width) / 2.0).max(0.0)
            } else {
                content_x
            };
            let property_line = group_plan
                .filter(|plan| plan.label_line() == line_idx)
                .map_or(line_idx, |plan| plan.group.header_line);
            if editor.active_tab_is_markdown() && !editor.active_tab_is_read_only() {
                let viewport = bounds.intersect(&window.content_mask().bounds);
                let icon_size = editor.scaled(LINE_FORMAT_ICON_SIZE_PX);
                let icon_gap = editor.scaled(LINE_FORMAT_ICON_GAP_PX);
                let content_gap = editor.scaled(LINE_FORMAT_CONTENT_GAP_PX);
                let trigger_padding = editor.scaled(4.0);
                let fold_icon_size = editor.scaled(editor.styles.gutter.gutter_fold_icon_size_px);
                let has_fold_icon = (editor.fold_range_for_line(line_idx).is_some()
                    && !super::fold::front_matter_opener_hides_fold_control(
                        &editor.decorations,
                        line_idx,
                        front_matter_fold_control_line,
                    ))
                    || front_matter_fold_control_line == Some(line_idx)
                    || decoration.image_path.as_ref().is_some_and(|path| {
                        image_cache.get(path).is_some_and(|image| image.is_some())
                    });
                let icon_x = line_format_trigger_left(
                    editor_left + editor.padding_left(),
                    icon_size,
                    has_fold_icon.then_some(fold_icon_size),
                    icon_gap,
                    content_gap,
                );
                let icon_y = if image_extra_height > 0.0 {
                    text_line_top - image_extra_height + image_gap_top
                } else {
                    text_line_top + (line_height - icon_size) * 0.5
                };
                let icon_bounds = Bounds::new(
                    point(px(icon_x), px(icon_y)),
                    size(px(icon_size), px(icon_size)),
                );
                let trigger_bounds = Bounds::new(
                    point(
                        px(icon_x - trigger_padding),
                        px(if image_extra_height > 0.0 {
                            icon_y
                        } else {
                            text_line_top
                        }),
                    ),
                    size(
                        px(icon_size
                            + trigger_padding
                            + if has_fold_icon {
                                icon_gap * 0.5
                            } else {
                                trigger_padding
                            }),
                        px(line_height.max(icon_size)),
                    ),
                );
                let row_hover_left = (icon_x - trigger_padding).min(origin_x);
                let row_hover_right = f32::from(viewport.right());
                format_line_areas_local.push((
                    Bounds::new(
                        point(px(row_hover_left), px(line_top)),
                        size(
                            px((row_hover_right - row_hover_left).max(0.0)),
                            px(total_line_height),
                        ),
                    ),
                    property_line,
                ));
                format_trigger_areas_local.push((trigger_bounds, property_line));
                line_format_toolbar_geometries_local.push(
                    selection_toolbar::LineFormatToolbarGeometry {
                        line: property_line,
                        viewport,
                        content_left: px(icon_x),
                        trigger_bounds,
                    },
                );
                if editor.format_trigger_line == Some(property_line)
                    && editor.format_trigger_opacity > 0.01
                {
                    let mut color = editor.styles.format_toolbar.format_toolbar_icon_color;
                    color.a = editor.format_trigger_opacity;
                    if color.a > 0.01 {
                        format_icons.push((icon_bounds, color));
                    }
                }
            }
            let selection_padding_left_px = if image_caption_centered {
                0.0
            } else {
                (content_x - (editor_left + editor.padding_left())).max(0.0)
            };
            if let Some((line_number, _)) = code_block_line_number {
                let gutter = &editor.styles.gutter;
                let background_left = editor_left + editor.padding_left();
                push_code_block_line_number_label(
                    &mut gutter_labels,
                    window,
                    line_number,
                    active_line_numbers.contains(&line_idx),
                    &monospace_font,
                    editor.scaled(gutter.gutter_line_number_font_size_px),
                    background_left
                        + editor.scaled(
                            editor
                                .styles
                                .code_block
                                .code_block_line_number_padding_left_px,
                        ),
                    text_line_top,
                    line_height,
                    editor.scaled(gutter.gutter_line_number_vertical_offset_px),
                    gutter.gutter_line_number_text_color,
                    gutter.gutter_line_number_active_text_color,
                );
            }
            let wrap_boundary_upstream_xs = if metadata_indent.is_some() {
                super::front_matter_layout::wrap_boundary_upstream_xs(wrapped)
            } else {
                compute_wrap_boundary_upstream_xs(
                    window,
                    wrapped,
                    &display_text,
                    &runs,
                    font_size,
                    line_height,
                    letter_spacing,
                )
            };

            display_info_updates.push((
                line_idx,
                Some(LineDisplayInfo {
                    display_text: display_text.clone(),
                    display_to_buffer: display_to_buffer.clone(),
                    buffer_to_display: buffer_to_display.clone(),
                    base_runs: runs.clone(),
                    font_size,
                    line_height,
                    visual_line_count,
                    wrapped_line: wrapped.clone(),
                    wrap_width_px: wrapped.wrap_width.map(f32::from),
                    unwrapped_width_px: f32::from(wrapped.unwrapped_layout.width),
                    wrap_boundary_upstream_xs: wrap_boundary_upstream_xs.clone(),
                    extra_height,
                    indent_px,
                    content_x,
                    horizontal_clip_right_x: code_block_group
                        .filter(|_| !code_block_word_wrap)
                        .map(|_| code_block_content_right)
                        .or_else(|| {
                            non_markdown_nowrap.then_some(non_markdown_content_left + text_width)
                        }),
                    selection_padding_left_px,
                }),
            ));

            if line_bottom < origin_y || line_top > origin_y + viewport_height {
                if editor.active_tab_is_non_markdown_text()
                    && editor.editor_wraps_to_available_width()
                    && cached_line.is_none()
                    && line_top > origin_y + viewport_height
                    && remaining_offscreen_wrap_reflows > 0
                {
                    remaining_offscreen_wrap_reflows -= 1;
                }
                y += total_line_height;
                _last_line_height = total_line_height;
                continue;
            }

            let front_matter_icon_type = if front_matter_summary.is_some() {
                Some(super::front_matter::FrontMatterType::List)
            } else if is_front_matter_line {
                super::front_matter::property_type(buffer, &decorations, property_line)
            } else {
                None
            };
            if let Some(ty) = front_matter_icon_type {
                let slot_size = editor.scaled(LINE_FORMAT_ICON_SIZE_PX);
                let slot_bounds = Bounds::new(
                    point(
                        px(editor_left + editor.padding_left()),
                        px(text_line_top + (line_height - slot_size) * 0.5),
                    ),
                    size(px(slot_size), px(slot_size)),
                );
                let viewport = bounds.intersect(&window.content_mask().bounds);
                if let Some(hit_bounds) = front_matter_type_hit_bounds(slot_bounds, viewport) {
                    let styles = &editor.styles.format_toolbar;
                    // The collapsed summary is not a property, so its icon never takes hover accent.
                    let color = if front_matter_summary.is_none()
                        && hit_bounds.contains(&window.mouse_position())
                    {
                        editor.theme.text_accent
                    } else {
                        let mut color = styles.format_toolbar_icon_color;
                        color.a *= 0.76;
                        color
                    };
                    let icon_size = editor
                        .scaled(styles.format_toolbar_icon_size_px)
                        .min(slot_size);
                    let path = front_matter_type_icon(ty);
                    let mut glyph_bounds =
                        front_matter_type_glyph_bounds(slot_bounds, px(icon_size));
                    // The slot has no padding: the remaining inset is inside the SVG view box.
                    glyph_bounds.origin.x -= px(icon_size * front_matter_type_icon_inset(path, cx));
                    front_matter_type_icons.push((glyph_bounds, path, color));
                    if front_matter_summary.is_none() {
                        front_matter_type_areas_local.push((hit_bounds, property_line));
                    }
                }
            }

            if let Some(plan) = group_plan.filter(|plan| {
                !plan.active && !plan.source_visible && plan.label_line() == line_idx
            }) {
                push_front_matter_group_paint(
                    editor,
                    plan,
                    window,
                    bounds,
                    wrapped,
                    prefix_bytes,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    front_matter_wrap_width,
                    &monospace_font,
                    &mut front_matter_group_texts,
                    &mut front_matter_text_areas,
                    &mut front_matter_delete_areas,
                    &mut front_matter_list_scroll_areas,
                    &mut clipped_table_selections,
                );
            }

            let reference_baseline = helpers::stable_text_baseline(
                &helpers::line_baseline_font(
                    render_decoration,
                    editor.active_tab_is_non_markdown_text(),
                    &body_font,
                    &heading_font,
                    &monospace_font,
                ),
                px(font_size),
                px(line_height),
                &mut text_baseline_cache,
                window,
            );
            let segments = if !wrapped.wrap_boundaries().is_empty() && indent_px > 0.0 {
                let byte_indices = helpers::wrap_boundary_byte_indices(wrapped, &display_text);
                let mut shaped_segments: Vec<(WrappedLine, Point<Pixels>)> = Vec::new();
                for (seg_idx, segment_range) in
                    helpers::wrap_segment_byte_ranges(display_text.len(), &byte_indices)
                        .into_iter()
                        .enumerate()
                {
                    let seg_text = &display_text[segment_range.clone()];
                    let seg_runs = helpers::extract_runs_for_range(
                        &runs,
                        segment_range.start,
                        segment_range.end,
                    );
                    let seg_shared: SharedString = if seg_text.is_empty() {
                        " ".into()
                    } else {
                        seg_text.to_string().into()
                    };
                    if let Ok(seg_wrapped_lines) = window.text_system().shape_text(
                        seg_shared,
                        px(font_size),
                        &seg_runs,
                        None,
                        None,
                    ) {
                        if let Some(mut seg_wrapped) = seg_wrapped_lines.into_iter().next() {
                            if metadata_indent.is_some() {
                                super::front_matter_layout::copy_segment_geometry(
                                    &mut seg_wrapped,
                                    wrapped,
                                    segment_range.clone(),
                                );
                            } else if letter_spacing != 0.0 {
                                helpers::apply_letter_spacing(&mut seg_wrapped, letter_spacing);
                            }
                            helpers::keep_inline_math_placeholders_unwrapped(
                                &mut seg_wrapped,
                                render_decoration,
                                line_content,
                                &buffer_to_display,
                                &active_ranges,
                                image_cache,
                                segment_range.start,
                            );
                            let seg_x = if seg_idx == 0 {
                                content_x
                            } else {
                                content_x + indent_px
                            };
                            let seg_height = wrapped.row_height(seg_idx, px(line_height));
                            seg_wrapped.set_row_heights(vec![seg_height]);
                            let seg_y = text_line_top
                                + f32::from(wrapped.row_top(seg_idx, px(line_height)));
                            let paint_origin = helpers::baseline_aligned_text_origin(
                                &seg_wrapped,
                                point(px(seg_x), px(seg_y)),
                                seg_height,
                                reference_baseline + (seg_height - px(line_height)) / 2.0,
                            );
                            shaped_segments.push((seg_wrapped, paint_origin));
                        }
                    }
                }
                Some(shaped_segments)
            } else {
                None
            };

            let origin = point(px(content_x), px(text_line_top));
            let show_line_indent_guides =
                show_non_markdown_indent_guides || code_block_syntax_active;
            let line_indent_guide_step_columns = if show_non_markdown_indent_guides {
                indent_guide_step_columns
            } else if code_block_syntax_active {
                active_markdown_indent_guide_step_columns
            } else {
                0
            };
            let indent_guide_start = indent_guide_quads.len();
            if show_line_indent_guides {
                push_indent_guide_quads(
                    &mut indent_guide_quads,
                    line_content,
                    content_x,
                    line_top,
                    total_line_height,
                    base_char_width,
                    line_indent_guide_step_columns,
                    indent_guide_width,
                    indent_guide_dash_len,
                    indent_guide_gap_len,
                    indent_guide_color,
                );
            }
            let nowrap_clip_bounds = code_block_group
                .filter(|_| !code_block_word_wrap)
                .map(|_| {
                    Bounds::new(
                        point(px(base_content_x), px(text_line_top)),
                        size(px(code_block_text_viewport_width), px(text_line_height)),
                    )
                })
                .or_else(|| {
                    non_markdown_nowrap.then(|| {
                        Bounds::new(
                            point(px(non_markdown_content_left), px(text_line_top)),
                            size(px(text_width), px(text_line_height)),
                        )
                    })
                });
            let code_block_text_clip_bounds = code_block_group.map(|_| {
                let clip_right = code_block_text_clip_right(
                    code_block_background_left + code_block_background_width,
                    code_block_content_right,
                    code_block_word_wrap,
                );
                Bounds::new(
                    point(px(base_content_x), px(text_line_top)),
                    size(
                        px((clip_right - base_content_x).max(1.0)),
                        px(text_line_height),
                    ),
                )
            });
            let text_clip_bounds = code_block_text_clip_bounds.or_else(|| {
                non_markdown_nowrap.then(|| {
                    Bounds::new(
                        point(px(non_markdown_content_left), px(text_line_top)),
                        size(px(text_width), px(text_line_height)),
                    )
                })
            });
            if let Some(clip_bounds) = nowrap_clip_bounds
                && indent_guide_quads.len() > indent_guide_start
            {
                clipped_code_block_decorations.push((
                    clip_bounds,
                    indent_guide_quads.split_off(indent_guide_start),
                ));
            }
            let is_active_line_number = active_line_numbers.contains(&line_idx);
            let is_table_source_row = *block_type == BlockType::Table
                && !active_tab_is_diff
                && decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|ti| !ti.is_front_matter && !ti.is_separator);
            if !is_table_source_row && editor.app_settings.show_whitespace {
                let whitespace_skip_prefix_bytes = if matches!(block_type, BlockType::TaskList(_)) {
                    prefix_bytes
                } else {
                    0
                };
                push_whitespace_dot_quads(
                    &mut whitespace_dots,
                    &display_text,
                    &wrapped,
                    origin,
                    line_height,
                    editor.scaled(editor.styles.editor.editor_whitespace_dot_size_px),
                    editor.styles.editor.editor_whitespace_dot_color,
                    whitespace_skip_prefix_bytes,
                );
            }

            if is_quote_line {
                let group_start = editor.quote_group_start(line_idx);
                let callout_kind = editor
                    .decorations
                    .get(group_start)
                    .and_then(|decoration| decoration.callout_kind);
                let (quote_bg, quote_border_color) = quote_block_colors(editor, callout_kind);
                let background_left = editor_left + editor.padding_left() + quote_indent_px;
                let background_width = (text_width - quote_indent_px).max(0.0);
                if background_width > 0.0 {
                    quote_block_bgs.push(quad(
                        Bounds::new(
                            point(px(background_left), px(line_top)),
                            size(px(background_width), px(total_line_height)),
                        ),
                        Corners::all(px(editor.scaled(quote_styles.quote_block_radius_px))),
                        quote_bg,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                    quote_block_bgs.push(fill(
                        Bounds::new(
                            point(px(background_left), px(line_top)),
                            size(px(quote_border_width), px(total_line_height)),
                        ),
                        quote_border_color,
                    ));
                }
            }

            // Code block-like background
            if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence)
                || diff_table_syntax_line
                || rendered_block_preview.is_some()
            {
                let corner_r = px(editor.scaled(editor.styles.code_block.code_block_radius_px));
                let side_inset = snap_pixel_width(
                    editor.scaled(editor.styles.code_block.code_block_fill_inset_px),
                    window,
                );
                let (group_start, group_end) = if is_front_matter_line || diff_table_syntax_line {
                    let table_info = decoration.table_info.as_ref();
                    let start = table_info.map(|ti| ti.table_start_line).unwrap_or(line_idx);
                    let end = table_info
                        .map(|ti| ti.table_end_line.saturating_sub(1))
                        .unwrap_or(line_idx);
                    (start, end)
                } else {
                    code_block_group_range(line_idx).unwrap_or((line_idx, line_idx))
                };
                let is_group_start = line_idx == group_start;
                let is_group_end = line_idx == group_end;
                let has_top_corner = is_group_start;
                let has_bottom_corner = is_group_end || collapsed_block_preview.is_some();
                let background_left = editor_left + editor.padding_left();
                let background_bottom = line_top + total_line_height;
                let fill_left = background_left + side_inset;
                let fill_width = (text_width - side_inset * 2.0).max(0.0);
                code_block_group_bounds
                    .entry(group_start)
                    .and_modify(|group| {
                        group.top = group.top.min(line_top);
                        group.bottom = group.bottom.max(background_bottom);
                    })
                    .or_insert(CodeBlockGroupBounds {
                        left: background_left,
                        top: line_top,
                        width: text_width,
                        bottom: background_bottom,
                    });
                if fill_width > 0.0 {
                    code_block_bgs.push(quad(
                        snap_bounds_to_device_pixels(
                            fill_left,
                            line_top,
                            fill_width,
                            total_line_height,
                            window,
                        ),
                        Corners {
                            top_left: if has_top_corner { corner_r } else { px(0.0) },
                            top_right: if has_top_corner { corner_r } else { px(0.0) },
                            bottom_right: if has_bottom_corner { corner_r } else { px(0.0) },
                            bottom_left: if has_bottom_corner { corner_r } else { px(0.0) },
                        },
                        editor.styles.code_block.code_block_bg,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }

            // Table rendering (independent visual block on header line)
            if *block_type == BlockType::Table && !active_tab_is_diff {
                if let Some(ref ti) = decoration.table_info {
                    if ti.is_header && !ti.is_front_matter {
                        let table_styles = &editor.styles.table;
                        let border_color = table_styles.table_border_color;
                        let border_w = table_styles.table_border_width_px;
                        let table_left = editor_left + editor.padding_left();

                        // Get or compute the cached table layout
                        let layout = table_layout_cache
                            .entry(ti.table_start_line)
                            .or_insert_with(|| {
                                build_precise_table_layout(
                                    window,
                                    buffer,
                                    &decorations,
                                    image_cache,
                                    ti.table_start_line,
                                    ti.table_end_line,
                                    text_width,
                                    table_cell_padding_px,
                                    table_cell_padding_px * 0.6,
                                    base_char_width,
                                    line_height,
                                    body_space_width,
                                    super::table_cell_render::task_prefix_columns(
                                        base_char_width + letter_spacing,
                                        checkbox_size_px + letter_spacing.max(0.0),
                                        checkbox_text_gap_px,
                                    ),
                                    task_source_indent_step_chars,
                                    editor.scaled(table_styles.table_border_track_width_px),
                                    &body_font,
                                    &body_cjk_font,
                                    &monospace_font,
                                    &heading_font,
                                    &heading_cjk_font,
                                    &italic_font,
                                    &italic_cjk_font,
                                    font_size,
                                    letter_spacing,
                                    editor.styles.editor.editor_superscript_scale_factor,
                                    editor.styles.editor.editor_subscript_scale_factor,
                                    theme,
                                    editor.styles.editor.editor_list_marker_text_color,
                                    editor.styles.editor.editor_list_marker_font_weight,
                                    editor.styles.editor.editor_heading_marker_text_color,
                                    &table_syntax_focus,
                                )
                            })
                            .clone();

                        let table_width = layout.total_width.min(text_width);
                        // Table block starts at text_line_top (header text is hidden)
                        let table_top = text_line_top;
                        let outer_radius = px(table_styles.table_outer_radius_px);
                        let table_bottom = table_top + layout.total_height;
                        let table_right = table_left + layout.total_width;

                        if layout.visual_row_count > 0 {
                            let first_row_height =
                                layout.row_heights.first().copied().unwrap_or(line_height);
                            let last_row_height =
                                layout.row_heights.last().copied().unwrap_or(line_height);
                            let cursor_height = line_height
                                * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
                            let cursor_width = editor.styles.cursor.cursor_width_px;
                            let boundary_gap = editor.scaled(4.0);
                            let left_blank_right = table_left;
                            let viewport_left = origin_x;
                            let viewport_right = origin_x + viewport_width;
                            let right_blank_right = viewport_right;
                            if left_blank_right > viewport_left {
                                table_boundary_caret_areas_local.push((
                                    Bounds::new(
                                        point(px(viewport_left), px(table_top)),
                                        size(
                                            px(left_blank_right - viewport_left),
                                            px(layout.total_height),
                                        ),
                                    ),
                                    TableBoundaryCaret {
                                        table_start_line: ti.table_start_line,
                                        side: TableBoundarySide::Above,
                                    },
                                ));
                            }
                            if right_blank_right > table_right {
                                table_boundary_caret_areas_local.push((
                                    Bounds::new(
                                        point(px(table_right), px(table_top)),
                                        size(
                                            px(right_blank_right - table_right),
                                            px(layout.total_height),
                                        ),
                                    ),
                                    TableBoundaryCaret {
                                        table_start_line: ti.table_start_line,
                                        side: TableBoundarySide::Below,
                                    },
                                ));
                            }

                            if editor.table_boundary_caret.is_some_and(|boundary| {
                                boundary.table_start_line == ti.table_start_line
                            }) && window.is_window_active()
                                && editor.focus_handle.is_focused(window)
                                && editor.focused_sidebar_path.is_none()
                            {
                                let boundary = editor.table_boundary_caret.unwrap();
                                let (cursor_x, row_top, row_height) = match boundary.side {
                                    TableBoundarySide::Above => (
                                        (table_left - boundary_gap - cursor_width)
                                            .max(viewport_left + 1.0),
                                        table_top,
                                        first_row_height,
                                    ),
                                    TableBoundarySide::Below => (
                                        (table_right + boundary_gap)
                                            .min(viewport_right - cursor_width - 1.0),
                                        table_bottom - last_row_height,
                                        last_row_height,
                                    ),
                                };
                                cursor_paints.push(super::caret::CaretPaint {
                                    bounds: Bounds::new(
                                        point(
                                            px(cursor_x),
                                            px(row_top + (row_height - cursor_height) / 2.0),
                                        ),
                                        size(px(cursor_width), px(cursor_height)),
                                    ),
                                    color: theme.cursor,
                                    force_visible: false,
                                });
                            }
                        }

                        // Selection highlight: blue border when selection covers table
                        let table_block_start_offset = buffer.line_start(ti.table_start_line);
                        let table_block_end_offset =
                            buffer.line_end(ti.table_end_line.saturating_sub(1));
                        let selection_overlaps_table = all_sel_ranges.iter().any(|sr| {
                            sr.start != sr.end
                                && sr.start < table_block_end_offset
                                && sr.end > table_block_start_offset
                        });
                        let show_table_border = super::table_handles::handles_visible(
                            editor.active_tab_is_read_only(),
                            editor.hovering_table_start == Some(ti.table_start_line),
                            editor
                                .table_handles
                                .selection
                                .as_ref()
                                .is_some_and(|selection| {
                                    selection.table_start_line == ti.table_start_line
                                }),
                        );
                        let table_corners = if show_table_border {
                            Corners {
                                top_left: px(0.0),
                                top_right: px(0.0),
                                bottom_left: px(0.0),
                                bottom_right: outer_radius,
                            }
                        } else {
                            Corners::all(outer_radius)
                        };
                        if selection_overlaps_table {
                            let sel_border_w = table_styles.table_selection_border_width_px;
                            table_borders.push(quad(
                                Bounds::new(
                                    point(
                                        px(table_left - sel_border_w),
                                        px(table_top - sel_border_w),
                                    ),
                                    size(
                                        px(table_width + sel_border_w * 2.0),
                                        px(layout.total_height + sel_border_w * 2.0),
                                    ),
                                ),
                                table_corners,
                                gpui::transparent_black(),
                                px(sel_border_w),
                                theme.cursor,
                                BorderStyle::default(),
                            ));
                        }

                        // Draw outer table border once.
                        table_borders.push(quad(
                            Bounds::new(
                                point(px(table_left), px(table_top)),
                                size(px(table_width), px(layout.total_height)),
                            ),
                            table_corners,
                            gpui::transparent_black(),
                            px(border_w),
                            border_color,
                            BorderStyle::default(),
                        ));

                        // Draw internal horizontal separators once.
                        let mut row_boundary_y = table_top;
                        for row_h in layout
                            .row_heights
                            .iter()
                            .take(layout.visual_row_count.saturating_sub(1))
                        {
                            row_boundary_y += *row_h;
                            table_borders.push(fill(
                                Bounds::new(
                                    point(px(table_left), px(row_boundary_y)),
                                    size(px(table_width), px(border_w)),
                                ),
                                border_color,
                            ));
                        }

                        // Draw internal vertical separators once.
                        if let Some(first_row) = layout.cells.first() {
                            for cell in first_row.iter().take(layout.col_count.saturating_sub(1)) {
                                let separator_x = table_left + cell.x + cell.width;
                                table_borders.push(fill(
                                    Bounds::new(
                                        point(px(separator_x), px(table_top)),
                                        size(px(border_w), px(layout.total_height)),
                                    ),
                                    border_color,
                                ));
                            }
                        }

                        // Render ALL visual rows of the table block
                        for vi in 0..layout.visual_row_count {
                            let row_y = table_top + layout.row_heights.iter().take(vi).sum::<f32>();
                            let row_h = layout.row_heights.get(vi).copied().unwrap_or(line_height);

                            // Header background
                            if layout.is_header.get(vi).copied().unwrap_or(false) {
                                table_bgs.push(fill(
                                    Bounds::new(
                                        point(px(table_left), px(row_y)),
                                        size(px(table_width), px(row_h)),
                                    ),
                                    table_styles.table_header_bg,
                                ));
                            }

                            let border_selection =
                                editor.table_handles.selection.as_ref().filter(|selection| {
                                    selection.table_start_line == ti.table_start_line
                                });

                            // Get the source line info for this visual row
                            let source_line = layout
                                .visual_row_to_line
                                .get(vi)
                                .copied()
                                .unwrap_or(line_idx);
                            let row_ti = decorations
                                .get(source_line)
                                .and_then(|d| d.table_info.as_ref());
                            // Per-cell text shaping
                            if let Some(row_cells) = layout.cells.get(vi) {
                                for cell_layout in row_cells {
                                    let decoration_start = diff_inline_quads.len();
                                    let selection_start = selection_quads.len();
                                    let search_highlight_start = search_match_quads.len();
                                    if border_selection.as_ref().is_some_and(|selection| {
                                        (selection.is_column && selection.contains(cell_layout.col))
                                            || (!selection.is_column && selection.contains(vi))
                                    }) {
                                        let selection_bg =
                                            crate::theme::with_alpha(theme.cursor, 0.18);
                                        table_bgs.push(fill(
                                            Bounds::new(
                                                point(px(table_left + cell_layout.x), px(row_y)),
                                                size(px(cell_layout.width), px(row_h)),
                                            ),
                                            selection_bg,
                                        ));
                                    }
                                    if editor.table_cell_selection.as_ref().is_some_and(
                                        |selection| {
                                            selection.contains(
                                                ti.table_start_line,
                                                vi,
                                                cell_layout.col,
                                            )
                                        },
                                    ) {
                                        table_bgs.push(fill(
                                            Bounds::new(
                                                point(px(table_left + cell_layout.x), px(row_y)),
                                                size(px(cell_layout.width), px(row_h)),
                                            ),
                                            crate::theme::with_alpha(theme.cursor, 0.12),
                                        ));
                                    }
                                    let (cell_content, _) = row_ti
                                        .map(|info| {
                                            super::table_cell_render::content_with_range(
                                                info,
                                                buffer.line_content(source_line),
                                                cell_layout.col,
                                            )
                                        })
                                        .unwrap_or_default();
                                    let cell_source_start = buffer.line_start(source_line)
                                        + cell_layout.cell_range.start;
                                    let cell_source_end =
                                        buffer.line_start(source_line) + cell_layout.cell_range.end;

                                    let mut cell_decoration = helpers::table_cell_decoration(
                                        &decorations[source_line],
                                        &cell_content,
                                        &cell_layout.cell_range,
                                    );
                                    helpers::add_inline_math_display_replacements(
                                        &mut cell_decoration,
                                        &cell_content,
                                        image_cache,
                                        body_space_width,
                                    );

                                    let mut cell_active_ranges = helpers::table_cell_active_ranges(
                                        buffer.line_start(source_line),
                                        &cell_layout.cell_range,
                                        &table_syntax_focus,
                                        &cell_decoration,
                                    );
                                    let build_cell = |active: &[Range<usize>]| {
                                        let columns = super::table_cell_render::task_prefix_columns(
                                            base_char_width + letter_spacing,
                                            checkbox_size_px + letter_spacing.max(0.0),
                                            checkbox_text_gap_px,
                                        );
                                        let (raw, d2b, b2d, collapsed) =
                                            helpers::build_display_text(
                                                &cell_content,
                                                &cell_decoration,
                                                active,
                                                false,
                                                None,
                                                0,
                                                columns,
                                                columns,
                                                task_source_indent_step_chars,
                                                0,
                                                editor
                                                    .styles
                                                    .editor
                                                    .editor_superscript_scale_factor,
                                                editor.styles.editor.editor_subscript_scale_factor,
                                                editor
                                                    .styles
                                                    .editor
                                                    .editor_superscript_scale_factor,
                                            );
                                        let range = helpers::table_cell_display_range(
                                            &raw,
                                            &cell_content,
                                            &cell_decoration,
                                            &b2d,
                                            active,
                                        );
                                        let display = raw[range.clone()].to_owned();
                                        let d2b = d2b[range.clone()].to_vec();
                                        let b2d = b2d
                                            .iter()
                                            .map(|index| {
                                                index.saturating_sub(range.start).min(range.len())
                                            })
                                            .collect::<Vec<_>>();
                                        let mut cell_runs = helpers::build_text_runs_for_display(
                                            &display,
                                            &cell_content,
                                            &d2b,
                                            &cell_decoration,
                                            active,
                                            false,
                                            collapsed,
                                            &body_font,
                                            &body_cjk_font,
                                            &monospace_font,
                                            &heading_font,
                                            &heading_cjk_font,
                                            &italic_font,
                                            &italic_cjk_font,
                                            0,
                                            font_size,
                                            theme,
                                            editor.styles.selection.selection_diff_added_inline_bg,
                                            editor
                                                .styles
                                                .selection
                                                .selection_diff_removed_inline_bg,
                                            editor.styles.wiki_link.wiki_link_text_color,
                                            editor.styles.wiki_link.wiki_link_text_weight,
                                            editor.styles.editor.editor_list_marker_text_color,
                                            editor.styles.editor.editor_list_marker_font_weight,
                                            editor.styles.editor.editor_heading_marker_text_color,
                                            None,
                                            false,
                                        );
                                        helpers::apply_search_match_text_styles(
                                            &mut cell_runs,
                                            &display,
                                            &d2b,
                                            cell_source_start,
                                            &editor.search_matches,
                                            editor.search_current_index,
                                            editor.styles.search.search_match_text_color,
                                            editor.styles.search.search_match_active_text_color,
                                        );
                                        if editor.current_file_path
                                            == editor.workspace_search_highlight_path
                                        {
                                            helpers::apply_search_match_text_styles(
                                                &mut cell_runs,
                                                &display,
                                                &d2b,
                                                cell_source_start,
                                                &editor.workspace_search_highlight_ranges,
                                                editor.workspace_search_highlight_current_index,
                                                editor.styles.search.search_match_text_color,
                                                editor.styles.search.search_match_active_text_color,
                                            );
                                        }
                                        if editor.current_file_path
                                            == editor.outline_flash_highlight_path
                                        {
                                            helpers::apply_search_match_text_styles(
                                                &mut cell_runs,
                                                &display,
                                                &d2b,
                                                cell_source_start,
                                                &editor.outline_flash_highlight_ranges,
                                                editor.outline_flash_highlight_current_index,
                                                editor.styles.search.search_match_text_color,
                                                editor.styles.search.search_match_active_text_color,
                                            );
                                        }

                                        // Apply table/front-matter text styling
                                        let cell_runs =
                                            if layout.is_header.get(vi).copied().unwrap_or(false) {
                                                cell_runs
                                                    .into_iter()
                                                    .map(|mut r| {
                                                        r.font.weight = gpui::FontWeight::BOLD;
                                                        r
                                                    })
                                                    .collect()
                                            } else {
                                                cell_runs
                                            };

                                        (display, d2b, b2d, collapsed, cell_runs)
                                    };

                                    let cell_content_x =
                                        table_left + cell_layout.x + layout.cell_padding_x;
                                    let cell_text_y = row_y + layout.cell_padding_y;

                                    let wrap_width =
                                        (cell_layout.width - layout.cell_padding_x * 2.0).max(1.0);

                                    if let Ok(prepared) = super::table_cell_render::prepare_cell(
                                        window,
                                        &cell_content,
                                        &cell_decoration,
                                        &mut cell_active_ranges,
                                        wrap_width,
                                        font_size,
                                        letter_spacing,
                                        line_height,
                                        image_cache,
                                        build_cell,
                                    ) {
                                        let cell_runs = prepared.display.4;
                                        let mut hard_line_y = cell_text_y;
                                        let mut visual_row_start = 0;
                                        for (((shaped, hard_line_range), fragment_info), hanging) in
                                            prepared
                                                .lines
                                                .into_iter()
                                                .zip(prepared.hard_ranges)
                                                .zip(&cell_decoration.table_cell_fragments)
                                                .zip(prepared.wraps)
                                        {
                                            let visual_row_count =
                                                shaped.wrap_boundaries().len() + 1;
                                            let indent_px = hanging.indent;
                                            let hard_display_text = cell_layout
                                                .display_text
                                                .get(hard_line_range.clone())
                                                .unwrap_or("");
                                            let hard_cell_runs = helpers::extract_runs_for_range(
                                                &cell_runs,
                                                hard_line_range.start,
                                                hard_line_range.end,
                                            );
                                            let hard_buffer_to_display = cell_layout
                                                .buffer_to_display
                                                .iter()
                                                .map(|display_index| {
                                                    display_index
                                                        .saturating_sub(hard_line_range.start)
                                                        .min(hard_line_range.len())
                                                })
                                                .collect::<Vec<_>>();
                                            let cell_x = cell_content_x
                                                + helpers::table_cell_content_offset(
                                                    cell_layout.alignment,
                                                    wrap_width,
                                                    f32::from(shaped.unwrapped_layout.width),
                                                );
                                            let fragment_start = fragment_info.range.start;
                                            let fragment_end = fragment_info.range.end;
                                            let fragment =
                                                &cell_content[fragment_info.range.clone()];

                                            let fragment_buffer_to_display = cell_layout
                                                .buffer_to_display
                                                [fragment_start..=fragment_end]
                                                .iter()
                                                .map(|display_index| {
                                                    display_index
                                                        .saturating_sub(hard_line_range.start)
                                                        .min(hard_line_range.len())
                                                })
                                                .collect::<Vec<_>>();
                                            let fragment_prefix_collapsed =
                                                helpers::table_cell_fragment_prefix_is_folded(
                                                    &cell_content,
                                                    fragment_info,
                                                    &cell_active_ranges,
                                                );
                                            let mut ordered_marker = None;
                                            if fragment_prefix_collapsed {
                                                if fragment_info.block_type
                                                    == BlockType::OrderedList
                                                {
                                                    let leading = fragment.len()
                                                        - fragment
                                                            .trim_start_matches([' ', '\t'])
                                                            .len();
                                                    let prefix_len =
                                                        fragment_info.prefix_len - leading;
                                                    let prefix_start = fragment_start + leading;
                                                    let prefix_end = prefix_start + prefix_len;
                                                    if prefix_len > 0 {
                                                        let display_start =
                                                            helpers::buffer_to_display_offset(
                                                                &cell_layout.buffer_to_display,
                                                                prefix_start,
                                                            )
                                                            .saturating_sub(hard_line_range.start);
                                                        let marker_text = cell_content
                                                            [prefix_start..prefix_end]
                                                            .trim_end_matches([' ', '\t']);
                                                        let mut marker_font =
                                                            monospace_font.clone();
                                                        marker_font.style = gpui::FontStyle::Normal;
                                                        marker_font.weight = if layout
                                                            .is_header
                                                            .get(vi)
                                                            .copied()
                                                            .unwrap_or(false)
                                                        {
                                                            FontWeight::BOLD
                                                        } else {
                                                            editor
                                                                .styles
                                                                .editor
                                                                .editor_list_marker_font_weight
                                                        };
                                                        let run = TextRun {
                                                            len: marker_text.len(),
                                                            font: marker_font,
                                                            color: editor
                                                                .styles
                                                                .editor
                                                                .editor_list_marker_text_color,
                                                            background_color: None,
                                                            underline: None,
                                                            strikethrough: None,
                                                        };
                                                        if let Ok(mut markers) =
                                                            window.text_system().shape_text(
                                                                marker_text.to_owned().into(),
                                                                px(font_size),
                                                                &[run],
                                                                None,
                                                                None,
                                                            )
                                                            && let Some(mut marker) = markers.pop()
                                                            && let Some(position) = shaped
                                                                .position_for_index(
                                                                    display_start,
                                                                    px(line_height),
                                                                )
                                                        {
                                                            helpers::apply_letter_spacing(
                                                                &mut marker,
                                                                letter_spacing,
                                                            );
                                                            marker.set_row_heights(vec![
                                                                shaped
                                                                    .row_height(0, px(line_height)),
                                                            ]);
                                                            ordered_marker = Some((
                                                                marker,
                                                                point(
                                                                    px(cell_x) + position.x,
                                                                    px(hard_line_y) + position.y,
                                                                ),
                                                            ));
                                                        }
                                                    }
                                                }
                                                push_unordered_list_marker_paint(
                                                    editor,
                                                    fragment,
                                                    &fragment_info.block_type,
                                                    fragment_prefix_collapsed,
                                                    &shaped,
                                                    &fragment_buffer_to_display,
                                                    font_size,
                                                    line_height,
                                                    cell_x,
                                                    cell_x,
                                                    hard_line_y,
                                                    &monospace_font,
                                                    window,
                                                    &mut unordered_list_markers,
                                                );
                                                push_task_checkbox_paint(
                                                    editor,
                                                    &fragment_info.block_type,
                                                    fragment_prefix_collapsed,
                                                    hard_display_text,
                                                    &shaped,
                                                    font_size,
                                                    line_height,
                                                    cell_x,
                                                    hard_line_y,
                                                    hard_line_y,
                                                    f32::from(
                                                        shaped.row_height(0, px(line_height)),
                                                    ),
                                                    3,
                                                    fragment_buffer_to_display
                                                        .get(fragment_info.prefix_len)
                                                        .copied(),
                                                    checkbox_text_gap_px,
                                                    Some(source_line),
                                                    &mut gutter_quads,
                                                    &mut gutter_svgs,
                                                    &mut checkbox_hit_areas,
                                                );
                                            }
                                            for (ranges, scale, align, color) in [
                                                (
                                                    &cell_decoration.superscript_spans,
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_superscript_scale_factor,
                                                    InlineOverlayVerticalAlign::Top(
                                                        editor.scaled(
                                                            editor
                                                                .styles
                                                                .editor
                                                                .editor_superscript_offset_px,
                                                        ),
                                                    ),
                                                    theme.syntax_marker,
                                                ),
                                                (
                                                    &cell_decoration.subscript_spans,
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_subscript_scale_factor,
                                                    InlineOverlayVerticalAlign::Bottom(
                                                        editor.scaled(
                                                            editor
                                                                .styles
                                                                .editor
                                                                .editor_subscript_offset_px,
                                                        ),
                                                    ),
                                                    theme.syntax_marker,
                                                ),
                                            ] {
                                                for range in ranges {
                                                    if !helpers::inline_range_is_collapsed(
                                                        &cell_decoration,
                                                        range,
                                                        &cell_active_ranges,
                                                    ) {
                                                        continue;
                                                    }
                                                    let start = helpers::buffer_to_display_offset(
                                                        &cell_layout.buffer_to_display,
                                                        range.start,
                                                    );
                                                    let end = helpers::buffer_to_display_offset(
                                                        &cell_layout.buffer_to_display,
                                                        range.end,
                                                    );
                                                    if start < hard_line_range.start
                                                        || end > hard_line_range.end
                                                    {
                                                        continue;
                                                    }
                                                    push_inline_text_overlay(
                                                        &mut inline_overlays,
                                                        window,
                                                        &shaped,
                                                        cell_content
                                                            .get(range.clone())
                                                            .unwrap_or_default(),
                                                        start - hard_line_range.start
                                                            ..end - hard_line_range.start,
                                                        &body_font,
                                                        &body_cjk_font,
                                                        color,
                                                        font_size,
                                                        scale,
                                                        align,
                                                        false,
                                                        cell_x,
                                                        hard_line_y,
                                                        indent_px,
                                                        line_height,
                                                    );
                                                }
                                            }
                                            for reference in &cell_decoration.footnote_references {
                                                if !helpers::inline_range_is_collapsed(
                                                    &cell_decoration,
                                                    &reference.collapsed_range,
                                                    &cell_active_ranges,
                                                ) {
                                                    continue;
                                                }
                                                let start = helpers::buffer_to_display_offset(
                                                    &cell_layout.buffer_to_display,
                                                    reference.collapsed_range.start,
                                                );
                                                let end = helpers::buffer_to_display_offset(
                                                    &cell_layout.buffer_to_display,
                                                    reference.collapsed_range.end,
                                                );
                                                if start < hard_line_range.start
                                                    || end > hard_line_range.end
                                                {
                                                    continue;
                                                }
                                                push_inline_text_overlay(
                                                    &mut inline_overlays,
                                                    window,
                                                    &shaped,
                                                    cell_content
                                                        .get(reference.collapsed_range.clone())
                                                        .unwrap_or_default(),
                                                    start - hard_line_range.start
                                                        ..end - hard_line_range.start,
                                                    &body_font,
                                                    &body_cjk_font,
                                                    theme.link,
                                                    font_size,
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_superscript_scale_factor,
                                                    InlineOverlayVerticalAlign::Offset(
                                                        -line_height * 0.14,
                                                    ),
                                                    false,
                                                    cell_x,
                                                    hard_line_y,
                                                    indent_px,
                                                    line_height,
                                                );
                                            }
                                            push_inline_code_background_quads(
                                                &mut diff_inline_quads,
                                                &cell_decoration,
                                                hard_display_text,
                                                cell_content.len(),
                                                &hard_buffer_to_display,
                                                &hard_cell_runs,
                                                &shaped,
                                                cell_x,
                                                hard_line_y,
                                                line_height,
                                                font_size,
                                                indent_px,
                                                editor.styles.editor.editor_inline_code_bg,
                                                px(editor.scaled(
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_inline_code_bg_x_padding_px,
                                                )),
                                                px(editor.scaled(
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_inline_code_bg_radius_px,
                                                )),
                                                px(editor.scaled(
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_inline_code_bg_y_inset_px,
                                                )),
                                                letter_spacing,
                                                window,
                                            );
                                            if !cell_decoration.highlight_spans.is_empty() {
                                                push_diff_inline_highlight_quads(
                                                    &mut diff_inline_quads,
                                                    &cell_decoration.highlight_spans,
                                                    hard_display_text,
                                                    cell_content.len(),
                                                    &hard_buffer_to_display,
                                                    &hard_cell_runs,
                                                    &shaped,
                                                    cell_x,
                                                    hard_line_y,
                                                    line_height,
                                                    font_size,
                                                    indent_px,
                                                    editor.styles.editor.editor_inline_highlight_bg,
                                                    gpui::transparent_black(),
                                                    0.0,
                                                    px(editor.scaled(
                                                        editor
                                                            .styles
                                                            .editor
                                                            .editor_inline_code_bg_x_padding_px,
                                                    )),
                                                    px(editor.scaled(
                                                        editor
                                                            .styles
                                                            .editor
                                                            .editor_inline_code_bg_radius_px,
                                                    )),
                                                    px(editor.scaled(
                                                        editor
                                                            .styles
                                                            .editor
                                                            .editor_inline_code_bg_y_inset_px,
                                                    )),
                                                    letter_spacing,
                                                    window,
                                                );
                                            }
                                            if editor.command_held {
                                                let mut link_ranges = cell_decoration
                                                    .inline_spans
                                                    .iter()
                                                    .filter(|span| {
                                                        span.kind
                                                            == crate::markdown::InlineKind::Link
                                                    })
                                                    .map(|span| span.range.clone())
                                                    .collect::<Vec<_>>();
                                                for wiki in &cell_decoration.wiki_links {
                                                    if editor.markdown_target_is_navigable(
                                                        &wiki.target_path,
                                                    ) {
                                                        link_ranges.push(
                                                            if helpers::active_ranges_intersect(
                                                                &cell_active_ranges,
                                                                &wiki.full_range,
                                                            ) {
                                                                wiki.full_range.clone()
                                                            } else {
                                                                wiki.collapsed_range.clone()
                                                            },
                                                        );
                                                    }
                                                }
                                                for reference in
                                                    &cell_decoration.footnote_references
                                                {
                                                    if editor
                                                        .find_footnote_definition_line(
                                                            &reference.label,
                                                        )
                                                        .is_some()
                                                    {
                                                        link_ranges.push(
                                                            if helpers::active_ranges_intersect(
                                                                &cell_active_ranges,
                                                                &reference.full_range,
                                                            ) {
                                                                reference.full_range.clone()
                                                            } else {
                                                                reference.collapsed_range.clone()
                                                            },
                                                        );
                                                    }
                                                }
                                                for range in link_ranges {
                                                    let global_start = if range.start
                                                        >= cell_layout.buffer_to_display.len()
                                                    {
                                                        cell_layout.display_text.len()
                                                    } else {
                                                        helpers::buffer_to_display_offset(
                                                            &cell_layout.buffer_to_display,
                                                            range.start,
                                                        )
                                                    };
                                                    let global_end = if range.end
                                                        >= cell_layout.buffer_to_display.len()
                                                    {
                                                        cell_layout.display_text.len()
                                                    } else {
                                                        helpers::buffer_to_display_offset(
                                                            &cell_layout.buffer_to_display,
                                                            range.end,
                                                        )
                                                    };
                                                    let display_start = global_start
                                                        .saturating_sub(hard_line_range.start)
                                                        .min(hard_line_range.len());
                                                    let display_end = global_end
                                                        .saturating_sub(hard_line_range.start)
                                                        .min(hard_line_range.len());
                                                    for link_bounds in
                                                        helpers::build_highlight_bounds(
                                                            hard_display_text,
                                                            &hard_cell_runs,
                                                            &shaped,
                                                            display_start,
                                                            display_end,
                                                            cell_x,
                                                            hard_line_y,
                                                            line_height,
                                                            font_size,
                                                            indent_px,
                                                            0.0,
                                                            letter_spacing,
                                                            window,
                                                        )
                                                    {
                                                        if editor
                                                            .hovered_editor_position
                                                            .is_some_and(|position| {
                                                                link_bounds.contains(&position)
                                                            })
                                                        {
                                                            link_underlines.push(fill(
                                                                Bounds::new(
                                                                    point(
                                                                        link_bounds.origin.x,
                                                                        link_bounds.origin.y
                                                                            + link_bounds
                                                                                .size
                                                                                .height
                                                                            - px(2.0),
                                                                    ),
                                                                    size(
                                                                        link_bounds.size.width,
                                                                        px(1.5),
                                                                    ),
                                                                ),
                                                                theme.link,
                                                            ));
                                                        }
                                                        link_hitboxes.push(window.insert_hitbox(
                                                            link_bounds,
                                                            HitboxBehavior::Normal,
                                                        ));
                                                    }
                                                }
                                            }
                                            let mut push_search_highlights =
                                                |matches: &[std::ops::Range<usize>],
                                                 active_index: Option<usize>| {
                                                    let (match_base, cell_matches) =
                                                        helpers::matches_overlapping_range(
                                                            matches,
                                                            cell_source_start,
                                                            cell_source_end,
                                                        );
                                                    for (match_offset, search_match) in
                                                        cell_matches.iter().enumerate()
                                                    {
                                                        let match_index = match_base + match_offset;
                                                        let match_start = search_match
                                                            .start
                                                            .max(cell_source_start);
                                                        let match_end =
                                                            search_match.end.min(cell_source_end);
                                                        if match_start >= match_end {
                                                            continue;
                                                        }
                                                        push_diff_inline_highlight_quads(
                                                            &mut search_match_quads,
                                                            &[match_start - cell_source_start
                                                                ..match_end - cell_source_start],
                                                            hard_display_text,
                                                            cell_content.len(),
                                                            &hard_buffer_to_display,
                                                            &hard_cell_runs,
                                                            &shaped,
                                                            cell_x,
                                                            hard_line_y,
                                                            line_height,
                                                            font_size,
                                                            indent_px,
                                                            if active_index == Some(match_index) {
                                                                editor
                                                                    .styles
                                                                    .search
                                                                    .search_match_active_bg
                                                            } else {
                                                                editor.styles.search.search_match_bg
                                                            },
                                                            editor
                                                                .styles
                                                                .search
                                                                .search_match_border_color,
                                                            editor.scaled(
                                                                editor
                                                                    .styles
                                                                    .search
                                                                    .search_match_border_width_px,
                                                            ),
                                                            px(0.0),
                                                            px(3.0),
                                                            px((line_height - font_size).max(0.0)
                                                                * 0.25),
                                                            letter_spacing,
                                                            window,
                                                        );
                                                    }
                                                };
                                            push_search_highlights(
                                                &editor.search_matches,
                                                editor.search_current_index,
                                            );
                                            if editor.current_file_path
                                                == editor.workspace_search_highlight_path
                                            {
                                                push_search_highlights(
                                                    &editor.workspace_search_highlight_ranges,
                                                    editor.workspace_search_highlight_current_index,
                                                );
                                            }
                                            if editor.current_file_path
                                                == editor.outline_flash_highlight_path
                                            {
                                                push_search_highlights(
                                                    &editor.outline_flash_highlight_ranges,
                                                    editor.outline_flash_highlight_current_index,
                                                );
                                            }

                                            if let Some(cell_range) =
                                                row_ti.map(|_| &cell_layout.cell_range)
                                            {
                                                let line_start_off = buffer.line_start(source_line);
                                                let cell_buf_start =
                                                    line_start_off + cell_range.start;
                                                let cell_buf_end = line_start_off + cell_range.end;

                                                for sr in &all_sel_ranges {
                                                    if sr.start == sr.end {
                                                        continue;
                                                    }
                                                    let sel_start = sr.start.max(cell_buf_start);
                                                    let sel_end = sr.end.min(cell_buf_end);
                                                    if sel_start < sel_end {
                                                        let buf_start = sel_start - cell_buf_start;
                                                        let buf_end = sel_end - cell_buf_start;
                                                        let global_disp_start = if buf_start
                                                            >= cell_layout.buffer_to_display.len()
                                                        {
                                                            cell_layout.display_text.len()
                                                        } else {
                                                            helpers::buffer_to_display_offset(
                                                                &cell_layout.buffer_to_display,
                                                                buf_start,
                                                            )
                                                        };
                                                        let global_disp_end = if buf_end
                                                            >= cell_layout.buffer_to_display.len()
                                                        {
                                                            cell_layout.display_text.len()
                                                        } else {
                                                            helpers::buffer_to_display_offset(
                                                                &cell_layout.buffer_to_display,
                                                                buf_end,
                                                            )
                                                        };
                                                        let disp_start = global_disp_start
                                                            .saturating_sub(hard_line_range.start)
                                                            .min(hard_line_range.len());
                                                        let disp_end = global_disp_end
                                                            .saturating_sub(hard_line_range.start)
                                                            .min(hard_line_range.len());
                                                        if hard_display_text.is_empty()
                                                            && global_disp_start
                                                                <= hard_line_range.start
                                                            && global_disp_end
                                                                >= hard_line_range.start
                                                        {
                                                            selection_quads.push(fill(
                                                                Bounds::new(
                                                                    point(
                                                                        px(cell_x),
                                                                        px(hard_line_y),
                                                                    ),
                                                                    size(
                                                                        px((font_size * 0.35)
                                                                            .max(3.0)),
                                                                        px(line_height),
                                                                    ),
                                                                ),
                                                                theme.selection,
                                                            ));
                                                        }
                                                        selection_quads.extend(
                                                            hanging
                                                                .selection_bounds(
                                                                    &shaped,
                                                                    hard_display_text,
                                                                    disp_start..disp_end,
                                                                    point(
                                                                        px(cell_x),
                                                                        px(hard_line_y),
                                                                    ),
                                                                    line_height,
                                                                )
                                                                .into_iter()
                                                                .map(|bounds| {
                                                                    fill(bounds, theme.selection)
                                                                }),
                                                        );
                                                    }
                                                }

                                                // Measure even while search owns keyboard focus.
                                                if editor.selected_handle.is_none()
                                                    && !editor.table_boundary_caret.is_some_and(
                                                        |boundary| {
                                                            boundary.table_start_line
                                                                == ti.table_start_line
                                                        },
                                                    )
                                                {
                                                    {
                                                        for &cur_off in &all_cursor_offsets {
                                                            if cur_off >= cell_buf_start
                                                                && cur_off <= cell_buf_end
                                                            {
                                                                let in_cell =
                                                                    cur_off - cell_buf_start;
                                                                let global_disp_col = if in_cell
                                                                    >= cell_layout
                                                                        .buffer_to_display
                                                                        .len()
                                                                {
                                                                    cell_layout.display_text.len()
                                                                } else {
                                                                    helpers::buffer_to_display_offset(
                                                                    &cell_layout.buffer_to_display,
                                                                    in_cell,
                                                                )
                                                                };
                                                                let global_disp_col =
                                                                    global_disp_col.min(
                                                                        cell_layout
                                                                            .display_text
                                                                            .len(),
                                                                    );
                                                                if global_disp_col
                                                                    < hard_line_range.start
                                                                    || global_disp_col
                                                                        > hard_line_range.end
                                                                {
                                                                    continue;
                                                                }
                                                                let disp_col = global_disp_col
                                                                    .saturating_sub(
                                                                        hard_line_range.start,
                                                                    )
                                                                    .min(hard_line_range.len());
                                                                {
                                                                    let pos = hanging.position(
                                                                        &shaped, hard_display_text, disp_col, line_height,
                                                                        editor.wrap_boundary_affinity_for_offset(cur_off),
                                                                    );
                                                                    let caret_x =
                                                                        super::caret::text_caret_x(
                                                                            hard_display_text,
                                                                            disp_col,
                                                                            pos,
                                                                            &shaped
                                                                                .unwrapped_layout,
                                                                            window.text_system(),
                                                                            |index| {
                                                                                Some(hanging.position(
                                                                                    &shaped,
                                                                                    hard_display_text,
                                                                                    index,
                                                                                    line_height,
                                                                                    None,
                                                                                ))
                                                                            },
                                                                        );
                                                                    let row_height = f32::from(
                                                                        shaped.row_height(
                                                                            shaped.row_for_y(
                                                                                pos.y,
                                                                                px(line_height),
                                                                            ),
                                                                            px(line_height),
                                                                        ),
                                                                    );
                                                                    let cursor_height = row_height
                                                                        * editor
                                                                            .styles
                                                                            .cursor
                                                                            .cursor_height_factor
                                                                            .clamp(0.1, 1.0);
                                                                    let cursor_top = hard_line_y
                                                                        + f32::from(pos.y)
                                                                        + (row_height
                                                                            - cursor_height)
                                                                            / 2.0;
                                                                    let cursor_width = px(editor
                                                                        .styles
                                                                        .cursor
                                                                        .cursor_width_px);
                                                                    let cursor_bounds = Bounds::new(
                                                                        point(
                                                                            px(cell_x) + super::caret::centered_caret_left(
                                                                                caret_x, cursor_width,
                                                                            ),
                                                                            px(cursor_top),
                                                                        ),
                                                                        size(cursor_width, px(cursor_height)),
                                                                    );
                                                                    if cur_off
                                                                        == editor.cursor.offset
                                                                    {
                                                                        let row_top = hard_line_y
                                                                            + f32::from(pos.y);
                                                                        table_cursor_paint_bounds = Some(TableCursorGeometry::from_paint(
                                                                            cur_off,
                                                                            cursor_bounds,
                                                                            visual_row_start + shaped.row_for_y(pos.y, px(line_height)),
                                                                            row_top..row_top + row_height,
                                                                            origin_y,
                                                                            scroll_offset,
                                                                        ));
                                                                    }
                                                                    if window.is_window_active()
                                                                        && editor
                                                                            .focus_handle
                                                                            .is_focused(window)
                                                                        && editor
                                                                            .focused_sidebar_path
                                                                            .is_none()
                                                                    {
                                                                        cursor_paints.push(
                                                                            super::caret::CaretPaint {
                                                                                bounds: cursor_bounds,
                                                                                color: theme.cursor,
                                                                                force_visible: editor.is_selecting,
                                                                            },
                                                                        );
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            if editor.app_settings.show_whitespace
                                                && indent_px == 0.0
                                            {
                                                push_whitespace_dot_quads(
                                                    &mut whitespace_dots,
                                                    hard_display_text,
                                                    &shaped,
                                                    point(px(cell_x), px(hard_line_y)),
                                                    line_height,
                                                    editor.scaled(
                                                        editor
                                                            .styles
                                                            .editor
                                                            .editor_whitespace_dot_size_px,
                                                    ),
                                                    editor
                                                        .styles
                                                        .editor
                                                        .editor_whitespace_dot_color,
                                                    0,
                                                );
                                            }

                                            push_inline_math_images(
                                                &cell_decoration,
                                                &cell_content,
                                                hard_display_text,
                                                &cell_layout.buffer_to_display,
                                                &cell_active_ranges,
                                                hard_line_range.start,
                                                &shaped,
                                                image_cache,
                                                cell_x,
                                                hard_line_y,
                                                indent_px,
                                                line_height,
                                                Some(Bounds::new(
                                                    point(
                                                        px(table_left + cell_layout.x),
                                                        px(row_y),
                                                    ),
                                                    size(px(cell_layout.width), px(row_h)),
                                                )),
                                                &mut image_previews,
                                            );

                                            let cell_baseline = helpers::stable_text_baseline(
                                                &helpers::line_baseline_font(
                                                    &decorations[source_line],
                                                    false,
                                                    &body_font,
                                                    &heading_font,
                                                    &monospace_font,
                                                ),
                                                px(font_size),
                                                px(line_height),
                                                &mut text_baseline_cache,
                                                window,
                                            );
                                            if let Some((marker, origin)) = ordered_marker {
                                                let origin = helpers::baseline_aligned_text_origin(
                                                    &marker,
                                                    origin,
                                                    px(line_height),
                                                    cell_baseline,
                                                );
                                                table_cell_texts.push((
                                                    marker,
                                                    origin,
                                                    px(line_height),
                                                ));
                                            }
                                            let hard_height =
                                                f32::from(shaped.size(px(line_height)).height);
                                            if indent_px > 0.0
                                                && !shaped.wrap_boundaries().is_empty()
                                            {
                                                if let Ok(segments) =
                                                    super::table_cell_render::paint_segments(
                                                        window,
                                                        &shaped,
                                                        &hard_cell_runs,
                                                        hanging,
                                                        line_height,
                                                    )
                                                {
                                                    for (segment, offset) in segments {
                                                        let height =
                                                            segment.row_height(0, px(line_height));
                                                        let origin =
                                                            helpers::baseline_aligned_text_origin(
                                                                &segment,
                                                                point(px(cell_x), px(hard_line_y))
                                                                    + offset,
                                                                height,
                                                                cell_baseline
                                                                    + (height - px(line_height))
                                                                        / 2.0,
                                                            );
                                                        if editor.app_settings.show_whitespace {
                                                            push_whitespace_dot_quads(
                                                                &mut whitespace_dots, &segment.text, &segment,
                                                                point(px(cell_x), px(hard_line_y)) + offset,
                                                                f32::from(height),
                                                                editor.scaled(editor.styles.editor.editor_whitespace_dot_size_px),
                                                                editor.styles.editor.editor_whitespace_dot_color, 0,
                                                            );
                                                        }
                                                        table_cell_texts.push((
                                                            segment,
                                                            origin,
                                                            px(line_height),
                                                        ));
                                                    }
                                                }
                                            } else {
                                                let paint_origin =
                                                    helpers::baseline_aligned_text_origin(
                                                        &shaped,
                                                        point(px(cell_x), px(hard_line_y)),
                                                        px(line_height),
                                                        cell_baseline,
                                                    );
                                                table_cell_texts.push((
                                                    shaped,
                                                    paint_origin,
                                                    px(line_height),
                                                ));
                                            }
                                            visual_row_start += visual_row_count;
                                            hard_line_y += hard_height;
                                        }
                                    }

                                    let cell_bounds = Bounds::new(
                                        point(px(table_left + cell_layout.x), px(row_y)),
                                        size(px(cell_layout.width), px(row_h)),
                                    );
                                    if diff_inline_quads.len() > decoration_start {
                                        clipped_table_decorations.push((
                                            cell_bounds,
                                            diff_inline_quads.split_off(decoration_start),
                                        ));
                                    }
                                    if selection_quads.len() > selection_start {
                                        clipped_table_selections.push((
                                            cell_bounds,
                                            selection_quads.split_off(selection_start),
                                        ));
                                    }
                                    if search_match_quads.len() > search_highlight_start {
                                        clipped_table_search_highlights.push((
                                            cell_bounds,
                                            search_match_quads.split_off(search_highlight_start),
                                        ));
                                    }

                                    // Register cell click area
                                    let click_bounds = Bounds::new(
                                        point(px(table_left + cell_layout.x), px(row_y)),
                                        size(px(cell_layout.width), px(row_h)),
                                    );
                                    table_click_areas_local.push((
                                        click_bounds,
                                        ti.table_start_line,
                                        vi,
                                        cell_layout.col,
                                    ));
                                }
                            }

                            let handle_w = layout.handle_size;
                            let handle_bounds = Bounds::new(
                                point(px(table_left - handle_w), px(row_y)),
                                size(px(handle_w), px(row_h)),
                            );
                            if !editor.active_tab_is_read_only() {
                                table_row_handle_areas_local.push((
                                    handle_bounds,
                                    ti.table_start_line,
                                    vi,
                                ));
                            }
                        }

                        {
                            let handle_h = layout.handle_size;
                            let mut hx = 0.0_f32;
                            for ci in 0..layout.col_count {
                                let cell_w = layout
                                    .cells
                                    .first()
                                    .and_then(|row| row.get(ci))
                                    .map(|c| c.width)
                                    .unwrap_or(0.0);
                                let handle_bounds = Bounds::new(
                                    point(px(table_left + hx), px(table_top - handle_h)),
                                    size(px(cell_w), px(handle_h)),
                                );
                                if !editor.active_tab_is_read_only() {
                                    table_col_handle_areas_local.push((
                                        handle_bounds,
                                        ti.table_start_line,
                                        ci,
                                    ));
                                }
                                hx += cell_w;
                            }
                        }

                        let hovering_table_border_segment = editor.hovering_handle;
                        let border_selection =
                            editor.table_handles.selection.as_ref().filter(|selection| {
                                selection.table_start_line == ti.table_start_line
                            });
                        let border_width_px =
                            editor.scaled(table_styles.table_border_track_width_px);
                        let outer_radius_px = table_styles.table_outer_radius_px;
                        if show_table_border {
                            let neutral = gpui::Hsla {
                                a: 0.25,
                                ..table_styles.table_border_color
                            };
                            let active_color = gpui::Hsla {
                                a: 0.3,
                                ..theme.cursor
                            };
                            if let Some(path) = table_border_track_path(
                                table_left,
                                table_top,
                                table_left + table_width,
                                table_top + layout.total_height,
                                border_width_px,
                                outer_radius_px,
                            ) {
                                table_border_paths.push((path, neutral));
                            }
                            let mut hx = 0.0_f32;
                            for ci in 0..layout.col_count {
                                let cell_w = layout
                                    .cells
                                    .first()
                                    .and_then(|row| row.get(ci))
                                    .map(|cell| cell.width)
                                    .unwrap_or(0.0);
                                let active = hovering_table_border_segment
                                    == Some((ti.table_start_line, true, ci))
                                    || border_selection.as_ref().is_some_and(|selection| {
                                        selection.is_column && selection.contains(ci)
                                    });
                                if active {
                                    table_borders.push(quad(
                                        Bounds::new(
                                            point(
                                                px(table_left + hx),
                                                px(table_top - border_width_px),
                                            ),
                                            size(px(cell_w), px(border_width_px)),
                                        ),
                                        Corners::all(px(0.0)),
                                        active_color,
                                        px(0.0),
                                        gpui::transparent_black(),
                                        BorderStyle::default(),
                                    ));
                                }
                                hx += cell_w;
                            }
                            for vi in 0..layout.visual_row_count {
                                let row_y =
                                    table_top + layout.row_heights.iter().take(vi).sum::<f32>();
                                let row_h =
                                    layout.row_heights.get(vi).copied().unwrap_or(line_height);
                                let active = hovering_table_border_segment
                                    == Some((ti.table_start_line, false, vi))
                                    || border_selection.as_ref().is_some_and(|selection| {
                                        !selection.is_column && selection.contains(vi)
                                    });
                                if active {
                                    table_borders.push(quad(
                                        Bounds::new(
                                            point(px(table_left - border_width_px), px(row_y)),
                                            size(px(border_width_px), px(row_h)),
                                        ),
                                        Corners::all(px(0.0)),
                                        active_color,
                                        px(0.0),
                                        gpui::transparent_black(),
                                        BorderStyle::default(),
                                    ));
                                }
                            }
                        }
                        if let Some(selection) = editor.table_cell_selection.as_ref()
                            && selection.table_start_line == ti.table_start_line
                            && let (Some(first), Some(last)) = (
                                layout
                                    .cells
                                    .first()
                                    .and_then(|cells| cells.get(*selection.columns.start())),
                                layout
                                    .cells
                                    .first()
                                    .and_then(|cells| cells.get(*selection.columns.end())),
                            )
                            && *selection.rows.end() < layout.visual_row_count
                        {
                            let top = table_top
                                + layout
                                    .row_heights
                                    .iter()
                                    .take(*selection.rows.start())
                                    .sum::<f32>();
                            let bottom = table_top
                                + layout
                                    .row_heights
                                    .iter()
                                    .take(*selection.rows.end() + 1)
                                    .sum::<f32>();
                            table_borders.push(quad(
                                Bounds::new(
                                    point(px(table_left + first.x), px(top)),
                                    size(px(last.x + last.width - first.x), px(bottom - top)),
                                ),
                                Corners::all(px(0.0)),
                                gpui::transparent_black(),
                                px(editor.scaled(2.0)),
                                theme.cursor,
                                BorderStyle::default(),
                            ));
                        }
                        if let Some(drag) = &editor.table_drag {
                            let indicator_color = theme.cursor;
                            let indicator_width = table_styles.table_drag_indicator_width_px;
                            match drag {
                                TableDragState::DraggingColumn {
                                    source_indices,
                                    table_start_line,
                                    current_x,
                                    ..
                                } if *table_start_line == ti.table_start_line => {
                                    let mut boundaries = Vec::with_capacity(layout.col_count + 1);
                                    boundaries.push(table_left);
                                    if let Some(first_row) = layout.cells.first() {
                                        for cell in first_row {
                                            boundaries.push(table_left + cell.x + cell.width);
                                        }
                                    }
                                    if let Some(&indicator_x) = boundaries.iter().min_by(|a, b| {
                                        (*a - *current_x)
                                            .abs()
                                            .partial_cmp(&(*b - *current_x).abs())
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    }) {
                                        let insert_at = boundaries
                                            .iter()
                                            .position(|boundary| {
                                                (*boundary - indicator_x).abs() < 0.5
                                            })
                                            .unwrap_or(0);
                                        let source_start =
                                            source_indices.iter().min().copied().unwrap_or(0);
                                        let source_end =
                                            source_indices.iter().max().copied().unwrap_or(0);
                                        if insert_at < source_start || insert_at > source_end + 1 {
                                            table_borders.push(fill(
                                                Bounds::new(
                                                    point(
                                                        px(indicator_x - indicator_width / 2.0),
                                                        px(table_top),
                                                    ),
                                                    size(
                                                        px(indicator_width),
                                                        px(layout.total_height),
                                                    ),
                                                ),
                                                indicator_color,
                                            ));
                                        }
                                    }
                                }
                                TableDragState::DraggingRow {
                                    source_indices,
                                    table_start_line,
                                    current_y,
                                    ..
                                } if *table_start_line == ti.table_start_line => {
                                    let mut boundaries =
                                        Vec::with_capacity(layout.visual_row_count + 1);
                                    boundaries.push(table_top);
                                    let mut y_cursor = table_top;
                                    for row_height in &layout.row_heights {
                                        y_cursor += *row_height;
                                        boundaries.push(y_cursor);
                                    }
                                    if let Some(&indicator_y) = boundaries.iter().min_by(|a, b| {
                                        (*a - *current_y)
                                            .abs()
                                            .partial_cmp(&(*b - *current_y).abs())
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    }) {
                                        let insert_at = boundaries
                                            .iter()
                                            .position(|boundary| {
                                                (*boundary - indicator_y).abs() < 0.5
                                            })
                                            .unwrap_or(0);
                                        let source_start =
                                            source_indices.iter().min().copied().unwrap_or(0);
                                        let source_end =
                                            source_indices.iter().max().copied().unwrap_or(0);
                                        if insert_at < source_start || insert_at > source_end + 1 {
                                            table_borders.push(fill(
                                                Bounds::new(
                                                    point(
                                                        px(table_left),
                                                        px(indicator_y - indicator_width / 2.0),
                                                    ),
                                                    size(px(table_width), px(indicator_width)),
                                                ),
                                                indicator_color,
                                            ));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // HR line: `***` renders dashed, `---`/`___` render solid.
            if *block_type == BlockType::HorizontalRule
                && !editor.search_reveals_line(line_idx)
                && (!is_cursor_line || !syntax_expansion_allowed)
            {
                let hr_y = text_line_top + text_line_height / 2.0;
                let is_dashed = line_content.trim_start().starts_with('*');
                hr_lines.push((
                    hr_y,
                    editor_left + editor.padding_left(),
                    text_width,
                    is_dashed,
                ));
            }

            // Image preview
            if !editor.folded_image_previews.contains(&line_idx) {
                if let Some(ref path) = decoration.image_path {
                    if let Some(Some(img_data)) = image_cache.get(path) {
                        let img_size = img_data.size(0);
                        let img_w = img_size.width.0 as f32;
                        let img_h = img_size.height.0 as f32;
                        if img_w > 0.0 && img_h > 0.0 {
                            let (display_w, display_h) = helpers::compute_image_preview_size(
                                img_w,
                                img_h,
                                text_width,
                                window_scale_factor,
                            );
                            // Preview sits above the syntax text within the
                            // line box reserved by `image_extra_height`.
                            let img_top = text_line_top - image_extra_height + image_gap_top;
                            let img_left = editor_left
                                + editor.padding_left()
                                + ((text_width - display_w) / 2.0).max(0.0);
                            let img_bounds = Bounds::new(
                                point(px(img_left), px(img_top)),
                                size(px(display_w), px(display_h)),
                            );
                            image_previews.push((
                                img_bounds,
                                img_data.clone(),
                                image_corner_radius,
                                None,
                            ));
                            image_hit_areas.push((img_bounds, line_idx));
                            let image_zoom_visible = image_active
                                || editor.hovering_image_preview_line == Some(line_idx);
                            if image_zoom_visible {
                                image_zoom_button_targets.push((img_bounds, line_idx));
                            }
                            if image_zoom_visible {
                                image_borders.push(quad(
                                    img_bounds,
                                    Corners::all(image_corner_radius),
                                    gpui::transparent_black(),
                                    image_border_width,
                                    image_styles.image_active_border_color,
                                    BorderStyle::default(),
                                ));
                            }
                        }
                    }
                }
            }

            if let Some(preview) = rendered_block_preview {
                if let Some(Some(img_data)) = image_cache.get(&preview.cache_key) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0
                        && img_h > 0.0
                        && let Some((fitted_w, fitted_h, _bg_width, bg_height)) =
                            generated_preview_layout
                    {
                        let overlay_open = editor
                            .preview_overlay_state()
                            .is_some_and(|overlay| overlay.line_idx == line_idx);
                        let pad_x =
                            editor.scaled(editor.styles.code_block.generated_preview_padding_x_px);
                        let pad_y =
                            editor.scaled(editor.styles.code_block.generated_preview_padding_y_px);
                        let block_left = editor_left + editor.padding_left();
                        let block_height = bg_height.max(text_line_height);
                        let preview_top = line_top;
                        let preview_left = block_left;
                        let preview_width = text_width;
                        let view = editor.generated_preview_view(&preview.cache_key);
                        let zoom = view.zoom_factor.clamp(0.25, 6.0);
                        let draw_w = fitted_w * zoom;
                        let draw_h = fitted_h * zoom;
                        // The background spans the full text column; the chart is
                        // centred inside it with padding (vertical for Mermaid via
                        // fill-height, both axes for math). Zooming in overflows
                        // and the clip below keeps it within the padded viewport.
                        let img_left = preview_left + (preview_width - draw_w) * 0.5 + view.pan_x;
                        let img_top = preview_top + (block_height - draw_h) * 0.5 + view.pan_y;
                        let img_bounds = Bounds::new(
                            point(px(img_left), px(img_top)),
                            size(px(draw_w), px(draw_h)),
                        );
                        // Clip to the full background, not the padded inner box,
                        // so the padding is just the default centred spacing and
                        // the chart stays visible all the way to the background
                        // edge while being dragged.
                        let visible_img_bounds = Bounds::new(
                            point(px(preview_left), px(preview_top)),
                            size(px(preview_width.max(1.0)), px(block_height.max(1.0))),
                        );
                        let _ = (pad_x, pad_y);
                        let hit_bounds = Bounds::new(
                            point(px(preview_left), px(line_top)),
                            size(px(preview_width.max(1.0)), px(block_height.max(1.0))),
                        );
                        // Generated math/Mermaid previews are cropped tightly to
                        // their glyph ink, so a rounded-corner clip would cut into
                        // real glyphs (e.g. the descenders and the `²` superscript
                        // in E=mc²). Keep square corners for these previews.
                        let clip_bounds = Some(visible_img_bounds);
                        image_previews.push((img_bounds, img_data.clone(), px(0.0), clip_bounds));
                        generated_preview_hit_areas.push((hit_bounds, preview.start_line));
                        generated_preview_image_areas.push((hit_bounds, preview.start_line));
                        let _ = overlay_open;
                    }
                }
            }

            // Presentation can move the control, never the source fold anchor.
            let front_matter_fold_anchor =
                (front_matter_fold_control_line == Some(line_idx)).then_some(0);
            let fold_anchor = front_matter_fold_anchor.unwrap_or(line_idx);
            let fold_range = if hidden_lines.contains(&line_idx)
                || super::fold::front_matter_opener_hides_fold_control(
                    &editor.decorations,
                    line_idx,
                    front_matter_fold_control_line,
                ) {
                None
            } else if front_matter_fold_anchor.is_some() {
                editor.fold_range_for_line(0)
            } else {
                editor.fold_range_for_line(line_idx)
            };
            if editor.editor_uses_code_gutter() {
                let gutter_styles = &editor.styles.gutter;
                push_gutter_line_number_label(
                    &mut gutter_labels,
                    window,
                    line_idx + 1,
                    is_active_line_number,
                    &monospace_font,
                    editor.scaled(gutter_styles.gutter_line_number_font_size_px),
                    gutter_right_x,
                    text_line_top,
                    line_height,
                    editor.scaled(gutter_styles.gutter_line_number_vertical_offset_px),
                    editor.scaled(gutter_styles.gutter_line_number_right_gap_px),
                    gutter_styles.gutter_line_number_text_color,
                    gutter_styles.gutter_line_number_active_text_color,
                );
            }

            if let Some(callout_kind) = render_decoration.callout_kind
                && let Some(replacement) =
                    render_decoration
                        .display_replacements
                        .iter()
                        .find(|replacement| {
                            line_content
                                .get(replacement.source_range.clone())
                                .is_some_and(|source| source.trim_start().starts_with("[!"))
                        })
                && !helpers::active_ranges_intersect(&active_ranges, &replacement.trigger_range)
            {
                let disp_start = helpers::buffer_to_display_offset(
                    &buffer_to_display,
                    replacement.source_range.start,
                );
                let disp_end = (disp_start
                    + callout_icon_placeholder_len(&replacement.display_text))
                .min(display_text.len());
                if let (Some(pos_start), Some(pos_end)) = (
                    wrapped.position_for_index(disp_start, px(line_height)),
                    wrapped.position_for_index(disp_end, px(line_height)),
                ) && pos_start.y == pos_end.y
                {
                    let indent_start = helpers::indent_for_y(indent_px, pos_start.y, line_height);
                    // Left-align the callout icon (no left gap) and size it from
                    // quote block style tokens only.
                    let quote_block_styles = &editor.styles.quote_block;
                    let callout_gap =
                        editor.scaled(quote_block_styles.quote_block_callout_icon_label_gap_px);
                    let display_width = f32::from(pos_end.x - pos_start.x);
                    let available_icon_width = (display_width - callout_gap).max(0.0);
                    if available_icon_width <= 0.0 {
                        continue;
                    }
                    let icon_size = callout_icon_size(
                        available_icon_width,
                        font_size,
                        line_height,
                        quote_block_styles.quote_block_callout_icon_size_factor,
                        editor.scaled(quote_block_styles.quote_block_callout_icon_min_px),
                        quote_block_styles.quote_block_callout_icon_max_line_height_factor,
                    );
                    let icon_x = content_x + indent_start + f32::from(pos_start.x);
                    let row_height = f32::from(wrapped.row_height(
                        wrapped.row_for_y(pos_start.y, px(line_height)),
                        px(line_height),
                    ));
                    let icon_y =
                        text_line_top + f32::from(pos_start.y) + (row_height - icon_size) / 2.0;
                    inline_svgs.push((
                        Bounds::new(
                            point(px(icon_x), px(icon_y)),
                            size(px(icon_size), px(icon_size)),
                        ),
                        callout_icon_path(callout_kind),
                        editor.styles.quote_block.quote_block_callout_icon_color,
                    ));
                }
            }

            if *block_type != BlockType::Table || active_tab_is_diff {
                push_inline_math_images(
                    render_decoration,
                    line_content,
                    &display_text,
                    &buffer_to_display,
                    &active_ranges,
                    0,
                    wrapped,
                    image_cache,
                    content_x,
                    text_line_top,
                    indent_px,
                    line_height,
                    None,
                    &mut image_previews,
                );
            }

            // Checkbox painting
            push_task_checkbox_paint(
                editor,
                block_type,
                prefix_collapsed,
                &display_text,
                wrapped,
                font_size,
                line_height,
                content_x,
                text_line_top,
                line_top,
                total_line_height,
                collapsed_task_prefix_display_columns,
                None,
                checkbox_text_gap_px,
                Some(line_idx),
                &mut gutter_quads,
                &mut gutter_svgs,
                &mut checkbox_hit_areas,
            );

            push_front_matter_boolean_paint(
                editor,
                render_decoration,
                line_content,
                &active_ranges,
                &display_text,
                &buffer_to_display,
                wrapped,
                font_size,
                line_height,
                content_x,
                text_line_top,
                indent_px,
                line_idx,
                &mut gutter_quads,
                &mut gutter_svgs,
                &mut checkbox_hit_areas,
            );

            // Gutter fold icon
            if !hidden_lines.contains(&line_idx) {
                let gs = &editor.styles.gutter;
                let fi_size = editor.scaled(gs.gutter_fold_icon_size_px);
                let fi_offset = editor.scaled(gs.gutter_fold_icon_offset_px);
                let fi_hit_padding = editor.scaled(gs.gutter_fold_hit_padding_px);
                let markdown_fold_left_padding = if matches!(block_type, BlockType::Heading(_)) {
                    editor.scaled(editor.styles.format_toolbar.line_format_heading_icon_gap_px)
                        * 0.5
                } else {
                    editor.scaled(LINE_FORMAT_ICON_GAP_PX) * 0.5
                };
                let format_icon_size = editor.scaled(LINE_FORMAT_ICON_SIZE_PX);
                let format_icon_x = line_format_trigger_left(
                    editor_left + editor.padding_left(),
                    format_icon_size,
                    Some(fi_size),
                    editor.scaled(LINE_FORMAT_ICON_GAP_PX),
                    editor.scaled(LINE_FORMAT_CONTENT_GAP_PX),
                );
                let fold_icon_expanded_x = if editor.editor_uses_code_gutter() {
                    editor.gutter_icon_x(editor_left, fi_size)
                } else {
                    format_icon_x + format_icon_size + editor.scaled(LINE_FORMAT_ICON_GAP_PX)
                };

                if fold_range.is_some() {
                    let is_folded = decoration
                        .block_preview
                        .as_ref()
                        .filter(|preview| preview.is_anchor)
                        .is_some_and(|preview| {
                            editor.folded_sections.contains(&preview.start_line)
                        })
                        || editor.folded_sections.contains(&fold_anchor);
                    let fold_icon_x = fold_icon_expanded_x;
                    let is_hovered = if editor.editor_uses_code_gutter() {
                        editor.hovering_fold_line == Some(line_idx)
                            || gutter_fold_row_hovered(
                                editor.hovering_gutter_fold_area,
                                window.mouse_position().y,
                                line_top,
                                total_line_height,
                            )
                    } else {
                        editor.format_trigger_line == Some(property_line)
                    };
                    let fi_y = text_line_top
                        + (f32::from(wrapped.row_height(0, px(line_height))) - fi_size) / 2.0;
                    let reveal = editor.gutter_fold_reveal.max(0.0);
                    let fold_hovered = editor.hovering_fold_line == Some(fold_anchor);
                    let fi_alpha = if !editor.editor_uses_code_gutter() {
                        markdown_fold_opacity(
                            fold_hovered,
                            is_hovered,
                            editor.format_trigger_opacity,
                            if is_folded && front_matter_fold_anchor.is_none() {
                                gs.gutter_fold_folded_opacity
                            } else {
                                0.0
                            },
                        )
                    } else if is_folded {
                        gs.gutter_fold_folded_opacity
                    } else if is_hovered {
                        gs.gutter_fold_hover_opacity * reveal
                    } else {
                        0.0
                    };
                    if fi_alpha > 0.0 {
                        let icon_bounds = Bounds::new(
                            point(px(fold_icon_x), px(fi_y)),
                            size(px(fi_size), px(fi_size)),
                        );
                        let mut fi_color = if !editor.editor_uses_code_gutter() && is_hovered {
                            editor.styles.format_toolbar.format_toolbar_icon_color
                        } else if is_folded {
                            gs.gutter_fold_folded_color
                        } else {
                            gs.gutter_fold_hover_color
                        };
                        fi_color.a = fi_alpha;
                        gutter_fold_icons.push((
                            icon_bounds,
                            if is_folded {
                                FOLD_SVG_PATH
                            } else {
                                FOLD_EXPANDED_SVG_PATH
                            },
                            fi_color,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        if editor.editor_uses_code_gutter() {
                            Bounds::new(
                                point(px(gutter_icon_column_left - fi_hit_padding), px(line_top)),
                                size(
                                    px(gutter_icon_column_width + fi_hit_padding * 2.0),
                                    px(total_line_height),
                                ),
                            )
                        } else if front_matter_fold_anchor.is_some() {
                            front_matter_fold_hit_bounds(
                                fold_icon_x - markdown_fold_left_padding,
                                editor_left + editor.padding_left(),
                                line_top,
                                total_line_height,
                                bounds.intersect(&window.content_mask().bounds),
                            )
                        } else {
                            Bounds::new(
                                point(px(fold_icon_x - markdown_fold_left_padding), px(line_top)),
                                size(
                                    px(fi_size
                                        + markdown_fold_left_padding
                                        + fi_hit_padding
                                        + fi_offset),
                                    px(total_line_height),
                                ),
                            )
                        },
                        fold_anchor,
                        false,
                    ));
                }

                // Image preview fold icon
                if decoration
                    .image_path
                    .as_ref()
                    .is_some_and(|p| image_cache.get(p).is_some_and(|v| v.is_some()))
                {
                    let is_img_folded = editor.folded_image_previews.contains(&line_idx);
                    let fold_icon_x = fold_icon_expanded_x;
                    let is_hovered = if editor.active_tab_is_non_markdown_text() {
                        editor.hovering_fold_line == Some(line_idx)
                            || gutter_fold_row_hovered(
                                editor.hovering_gutter_fold_area,
                                window.mouse_position().y,
                                line_top,
                                total_line_height,
                            )
                    } else {
                        editor.format_trigger_line == Some(line_idx)
                    };
                    // Align the fold icon with the top of the rendered preview
                    // (painted above the syntax text); fall back to the syntax
                    // row when the preview is folded or not loaded.
                    let fi_y = if image_extra_height > 0.0 {
                        text_line_top - image_extra_height + image_gap_top
                    } else {
                        text_line_top
                            + (f32::from(wrapped.row_height(0, px(line_height))) - fi_size) / 2.0
                    };
                    let gs = &editor.styles.gutter;
                    let reveal = editor.gutter_fold_reveal.max(0.0);
                    let fold_hovered = editor.hovering_fold_line == Some(line_idx);
                    let fi_alpha = if !editor.active_tab_is_non_markdown_text() && fold_hovered {
                        selection_toolbar::FORMAT_TRIGGER_HOVER_OPACITY
                    } else if !editor.active_tab_is_non_markdown_text() && is_hovered {
                        editor.format_trigger_opacity
                    } else if is_img_folded {
                        gs.gutter_fold_folded_opacity
                    } else if is_hovered {
                        gs.gutter_fold_hover_opacity * reveal
                    } else {
                        0.0
                    };
                    if fi_alpha > 0.0 {
                        let icon_bounds = Bounds::new(
                            point(px(fold_icon_x), px(fi_y)),
                            size(px(fi_size), px(fi_size)),
                        );
                        let mut fi_color =
                            if !editor.active_tab_is_non_markdown_text() && is_hovered {
                                editor.styles.format_toolbar.format_toolbar_icon_color
                            } else if is_img_folded {
                                gs.gutter_fold_folded_color
                            } else {
                                gs.gutter_fold_hover_color
                            };
                        fi_color.a = fi_alpha;
                        gutter_fold_icons.push((
                            icon_bounds,
                            if is_img_folded {
                                FOLD_SVG_PATH
                            } else {
                                FOLD_EXPANDED_SVG_PATH
                            },
                            fi_color,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        if editor.active_tab_is_non_markdown_text() {
                            Bounds::new(
                                point(px(gutter_icon_column_left - fi_hit_padding), px(line_top)),
                                size(
                                    px(gutter_icon_column_width + fi_hit_padding * 2.0),
                                    px(total_line_height),
                                ),
                            )
                        } else {
                            Bounds::new(
                                point(px(fold_icon_x - markdown_fold_left_padding), px(line_top)),
                                size(
                                    px(fi_size
                                        + markdown_fold_left_padding
                                        + fi_hit_padding
                                        + fi_offset),
                                    px(total_line_height),
                                ),
                            )
                        },
                        line_idx,
                        true,
                    ));
                }
            }

            // Image URL-edit icon
            if image_active && !editor.active_tab_is_read_only() {
                let ie_size = editor.scaled(13.0);
                // Follow the syntax to its true end, including the wrapped row
                // it lands on, so folding/expanding never strands the icon.
                let syntax_end = wrapped
                    .position_for_index(display_text.len(), px(line_height))
                    .unwrap_or_default();
                let ie_x = content_x + f32::from(syntax_end.x) + editor.scaled(10.0);
                let row_height = f32::from(wrapped.row_height(
                    wrapped.row_for_y(syntax_end.y, px(line_height)),
                    px(line_height),
                ));
                let ie_y = text_line_top + f32::from(syntax_end.y) + (row_height - ie_size) / 2.0;
                let ie_bounds =
                    Bounds::new(point(px(ie_x), px(ie_y)), size(px(ie_size), px(ie_size)));
                let is_hovered = editor.hovering_image_edit_line == Some(line_idx);
                let alpha = if is_hovered { 0.55 } else { 0.7 };
                image_edit_icons.push((ie_bounds, line_idx, alpha));
                image_edit_hit_areas_local.push((ie_bounds, line_idx));
                editor_icon_tooltips.push((
                    ie_bounds,
                    SharedString::from(editor.t(crate::i18n::TextKey::EditUrl)),
                ));
            }

            // Line highlight
            let skip_block_line_highlight = decoration
                .table_info
                .as_ref()
                .is_some_and(|ti| !ti.is_front_matter && line_idx == ti.table_start_line);
            if let Some(diff_bg) = diff_line_background(editor, line_idx) {
                diff_line_bgs.push(fill(
                    Bounds::new(
                        point(px(origin_x), px(line_top)),
                        size(px(viewport_width), px(total_line_height)),
                    ),
                    diff_bg,
                ));
            }
            let selection_styles = &editor.styles.selection;
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff) {
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.diff_removed_spans,
                    &display_text,
                    line_content.len(),
                    &buffer_to_display,
                    &runs,
                    wrapped,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    indent_px,
                    selection_styles.selection_diff_removed_inline_bg,
                    selection_styles.selection_diff_removed_inline_border_color,
                    editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                    px(0.0),
                    px(0.0),
                    px(0.0),
                    letter_spacing,
                    window,
                );
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.diff_added_spans,
                    &display_text,
                    line_content.len(),
                    &buffer_to_display,
                    &runs,
                    wrapped,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    indent_px,
                    selection_styles.selection_diff_added_inline_bg,
                    selection_styles.selection_diff_added_inline_border_color,
                    editor.scaled(selection_styles.selection_diff_inline_border_width_px),
                    px(0.0),
                    px(0.0),
                    px(0.0),
                    letter_spacing,
                    window,
                );
            }
            // Read-only phantom rows: an expanded inline-diff hunk's removed
            // lines render in the padding band reserved by `block_padding_for_line`
            // (above the anchor line, or below the last line for a trailing
            // deletion). Their HEAD decorations are projected into the current
            // document mode. They soft-wrap like live lines, but never enter
            // the buffer. The painted band is top-aligned at the reserved height
            // so the anchor's geometry stays consistent; the measured height is
            // recorded for the next frame.
            {
                let above_rows = editor.inline_diff_phantom_removed_above(line_idx);
                let below_rows = editor.inline_diff_phantom_removed_below(line_idx);
                if !above_rows.is_empty() || !below_rows.is_empty() {
                    let phantom_content_x = editor_left + editor.padding_left();
                    let phantom_border_width =
                        editor.scaled(selection_styles.selection_diff_inline_border_width_px);
                    let mut render_band = |rows: &[(usize, usize, InlineDiffRemovedLine)],
                                           band_top: f32|
                     -> f32 {
                        let mut row_top = band_top;
                        for (hunk_base_start, row_in_hunk, removed) in rows {
                            let projected_decoration = super::inline_diff::phantom_row_decoration(
                                &removed.decoration,
                                editor.source_mode,
                            );
                            let r_decoration = projected_decoration.as_ref();
                            let mut r_active_ranges = Vec::new();
                            helpers::expand_source_syntax(
                                editor.source_mode,
                                removed.text.len(),
                                &mut r_active_ranges,
                            );
                            let r_font_size = editor.font_size_for_block(&r_decoration.block_type);
                            let r_line_height =
                                editor.line_height_for_block(&r_decoration.block_type);
                            let row_is_front_matter = r_decoration
                                .table_info
                                .as_ref()
                                .is_some_and(|table_info| table_info.is_front_matter);
                            let row_is_block_syntax = matches!(
                                r_decoration.block_type,
                                BlockType::CodeBlock | BlockType::CodeBlockFence
                            ) || row_is_front_matter;
                            let mut row_content_x = if row_is_block_syntax {
                                phantom_content_x
                                    + editor.scaled(
                                        editor.styles.code_block.code_block_text_padding_left_px,
                                    )
                            } else {
                                phantom_content_x
                            };
                            let row_wrap_width = if row_is_block_syntax {
                                (wrap_width
                                    - editor.scaled(
                                        editor.styles.code_block.code_block_text_padding_left_px,
                                    )
                                    - editor.scaled(
                                        editor.styles.code_block.code_block_wrap_right_padding_px,
                                    ))
                                .max(editor.scaled(80.0))
                            } else {
                                wrap_width
                            };
                            let preview = r_decoration.block_preview.as_ref();
                            if preview.is_some_and(|preview| !preview.is_anchor) {
                                continue;
                            }
                            let preview_image = preview
                                .filter(|preview| preview.is_anchor)
                                .and_then(|preview| {
                                    image_cache
                                        .get(&preview.cache_key)
                                        .and_then(|image| image.as_ref())
                                        .map(|image| (preview, image.clone()))
                                });
                            let (
                                mut display_text,
                                display_to_buffer,
                                buffer_to_display,
                                prefix_collapsed,
                            ) = if preview_image.is_some() {
                                (
                                    String::new(),
                                    Vec::new(),
                                    vec![0; removed.text.len().saturating_add(1)],
                                    false,
                                )
                            } else {
                                helpers::build_display_text(
                                    &removed.text,
                                    r_decoration,
                                    &r_active_ranges,
                                    false,
                                    None,
                                    table_cell_padding_chars,
                                    collapsed_task_prefix_display_columns,
                                    checkbox_indent_step_display_columns,
                                    editor.styles.checkbox.checkbox_parent_prefix_chars as usize,
                                    0,
                                    editor.styles.editor.editor_superscript_scale_factor,
                                    editor.styles.editor.editor_subscript_scale_factor,
                                    editor.styles.editor.editor_superscript_scale_factor,
                                )
                            };
                            editor.apply_list_display_styles(
                                &removed.text,
                                r_decoration,
                                &mut display_text,
                                prefix_collapsed,
                            );
                            let runs = if display_text.is_empty() {
                                vec![TextRun {
                                    len: 1,
                                    font: body_font.clone(),
                                    color: gpui::transparent_black(),
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                }]
                            } else {
                                helpers::build_text_runs_for_display_with_language(
                                    &display_text,
                                    &removed.text,
                                    &display_to_buffer,
                                    r_decoration,
                                    &r_active_ranges,
                                    false,
                                    prefix_collapsed,
                                    &body_font,
                                    &body_cjk_font,
                                    &monospace_font,
                                    &heading_font,
                                    &heading_cjk_font,
                                    &italic_font,
                                    &italic_cjk_font,
                                    collapsed_task_prefix_display_columns,
                                    r_font_size,
                                    theme,
                                    selection_styles.selection_diff_added_inline_bg,
                                    selection_styles.selection_diff_removed_inline_bg,
                                    editor.styles.wiki_link.wiki_link_text_color,
                                    editor.styles.wiki_link.wiki_link_text_weight,
                                    editor.styles.editor.editor_list_marker_text_color,
                                    editor.styles.editor.editor_list_marker_font_weight,
                                    editor.styles.editor.editor_heading_marker_text_color,
                                    row_is_front_matter
                                        .then_some(editor.styles.table.front_matter_key_text_color),
                                    false,
                                    None,
                                )
                            };
                            let shared: SharedString = if display_text.is_empty() {
                                " ".into()
                            } else {
                                display_text.clone().into()
                            };
                            let Ok(shaped) = window.text_system().shape_text(
                                shared,
                                px(r_font_size),
                                &runs,
                                Some(px(row_wrap_width)),
                                None,
                            ) else {
                                row_top += r_line_height;
                                continue;
                            };
                            let Some(wrapped) = shaped.into_iter().next() else {
                                row_top += r_line_height;
                                continue;
                            };
                            let visual_lines =
                                helpers::wrap_boundary_byte_indices(&wrapped, &display_text)
                                    .len()
                                    .saturating_add(1);
                            if matches!(r_decoration.block_type, BlockType::TaskList(_))
                                && prefix_collapsed
                            {
                                let prefix_bytes = helpers::compute_display_prefix_bytes(
                                    &display_text,
                                    &r_decoration.block_type,
                                    prefix_collapsed,
                                    collapsed_task_prefix_display_columns,
                                )
                                .min(display_text.len());
                                let indent_px = wrapped
                                    .position_for_index(prefix_bytes, px(r_line_height))
                                    .map(|pos| f32::from(pos.x))
                                    .unwrap_or(0.0);
                                let target_prefix_width = helpers::task_prefix_target_width_px(
                                    &removed.text,
                                    r_font_size * editor.styles.checkbox.checkbox_size_factor,
                                    checkbox_text_gap_px,
                                    task_source_indent_step_chars,
                                )
                                .unwrap_or(indent_px);
                                row_content_x += target_prefix_width - indent_px;
                            }
                            let text_height = visual_lines as f32 * r_line_height;
                            let row_height = if let Some((preview, img_data)) = preview_image {
                                let img_size = img_data.size(0);
                                let img_w = img_size.width.0 as f32;
                                let img_h = img_size.height.0 as f32;
                                if img_w > 0.0 && img_h > 0.0 {
                                    let (display_w, full_display_h, _, display_h) =
                                        helpers::fit_generated_preview_size(
                                            img_w,
                                            img_h,
                                            preview.kind,
                                            text_width,
                                            editor.scaled(
                                                editor
                                                    .styles
                                                    .code_block
                                                    .generated_preview_min_height_px,
                                            ),
                                            editor.scaled(
                                                editor
                                                    .styles
                                                    .code_block
                                                    .generated_preview_max_height_px,
                                            ),
                                            0.0,
                                            0.0,
                                        );
                                    let preview_top = row_top + text_height + image_gap_top;
                                    let img_left = if preview.kind
                                        == crate::markdown::BlockPreviewKind::Mermaid
                                    {
                                        phantom_content_x + (text_width - display_w) * 0.5
                                    } else {
                                        phantom_content_x
                                            + ((text_width - display_w) / 2.0).max(0.0)
                                    };
                                    let img_top = preview_top + (display_h - full_display_h) * 0.5;
                                    let img_bounds = Bounds::new(
                                        point(px(img_left), px(img_top)),
                                        size(px(display_w), px(full_display_h)),
                                    );
                                    let visible_img_bounds = Bounds::new(
                                        point(px(phantom_content_x), px(preview_top)),
                                        size(px(text_width.max(1.0)), px(display_h.max(1.0))),
                                    );
                                    image_previews.push((
                                        img_bounds,
                                        img_data.clone(),
                                        px(0.0),
                                        Some(visible_img_bounds),
                                    ));
                                    text_height
                                        + display_h
                                        + image_gap_top
                                        + r_line_height
                                        + image_gap_top
                                } else {
                                    text_height
                                }
                            } else {
                                text_height
                            };
                            push_task_checkbox_paint(
                                editor,
                                &r_decoration.block_type,
                                prefix_collapsed,
                                &display_text,
                                &wrapped,
                                r_font_size,
                                r_line_height,
                                row_content_x,
                                row_top,
                                row_top,
                                row_height,
                                collapsed_task_prefix_display_columns,
                                None,
                                checkbox_text_gap_px,
                                None,
                                &mut gutter_quads,
                                &mut gutter_svgs,
                                &mut checkbox_hit_areas,
                            );
                            push_unordered_list_marker_paint(
                                editor,
                                &removed.text,
                                &r_decoration.block_type,
                                prefix_collapsed,
                                &wrapped,
                                &buffer_to_display,
                                r_font_size,
                                r_line_height,
                                row_content_x,
                                phantom_content_x,
                                row_top,
                                &monospace_font,
                                window,
                                &mut unordered_list_markers,
                            );
                            push_rendered_inline_overlays(
                                editor,
                                window,
                                r_decoration,
                                &removed.text,
                                &display_text,
                                &buffer_to_display,
                                &[],
                                &wrapped,
                                &body_font,
                                &body_cjk_font,
                                theme,
                                r_font_size,
                                row_content_x,
                                row_top,
                                0.0,
                                r_line_height,
                                &mut inline_svgs,
                                &mut inline_overlays,
                            );
                            let row_bounds = Bounds::new(
                                point(px(origin_x), px(row_top)),
                                size(px(viewport_width), px(row_height)),
                            );
                            diff_line_bgs.push(fill(
                                row_bounds,
                                selection_styles.selection_diff_removed_line_bg,
                            ));
                            // Record geometry so a later mouse event can hit-test
                            // this soft-wrapped removed row down to a byte offset.
                            phantom_row_hits_local.push(PhantomRowHit {
                                bounds: row_bounds,
                                hunk_base_start: *hunk_base_start,
                                row: *row_in_hunk,
                                content_x: row_content_x,
                                row_top,
                                line_height: r_line_height,
                                wrapped: wrapped.clone(),
                                text_len: removed.text.len(),
                                display_to_buffer: display_to_buffer.clone(),
                            });
                            // Read-only character-level selection + caret.
                            if let Some(sel) = editor.phantom_selection
                                && sel.hunk_base_start == *hunk_base_start
                            {
                                let text_len = removed.text.len();
                                let clamp_off = |mut off: usize| -> usize {
                                    off = off.min(text_len);
                                    while off > 0 && !removed.text.is_char_boundary(off) {
                                        off -= 1;
                                    }
                                    off
                                };
                                if !sel.is_collapsed() {
                                    let (sr, so, er, eo) = sel.ordered();
                                    if *row_in_hunk >= sr && *row_in_hunk <= er {
                                        let raw_start =
                                            if *row_in_hunk == sr { clamp_off(so) } else { 0 };
                                        let raw_end = if *row_in_hunk == er {
                                            clamp_off(eo)
                                        } else {
                                            text_len
                                        };
                                        let disp_start = raw_offset_to_display_offset(
                                            &buffer_to_display,
                                            raw_start,
                                        )
                                        .min(display_text.len());
                                        let disp_end = raw_offset_to_display_offset(
                                            &buffer_to_display,
                                            raw_end,
                                        )
                                        .min(display_text.len());
                                        if disp_start < disp_end {
                                            selection_quads.extend(helpers::build_selection_quads(
                                                &display_text,
                                                &runs,
                                                &wrapped,
                                                disp_start,
                                                disp_end,
                                                row_content_x,
                                                row_top,
                                                r_line_height,
                                                r_font_size,
                                                0.0,
                                                0.0,
                                                theme.selection,
                                                letter_spacing,
                                                window,
                                            ));
                                        }
                                    }
                                }
                                if sel.head_row == *row_in_hunk
                                    && window.is_window_active()
                                    && editor.focus_handle.is_focused(window)
                                {
                                    let head_off = clamp_off(sel.head_offset);
                                    let display_head =
                                        raw_offset_to_display_offset(&buffer_to_display, head_off)
                                            .min(display_text.len());
                                    let caret_pos = wrapped
                                        .position_for_index(display_head, px(r_line_height))
                                        .unwrap_or_default();
                                    let caret_x = super::caret::text_caret_x(
                                        &display_text,
                                        display_head,
                                        caret_pos,
                                        &wrapped.unwrapped_layout,
                                        window.text_system(),
                                        |index| {
                                            wrapped.position_for_index(index, px(r_line_height))
                                        },
                                    );
                                    let caret_height = r_line_height
                                        * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
                                    let caret_top = row_top
                                        + f32::from(caret_pos.y)
                                        + (r_line_height - caret_height) / 2.0;
                                    let caret_width = px(editor.styles.cursor.cursor_width_px);
                                    cursor_paints.push(super::caret::CaretPaint {
                                        bounds: Bounds::new(
                                            point(
                                                px(row_content_x)
                                                    + super::caret::centered_caret_left(
                                                        caret_x,
                                                        caret_width,
                                                    ),
                                                px(caret_top),
                                            ),
                                            size(caret_width, px(caret_height)),
                                        ),
                                        color: theme.cursor,
                                        force_visible: editor.phantom_selecting,
                                    });
                                }
                            }
                            if !removed.removed_spans.is_empty() {
                                push_diff_inline_highlight_quads(
                                    &mut diff_inline_quads,
                                    &removed.removed_spans,
                                    &display_text,
                                    removed.text.len(),
                                    &buffer_to_display,
                                    &runs,
                                    &wrapped,
                                    row_content_x,
                                    row_top,
                                    r_line_height,
                                    r_font_size,
                                    0.0,
                                    selection_styles.selection_diff_removed_inline_bg,
                                    selection_styles.selection_diff_removed_inline_border_color,
                                    phantom_border_width,
                                    px(0.0),
                                    px(0.0),
                                    px(0.0),
                                    letter_spacing,
                                    window,
                                );
                            }
                            let row_baseline = helpers::stable_text_baseline(
                                &helpers::line_baseline_font(
                                    r_decoration,
                                    false,
                                    &body_font,
                                    &heading_font,
                                    &monospace_font,
                                ),
                                px(r_font_size),
                                px(r_line_height),
                                &mut text_baseline_cache,
                                window,
                            );
                            let paint_origin = helpers::baseline_aligned_text_origin(
                                &wrapped,
                                point(px(row_content_x), px(row_top)),
                                px(r_line_height),
                                row_baseline,
                            );
                            phantom_text_lines.push((wrapped, paint_origin, px(r_line_height)));
                            row_top += row_height;
                        }
                        row_top - band_top
                    };
                    if !above_rows.is_empty() {
                        let reserved = editor.inline_diff_phantom_reserved_above(line_idx);
                        let measured = render_band(&above_rows, text_line_top - reserved);
                        phantom_above_heights.insert(line_idx, measured);
                    }
                    if !below_rows.is_empty() {
                        let reserved = editor.inline_diff_phantom_reserved_below(line_idx);
                        let measured =
                            render_band(&below_rows, line_top + total_line_height - reserved);
                        phantom_below_heights.insert(line_idx, measured);
                    }
                }
            }
            let inline_decoration_start = diff_inline_quads.len();
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff) {
                push_inline_code_background_quads(
                    &mut diff_inline_quads,
                    &decoration,
                    &display_text,
                    line_content.len(),
                    &buffer_to_display,
                    &runs,
                    wrapped,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    indent_px,
                    editor.styles.editor.editor_inline_code_bg,
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_x_padding_px)),
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_radius_px)),
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_y_inset_px)),
                    letter_spacing,
                    window,
                );
            }
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff)
                && !decoration.highlight_spans.is_empty()
            {
                push_diff_inline_highlight_quads(
                    &mut diff_inline_quads,
                    &decoration.highlight_spans,
                    &display_text,
                    line_content.len(),
                    &buffer_to_display,
                    &runs,
                    wrapped,
                    content_x,
                    text_line_top,
                    line_height,
                    font_size,
                    indent_px,
                    editor.styles.editor.editor_inline_highlight_bg,
                    gpui::transparent_black(),
                    0.0,
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_x_padding_px)),
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_radius_px)),
                    px(editor.scaled(editor.styles.editor.editor_inline_code_bg_y_inset_px)),
                    letter_spacing,
                    window,
                );
            }
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff)
                && !selection_match_ranges.is_empty()
            {
                let mut line_spans: Vec<std::ops::Range<usize>> = Vec::new();
                for range in &selection_match_ranges {
                    if range.end > line_start_offset && range.start < line_end_offset {
                        let start = range.start.max(line_start_offset) - line_start_offset;
                        let end = range.end.min(line_end_offset) - line_start_offset;
                        if start < end {
                            line_spans.push(start..end);
                        }
                    }
                }
                if !line_spans.is_empty() {
                    push_diff_inline_highlight_quads(
                        &mut diff_inline_quads,
                        &line_spans,
                        &display_text,
                        line_content.len(),
                        &buffer_to_display,
                        &runs,
                        wrapped,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        editor.styles.editor.editor_selection_match_bg,
                        editor.styles.editor.editor_selection_match_border,
                        editor.scaled(editor.styles.editor.editor_match_border_width_px),
                        px(0.0),
                        px(0.0),
                        px(0.0),
                        letter_spacing,
                        window,
                    );
                }
            }
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff)
                && !bracket_match_ranges.is_empty()
            {
                let mut line_spans: Vec<std::ops::Range<usize>> = Vec::new();
                for range in &bracket_match_ranges {
                    if range.end > line_start_offset && range.start < line_end_offset {
                        let start = range.start.max(line_start_offset) - line_start_offset;
                        let end = range.end.min(line_end_offset) - line_start_offset;
                        if start < end {
                            line_spans.push(start..end);
                        }
                    }
                }
                if !line_spans.is_empty() {
                    push_diff_inline_highlight_quads(
                        &mut diff_inline_quads,
                        &line_spans,
                        &display_text,
                        line_content.len(),
                        &buffer_to_display,
                        &runs,
                        wrapped,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        gpui::transparent_black(),
                        editor.styles.editor.editor_bracket_match_border,
                        editor.scaled(editor.styles.editor.editor_match_border_width_px),
                        px(0.0),
                        px(0.0),
                        px(0.0),
                        letter_spacing,
                        window,
                    );
                }
            }
            if let Some(clip_bounds) = nowrap_clip_bounds
                && diff_inline_quads.len() > inline_decoration_start
            {
                clipped_code_block_decorations.push((
                    clip_bounds,
                    diff_inline_quads.split_off(inline_decoration_start),
                ));
            }
            if editor.selected_handle.is_none()
                && is_cursor_line
                && !any_selection
                && editor.extra_cursors.is_empty()
                && !skip_block_line_highlight
                && editor.app_settings.highlight_line_background
                && !editor.embedded_editor
            {
                let highlight_vertical_inset = if matches!(block_type, BlockType::Heading(_)) {
                    f32::from(cursor_aligned_vertical_inset(editor, line_height))
                } else {
                    0.0
                };
                let highlight_top = if matches!(block_type, BlockType::Heading(_)) {
                    text_line_top + highlight_vertical_inset
                } else {
                    line_top
                };
                let highlight_height = if matches!(block_type, BlockType::Heading(_)) {
                    (line_height - highlight_vertical_inset * 2.0).max(0.0)
                } else {
                    total_line_height
                };
                line_highlight = Some(fill(
                    Bounds::new(
                        point(px(origin_x), px(highlight_top)),
                        size(px(viewport_width), px(highlight_height)),
                    ),
                    theme.line_highlight,
                ));
            }

            // Real Markdown tables render selection per cell above. Running the
            // ordinary folded-line path as well selects synthetic inter-cell
            // padding and produces duplicate blocks around cell boundaries.
            let selection_start = selection_quads.len();
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff) {
                // Selection quads — iterate over all cursors (primary + extra)
                for sr in &all_sel_ranges {
                    if sr.start == sr.end {
                        continue;
                    }
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);

                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                        let disp_end =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                        let selection_spans_line =
                            sr.start < line_start_offset || sr.end > line_end_offset;
                        let selection_leading_padding_left_px =
                            if matches!(
                                block_type,
                                BlockType::CodeBlock | BlockType::CodeBlockFence
                            ) || decoration.block_preview.is_some()
                                || is_front_matter_line
                            {
                                0.0
                            } else {
                                selection_padding_left_px
                            };
                        if matches!(block_type, BlockType::Heading(_)) && selection_spans_line {
                            push_cursor_aligned_highlight_quads(
                                &mut selection_quads,
                                editor,
                                &display_text,
                                &runs,
                                wrapped,
                                disp_start,
                                disp_end,
                                content_x,
                                text_line_top,
                                line_height,
                                font_size,
                                indent_px,
                                selection_leading_padding_left_px,
                                theme.selection,
                                letter_spacing,
                                window,
                            );
                        } else {
                            selection_quads.extend(helpers::build_selection_quads(
                                &display_text,
                                &runs,
                                wrapped,
                                disp_start,
                                disp_end,
                                content_x,
                                text_line_top,
                                line_height,
                                font_size,
                                indent_px,
                                selection_leading_padding_left_px,
                                theme.selection,
                                letter_spacing,
                                window,
                            ));
                        }
                    }
                    if sr.end > line_end_offset && sr.start <= line_end_offset {
                        let disp_len = display_text.len();
                        let pos_end = wrapped
                            .position_for_index(disp_len, px(line_height))
                            .unwrap_or_default();
                        let indent_end = helpers::indent_for_y(indent_px, pos_end.y, line_height);
                        selection_quads.push(fill(
                            Bounds::from_corners(
                                point(
                                    px(content_x + indent_end) + pos_end.x,
                                    px(text_line_top) + pos_end.y,
                                ),
                                point(
                                    px(content_x + indent_end)
                                        + pos_end.x
                                        + px(editor.base_font_size() * 0.4),
                                    px(text_line_top)
                                        + pos_end.y
                                        + wrapped.row_height(
                                            wrapped.row_for_y(pos_end.y, px(line_height)),
                                            px(line_height),
                                        ),
                                ),
                            ),
                            theme.selection,
                        ));
                    }
                }
            }
            if let Some(clip_bounds) = nowrap_clip_bounds
                && selection_quads.len() > selection_start
            {
                clipped_code_block_selections
                    .push((clip_bounds, selection_quads.split_off(selection_start)));
            }

            // Search match quads
            let search_highlight_start = search_match_quads.len();
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff)
                && !editor.search_matches.is_empty()
            {
                let match_bg = editor.styles.search.search_match_bg;
                let active_bg = editor.styles.search.search_match_active_bg;
                let border_color = editor.styles.search.search_match_border_color;
                let border_width = editor.scaled(editor.styles.search.search_match_border_width_px);
                let search_line_top = text_line_top;
                for (match_idx, sr) in editor.search_matches.iter().enumerate() {
                    let is_active = editor.search_current_index == Some(match_idx);
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);
                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        push_diff_inline_highlight_quads(
                            &mut search_match_quads,
                            &[buf_start_col..buf_end_col],
                            &display_text,
                            line_content.len(),
                            &buffer_to_display,
                            &runs,
                            wrapped,
                            content_x,
                            search_line_top,
                            line_height,
                            font_size,
                            indent_px,
                            if is_active { active_bg } else { match_bg },
                            border_color,
                            border_width,
                            px(0.0),
                            px(3.0),
                            px((line_height - font_size).max(0.0) * 0.25),
                            letter_spacing,
                            window,
                        );
                    }
                }
            }

            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff)
                && editor.current_file_path == editor.workspace_search_highlight_path
            {
                let match_bg = editor.styles.search.search_match_bg;
                let active_bg = editor.styles.search.search_match_active_bg;
                for (match_idx, sr) in editor.workspace_search_highlight_ranges.iter().enumerate() {
                    let is_active =
                        editor.workspace_search_highlight_current_index == Some(match_idx);
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);
                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                        let disp_end =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                        push_cursor_aligned_highlight_quads(
                            &mut search_match_quads,
                            editor,
                            &display_text,
                            &runs,
                            wrapped,
                            disp_start,
                            disp_end,
                            content_x,
                            text_line_top,
                            line_height,
                            font_size,
                            indent_px,
                            0.0,
                            if is_active { active_bg } else { match_bg },
                            letter_spacing,
                            window,
                        );
                    }
                }
            }
            if !uses_table_cell_selection(block_type, &decoration, active_tab_is_diff)
                && editor.current_file_path == editor.outline_flash_highlight_path
            {
                let match_bg = editor.styles.search.search_match_bg;
                let active_bg = editor.styles.search.search_match_active_bg;
                for (match_idx, sr) in editor.outline_flash_highlight_ranges.iter().enumerate() {
                    let is_active = editor.outline_flash_highlight_current_index == Some(match_idx);
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);
                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                        let disp_end =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                        push_cursor_aligned_highlight_quads(
                            &mut search_match_quads,
                            editor,
                            &display_text,
                            &runs,
                            wrapped,
                            disp_start,
                            disp_end,
                            content_x,
                            text_line_top,
                            line_height,
                            font_size,
                            indent_px,
                            0.0,
                            if is_active { active_bg } else { match_bg },
                            letter_spacing,
                            window,
                        );
                    }
                }
            }
            if let Some(clip_bounds) = nowrap_clip_bounds
                && search_match_quads.len() > search_highlight_start
            {
                clipped_code_block_search_highlights.push((
                    clip_bounds,
                    search_match_quads.split_off(search_highlight_start),
                ));
            }

            // Cursor bars — paint for all cursors (primary + extra)
            // Skip for table lines — table cursor is rendered within the table block
            if editor.selected_handle.is_none()
                && editor.phantom_selection.is_none()
                && (*block_type != BlockType::Table || is_front_matter_line)
                && window.is_window_active()
                && editor.focus_handle.is_focused(window)
                && editor.focused_sidebar_path.is_none()
            {
                for &cur_offset in &all_cursor_offsets {
                    if cur_offset < line_start_offset || cur_offset > line_end_offset {
                        continue;
                    }
                    // For line_end_offset, only match if this is the actual last offset
                    // (handles cursor at end of line)
                    if cur_offset == line_end_offset && cur_offset != line_start_offset {
                        // cursor is at line end — show it here
                    }
                    let buf_cursor_col = cur_offset.saturating_sub(line_start_offset);
                    let buf_cursor_col = buf_cursor_col.min(line_content.len());
                    let disp_cursor_col = if buf_cursor_col >= line_content.len() {
                        display_text.len()
                    } else {
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_cursor_col)
                    };

                    let cursor_pos = wrapped
                        .position_for_index(disp_cursor_col, px(line_height))
                        .unwrap_or_default();
                    let cursor_pos = match editor.wrap_boundary_affinity {
                        Some(WrapBoundaryAffinity {
                            offset: affinity_offset,
                            kind: WrapBoundaryAffinityKind::Upstream,
                        }) if affinity_offset == cur_offset => upstream_wrap_boundary_position(
                            wrapped,
                            &display_text,
                            disp_cursor_col,
                            &wrap_boundary_upstream_xs,
                            line_height,
                        )
                        .unwrap_or(cursor_pos),
                        Some(WrapBoundaryAffinity {
                            offset: affinity_offset,
                            kind: WrapBoundaryAffinityKind::Downstream,
                        }) if affinity_offset == cur_offset => downstream_wrap_boundary_position(
                            wrapped,
                            &display_text,
                            disp_cursor_col,
                            line_height,
                        )
                        .unwrap_or(cursor_pos),
                        _ => cursor_pos,
                    };
                    let cursor_x = super::caret::text_caret_x(
                        &display_text,
                        disp_cursor_col,
                        cursor_pos,
                        &wrapped.unwrapped_layout,
                        window.text_system(),
                        |index| wrapped.position_for_index(index, px(line_height)),
                    );

                    let cursor_indent = helpers::indent_for_y(indent_px, cursor_pos.y, line_height);
                    let row_height = f32::from(wrapped.row_height(
                        wrapped.row_for_y(cursor_pos.y, px(line_height)),
                        px(line_height),
                    ));
                    let cursor_height =
                        row_height * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
                    let cursor_top = text_line_top
                        + f32::from(cursor_pos.y)
                        + (row_height - cursor_height) / 2.0;
                    let cursor_width = px(editor.styles.cursor.cursor_width_px);
                    let cursor_bounds = Bounds::new(
                        point(
                            px(content_x + cursor_indent)
                                + super::caret::centered_caret_left(cursor_x, cursor_width),
                            px(cursor_top),
                        ),
                        size(cursor_width, px(cursor_height)),
                    );
                    let cursor_left = f32::from(cursor_bounds.origin.x);
                    let cursor_right = cursor_left + f32::from(cursor_bounds.size.width);
                    let cursor_is_visible = nowrap_clip_bounds.is_none_or(|clip_bounds| {
                        let clip_left = f32::from(clip_bounds.origin.x);
                        let clip_right = clip_left + f32::from(clip_bounds.size.width);
                        cursor_right > clip_left && cursor_left < clip_right
                    });
                    if cursor_is_visible {
                        cursor_paints.push(super::caret::CaretPaint {
                            bounds: cursor_bounds,
                            color: theme.cursor,
                            force_visible: editor.is_selecting,
                        });
                    }
                }
            }

            // IME marked text underline
            if let Some(ref marked) = editor.marked_range {
                let mark_start_on_line = marked.start.max(line_start_offset);
                let mark_end_on_line = marked.end.min(line_end_offset);
                if mark_start_on_line < mark_end_on_line {
                    let buf_start = mark_start_on_line - line_start_offset;
                    let buf_end = mark_end_on_line - line_start_offset;
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_start);
                    let disp_end = helpers::buffer_to_display_offset(&buffer_to_display, buf_end);
                    for bounds in helpers::build_highlight_bounds(
                        &display_text,
                        &runs,
                        wrapped,
                        disp_start,
                        disp_end,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        0.0,
                        letter_spacing,
                        window,
                    ) {
                        marked_underlines.push(fill(
                            Bounds::new(
                                point(bounds.left(), bounds.bottom() - px(2.0)),
                                size(bounds.size.width, px(1.0)),
                            ),
                            theme.text_primary,
                        ));
                    }
                }
            }

            // Link underlines when cmd is held
            if editor.command_held {
                let hovered_position = editor.hovered_editor_position;
                for span in &decoration.inline_spans {
                    if span.kind != crate::markdown::InlineKind::Link {
                        continue;
                    }
                    let buf_start = span.range.start.min(line_content.len());
                    let buf_end = span.range.end.min(line_content.len());
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_start);
                    let disp_end = helpers::buffer_to_display_offset(&buffer_to_display, buf_end);
                    for link_bounds in helpers::build_highlight_bounds(
                        &display_text,
                        &runs,
                        wrapped,
                        disp_start,
                        disp_end,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        0.0,
                        letter_spacing,
                        window,
                    ) {
                        if hovered_position.is_some_and(|position| link_bounds.contains(&position))
                        {
                            link_underlines.push(fill(
                                Bounds::new(
                                    point(link_bounds.left(), link_bounds.bottom() - px(2.0)),
                                    size(link_bounds.size.width, px(1.5)),
                                ),
                                theme.link,
                            ));
                        }
                        link_hitboxes
                            .push(window.insert_hitbox(link_bounds, HitboxBehavior::Normal));
                    }
                }

                for wiki_link in &decoration.wiki_links {
                    if !editor.markdown_target_is_navigable(&wiki_link.target_path) {
                        continue;
                    }
                    let is_collapsed =
                        !helpers::active_ranges_intersect(&active_ranges, &wiki_link.full_range);
                    let visible_range = if is_collapsed {
                        wiki_link.collapsed_range.clone()
                    } else {
                        wiki_link.full_range.clone()
                    };
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.start);
                    let disp_end =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.end);
                    for link_bounds in helpers::build_highlight_bounds(
                        &display_text,
                        &runs,
                        wrapped,
                        disp_start,
                        disp_end,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        0.0,
                        letter_spacing,
                        window,
                    ) {
                        if hovered_position.is_some_and(|position| link_bounds.contains(&position))
                        {
                            link_underlines.push(fill(
                                Bounds::new(
                                    point(link_bounds.left(), link_bounds.bottom() - px(2.0)),
                                    size(link_bounds.size.width, px(1.5)),
                                ),
                                theme.link,
                            ));
                        }
                        link_hitboxes
                            .push(window.insert_hitbox(link_bounds, HitboxBehavior::Normal));
                    }
                }

                for footnote_reference in &decoration.footnote_references {
                    if editor
                        .find_footnote_definition_line(&footnote_reference.label)
                        .is_none()
                    {
                        continue;
                    }
                    let is_collapsed = !helpers::active_ranges_intersect(
                        &active_ranges,
                        &footnote_reference.full_range,
                    );
                    let visible_range = if is_collapsed {
                        footnote_reference.collapsed_range.clone()
                    } else {
                        footnote_reference.full_range.clone()
                    };
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.start);
                    let disp_end =
                        helpers::buffer_to_display_offset(&buffer_to_display, visible_range.end);
                    for link_bounds in helpers::build_highlight_bounds(
                        &display_text,
                        &runs,
                        wrapped,
                        disp_start,
                        disp_end,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        0.0,
                        letter_spacing,
                        window,
                    ) {
                        if hovered_position.is_some_and(|position| link_bounds.contains(&position))
                        {
                            link_underlines.push(fill(
                                Bounds::new(
                                    point(link_bounds.left(), link_bounds.bottom() - px(2.0)),
                                    size(link_bounds.size.width, px(1.5)),
                                ),
                                theme.link,
                            ));
                        }
                        link_hitboxes
                            .push(window.insert_hitbox(link_bounds, HitboxBehavior::Normal));
                    }
                }
            }

            for superscript_range in &decoration.superscript_spans {
                if !helpers::inline_range_is_collapsed(
                    &decoration,
                    superscript_range,
                    &active_ranges,
                ) {
                    continue;
                }
                let disp_start =
                    helpers::buffer_to_display_offset(&buffer_to_display, superscript_range.start);
                let disp_end =
                    helpers::buffer_to_display_offset(&buffer_to_display, superscript_range.end);
                push_inline_text_overlay(
                    &mut inline_overlays,
                    window,
                    wrapped,
                    line_content
                        .get(superscript_range.clone())
                        .unwrap_or_default(),
                    disp_start..disp_end,
                    &body_font,
                    &body_cjk_font,
                    theme.syntax_marker,
                    font_size,
                    editor.styles.editor.editor_superscript_scale_factor,
                    InlineOverlayVerticalAlign::Top(
                        editor.scaled(editor.styles.editor.editor_superscript_offset_px),
                    ),
                    false,
                    content_x,
                    text_line_top,
                    indent_px,
                    line_height,
                );
            }

            for subscript_range in &decoration.subscript_spans {
                if !helpers::inline_range_is_collapsed(&decoration, subscript_range, &active_ranges)
                {
                    continue;
                }
                let disp_start =
                    helpers::buffer_to_display_offset(&buffer_to_display, subscript_range.start);
                let disp_end =
                    helpers::buffer_to_display_offset(&buffer_to_display, subscript_range.end);
                push_inline_text_overlay(
                    &mut inline_overlays,
                    window,
                    wrapped,
                    line_content
                        .get(subscript_range.clone())
                        .unwrap_or_default(),
                    disp_start..disp_end,
                    &body_font,
                    &body_cjk_font,
                    theme.syntax_marker,
                    font_size,
                    editor.styles.editor.editor_subscript_scale_factor,
                    InlineOverlayVerticalAlign::Bottom(
                        editor.scaled(editor.styles.editor.editor_subscript_offset_px),
                    ),
                    false,
                    content_x,
                    text_line_top,
                    indent_px,
                    line_height,
                );
            }

            if decoration.footnote_definition_label.is_none() {
                for footnote_reference in &decoration.footnote_references {
                    if !helpers::inline_range_is_collapsed(
                        &decoration,
                        &footnote_reference.collapsed_range,
                        &active_ranges,
                    ) {
                        continue;
                    }
                    let disp_start = helpers::buffer_to_display_offset(
                        &buffer_to_display,
                        footnote_reference.collapsed_range.start,
                    );
                    let disp_end = helpers::buffer_to_display_offset(
                        &buffer_to_display,
                        footnote_reference.collapsed_range.end,
                    );
                    push_inline_text_overlay(
                        &mut inline_overlays,
                        window,
                        wrapped,
                        line_content
                            .get(footnote_reference.collapsed_range.clone())
                            .unwrap_or_default(),
                        disp_start..disp_end,
                        &body_font,
                        &body_cjk_font,
                        theme.link,
                        font_size,
                        editor.styles.editor.editor_superscript_scale_factor,
                        InlineOverlayVerticalAlign::Offset(-line_height * 0.14),
                        false,
                        content_x,
                        text_line_top,
                        indent_px,
                        line_height,
                    );
                }
            }

            // Determine if this is a table row with per-cell rendering
            let is_table_block_line =
                uses_table_cell_selection(block_type, &decoration, active_tab_is_diff)
                    && decoration
                        .table_info
                        .as_ref()
                        .is_some_and(|ti| !ti.is_separator);

            lines.push(PrepaintLine {
                wrapped: wrapped.clone(),
                origin: helpers::baseline_aligned_text_origin(
                    wrapped,
                    origin,
                    px(line_height),
                    reference_baseline,
                ),
                line_height: px(line_height),
                clip_bounds: text_clip_bounds.map(|clip| horizontal_text_clip_bounds(clip, bounds)),
                segments,
                is_table_block: is_table_block_line,
            });

            push_unordered_list_marker_paint(
                editor,
                line_content,
                block_type,
                prefix_collapsed,
                wrapped,
                &buffer_to_display,
                font_size,
                line_height,
                content_x,
                base_content_x,
                text_line_top,
                &monospace_font,
                window,
                &mut unordered_list_markers,
            );

            y += total_line_height;
            _last_line_height = total_line_height;
        }

        let mut code_block_copy_hitboxes = Vec::new();
        let code_block_styles = &editor.styles.code_block;
        let preview_button_gap = editor.scaled(code_block_styles.generated_preview_action_gap_px);
        let button_font_size = editor.scaled(code_block_styles.code_block_copy_button_font_size_px);
        let button_padding_x = editor.scaled(code_block_styles.code_block_copy_button_padding_x_px);
        let button_padding_y = editor.scaled(code_block_styles.code_block_copy_button_padding_y_px);
        let button_text_line_height = button_font_size * 1.2;
        let button_right = editor.scaled(code_block_styles.code_block_copy_button_right_px);
        let button_top = editor.scaled(code_block_styles.code_block_copy_button_top_px);
        let button_radius = px(editor.scaled(code_block_styles.code_block_copy_button_radius_px));
        let button_border_width =
            px(editor.scaled(code_block_styles.code_block_copy_button_border_width_px));
        let button_min_left = editor.scaled(code_block_styles.code_block_text_padding_left_px);
        let mut button_font = text_style.font();
        button_font.family = configured_font_family(editor.styles.ui_font_family);
        let code_block_border_width_px = snap_pixel_width(
            editor.scaled(code_block_styles.code_block_border_width_px),
            window,
        );
        let overlay_active = editor.preview_overlay_state().is_some();
        let preview_outer_button_padding_x = button_padding_x;
        let preview_inner_button_padding_x =
            editor.scaled(code_block_styles.generated_preview_action_inner_padding_x_px);
        let preview_action_padding = editor
            .scaled(code_block_styles.generated_preview_action_padding_px)
            .round();
        let preview_action_button_size = editor
            .scaled(code_block_styles.generated_preview_action_button_size_px)
            .round();
        let preview_action_icon_size = editor
            .scaled(code_block_styles.generated_preview_action_icon_size_px)
            .round();
        let preview_action_button_radius =
            px(editor.scaled(code_block_styles.generated_preview_action_button_radius_px));
        let preview_action_border_width =
            px(editor.scaled(code_block_styles.generated_preview_action_border_width_px));
        let preview_action_border_color = code_block_styles.generated_preview_action_border_color;
        if non_markdown_nowrap {
            let max_horizontal_offset = non_markdown_horizontal_state
                .max_offset
                .max(non_markdown_horizontal_extent);
            if max_horizontal_offset > 0.5 {
                let scrollbar_styles = &editor.styles.editor;
                let margin =
                    editor.scaled(code_block_styles.code_block_horizontal_scrollbar_margin_x_px);
                let track_left = editor_left + margin;
                let track_width = (editor_width - margin * 2.0).max(1.0);
                let (thumb_left, thumb_width, travel) = code_block_horizontal_thumb_geometry(
                    track_left,
                    track_width,
                    max_horizontal_offset,
                    non_markdown_horizontal_state.offset,
                    editor.scaled(scrollbar_styles.editor_scrollbar_min_thumb_height_px),
                );
                let hit_height = editor.scaled(
                    scrollbar_styles
                        .editor_scrollbar_hit_width_px
                        .max(scrollbar_styles.editor_scrollbar_width_px)
                        .max(scrollbar_styles.editor_scrollbar_hover_width_px),
                );
                let bar_bottom = origin_y + viewport_height
                    - editor.scaled(scrollbar_styles.editor_scrollbar_bottom_margin_px);
                let track_top = bar_bottom - hit_height;
                code_block_horizontal_scrollbars_local.push(CodeBlockHorizontalScrollbarGeometry {
                    start_line: NON_MARKDOWN_HORIZONTAL_SCROLL_TARGET,
                    track_bounds: Bounds::new(
                        point(px(track_left), px(track_top)),
                        size(px(track_width), px(hit_height)),
                    ),
                    thumb_bounds: Bounds::new(
                        point(px(thumb_left), px(track_top)),
                        size(px(thumb_width), px(hit_height)),
                    ),
                    track_travel: travel,
                    max_scroll: max_horizontal_offset,
                });
            }
        }
        for (start_line, group) in &code_block_group_bounds {
            let group_height = group.bottom - group.top;
            if code_block_border_width_px > 0.0 && group.width > 0.0 && group_height > 0.0 {
                let border_color = code_block_styles.code_block_border_color;
                let border_width = code_block_border_width_px.min(group.width * 0.5);
                code_block_bgs.push(fill(
                    snap_bounds_to_device_pixels(
                        group.left,
                        group.top,
                        border_width,
                        group_height,
                        window,
                    ),
                    border_color,
                ));
                code_block_bgs.push(fill(
                    snap_bounds_to_device_pixels(
                        group.left + group.width - border_width,
                        group.top,
                        border_width,
                        group_height,
                        window,
                    ),
                    border_color,
                ));
                code_block_bgs.push(fill(
                    snap_bounds_to_device_pixels(
                        group.left,
                        group.top,
                        group.width,
                        border_width.min(group_height),
                        window,
                    ),
                    border_color,
                ));
                code_block_bgs.push(fill(
                    snap_bounds_to_device_pixels(
                        group.left,
                        group.bottom - border_width.min(group_height),
                        group.width,
                        border_width.min(group_height),
                        window,
                    ),
                    border_color,
                ));
            }

            let group_bounds = Bounds::new(
                point(px(group.left), px(group.top)),
                size(px(group.width), px(group.bottom - group.top)),
            );
            let display_override = editor.code_block_display_override(*start_line);
            let max_horizontal_offset = code_block_horizontal_extents
                .get(start_line)
                .copied()
                .unwrap_or(display_override.max_horizontal_offset);
            if !editor.code_block_word_wrap_enabled(*start_line) && max_horizontal_offset > 0.5 {
                let scrollbar_styles = &editor.styles.editor;
                let track_margin =
                    editor.scaled(code_block_styles.code_block_horizontal_scrollbar_margin_x_px);
                let track_left = group.left + track_margin;
                let track_width = (group.width - track_margin * 2.0).max(1.0);
                let (thumb_left, thumb_width, travel) = code_block_horizontal_thumb_geometry(
                    track_left,
                    track_width,
                    max_horizontal_offset,
                    display_override.horizontal_offset,
                    editor.scaled(scrollbar_styles.editor_scrollbar_min_thumb_height_px),
                );
                let requested_hit_height = editor.scaled(
                    scrollbar_styles
                        .editor_scrollbar_hit_width_px
                        .max(scrollbar_styles.editor_scrollbar_width_px)
                        .max(scrollbar_styles.editor_scrollbar_hover_width_px),
                );
                let visible_bottom = group.bottom.min(origin_y + viewport_height);
                let visible_top = group.top.max(origin_y);
                let bar_bottom = visible_bottom
                    - editor.scaled(scrollbar_styles.editor_scrollbar_bottom_margin_px);
                let hit_height = requested_hit_height.min((bar_bottom - visible_top).max(1.0));
                let track_top = bar_bottom - hit_height;
                let track_bounds = Bounds::new(
                    point(px(track_left), px(track_top)),
                    size(px(track_width), px(hit_height)),
                );
                let thumb_bounds = Bounds::new(
                    point(px(thumb_left), px(track_top)),
                    size(px(thumb_width), px(hit_height)),
                );
                code_block_horizontal_scrollbars_local.push(CodeBlockHorizontalScrollbarGeometry {
                    start_line: *start_line,
                    track_bounds,
                    thumb_bounds,
                    track_travel: travel,
                    max_scroll: max_horizontal_offset,
                });
            }
            let is_front_matter_group = editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.table_info.as_ref())
                .is_some_and(|table_info| table_info.is_front_matter);

            if overlay_active {
                continue;
            }

            // Track hover and draw the theme's primary border around block
            // groups (code blocks, front matter, charts, formulas).
            code_block_hover_areas_local.push((group_bounds, *start_line));
            if editor.hovering_code_block_start == Some(*start_line) {
                let image_styles = &editor.styles.image_preview;
                let hover_border_width =
                    px(editor.scaled(image_styles.image_active_border_width_px));
                code_block_bgs.push(quad(
                    group_bounds,
                    Corners::all(px(
                        editor.scaled(editor.styles.code_block.code_block_radius_px)
                    )),
                    gpui::transparent_black(),
                    hover_border_width,
                    image_styles.image_active_border_color,
                    BorderStyle::default(),
                ));
            }

            if is_front_matter_group {
                continue;
            }

            let language_label = editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.code_language.as_deref())
                .filter(|language| !language.trim().is_empty())
                .map(|language| language.to_uppercase())
                .unwrap_or_else(|| "CODE".to_string());

            let preview_anchor = editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.block_preview.as_ref())
                .filter(|preview| preview.is_anchor);
            if let Some(preview) = preview_anchor {
                let code_mode = generated_preview_code_mode_active(
                    preview,
                    cursor_line,
                    cursor.offset,
                    &all_sel_ranges,
                    syntax_expansion_allowed,
                    search_reveal_range.as_ref(),
                );
                if code_mode && let Some(Some(img_data)) = image_cache.get(&preview.cache_key) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0 && img_h > 0.0 {
                        let (intrinsic_w, intrinsic_h) =
                            helpers::intrinsic_generated_preview_size_for_kind(
                                img_w,
                                img_h,
                                preview.kind,
                            );
                        let code_mode_text_width = ((group.width - 1.0) * 0.5).max(80.0);
                        let preview_border_x = group.left + code_mode_text_width;
                        let preview_left = preview_border_x + 1.0;
                        let preview_width = (group.width - code_mode_text_width - 1.0).max(1.0);
                        // `group` is measured from this frame's real line
                        // geometry, so it is authoritative whenever the whole
                        // block was laid out. The cached `line_y_offset` path
                        // only covers blocks the windowed prepaint clipped;
                        // using it while editing made the render (and its action
                        // buttons) jump by one frame of stale line heights.
                        let block_fully_rendered = preview.start_line >= render_start_line
                            && preview.end_line < render_end_line;
                        let (preview_top, preview_height) = if !block_fully_rendered
                            && has_valid_cache
                        {
                            let block_scroll = scroll_offset + scroll_anchor_delta;
                            let true_top =
                                origin_y + editor.line_y_offset(preview.start_line) - block_scroll;
                            let true_bottom = origin_y
                                + editor
                                    .line_y_offset((preview.end_line + 1).min(buffer.line_count()))
                                - block_scroll;
                            (true_top, (true_bottom - true_top).max(1.0))
                        } else {
                            (group.top, (group.bottom - group.top).max(1.0))
                        };
                        // Dashed divider between the source (left) and rendered
                        // (right) panes; sizing/colour all come from style.rs.
                        let split_styles = &editor.styles.code_block;
                        let dash_w = editor.scaled(split_styles.code_block_split_border_width_px);
                        let dash_len = editor
                            .scaled(split_styles.code_block_split_border_dash_length_px)
                            .max(0.5);
                        let dash_gap =
                            editor.scaled(split_styles.code_block_split_border_dash_gap_px);
                        let dash_color = split_styles.code_block_split_border_color;
                        let split_end_y = preview_top + preview_height;
                        let mut dash_y = preview_top;
                        while dash_y < split_end_y {
                            let segment = dash_len.min(split_end_y - dash_y);
                            code_block_bgs.push(fill(
                                Bounds::new(
                                    point(px(preview_border_x), px(dash_y)),
                                    size(px(dash_w), px(segment)),
                                ),
                                dash_color,
                            ));
                            dash_y += dash_len + dash_gap;
                        }

                        let pad_x =
                            editor.scaled(editor.styles.code_block.generated_preview_padding_x_px);
                        let pad_y =
                            editor.scaled(editor.styles.code_block.generated_preview_padding_y_px);
                        let inner_width = (preview_width - 2.0 * pad_x).max(1.0);
                        let inner_height = (preview_height - 2.0 * pad_y).max(1.0);
                        let fit_scale = (inner_width / intrinsic_w.max(1.0))
                            .min(inner_height / intrinsic_h.max(1.0))
                            .min(1.0)
                            .max(0.0);
                        let fitted_w = intrinsic_w * fit_scale;
                        let fitted_h = intrinsic_h * fit_scale;

                        let view = editor.generated_preview_view(&preview.cache_key);
                        let zoom = view.zoom_factor.clamp(0.25, 6.0);
                        let draw_w = fitted_w * zoom;
                        let draw_h = fitted_h * zoom;
                        // Pin the chart to the pane's top-left in code (edit)
                        // mode. Live re-rendering changes the chart's intrinsic
                        // size; centring it made it drift both horizontally and
                        // vertically on each rebuild. Anchoring the corner lets it
                        // only grow right/down, keeping it steady while editing.
                        let img_left = preview_left + pad_x + view.pan_x;
                        let img_top = preview_top + pad_y + view.pan_y;
                        let img_bounds = Bounds::new(
                            point(px(img_left), px(img_top)),
                            size(px(draw_w), px(draw_h)),
                        );
                        // Clip to the full render pane, not the padded inner box,
                        // so the chart stays visible to the pane edge when dragged.
                        let visible_img_bounds = Bounds::new(
                            point(px(preview_left), px(preview_top)),
                            size(px(preview_width), px(preview_height)),
                        );
                        let hit_bounds = Bounds::new(
                            point(px(preview_left), px(preview_top)),
                            size(px(preview_width), px(preview_height)),
                        );
                        image_previews.push((
                            img_bounds,
                            img_data.clone(),
                            px(0.0),
                            Some(visible_img_bounds),
                        ));
                        generated_preview_hit_areas.push((hit_bounds, preview.start_line));
                        generated_preview_image_areas.push((hit_bounds, preview.start_line));
                    }
                }
            }
            let preview_is_folded = preview_anchor
                .is_some_and(|preview| editor.folded_sections.contains(&preview.start_line));
            let preview_is_active = preview_anchor.is_some_and(|preview| {
                helpers::block_preview_is_active(preview, cursor_line, &all_sel_ranges)
                    || editor.search_reveals_source_range(&preview.source_range)
            });
            let group_syntax_active = editor.search_reveals_code_fence_group(*start_line)
                || code_block_group_range(*start_line).is_some_and(|(group_start, group_end)| {
                    active_code_fence_group.is_some_and(|(active_start, active_end)| {
                        group_start == active_start && group_end == active_end
                    })
                });

            let is_copied = editor.copied_code_block_start == Some(*start_line);
            let preview_action_anchor_bounds = preview_anchor.and_then(|_| {
                generated_preview_hit_areas
                    .iter()
                    .find(|(_, line_idx)| *line_idx == *start_line)
                    .map(|(bounds, _)| *bounds)
            });
            let preview_actions_visible = preview_anchor.is_some()
                && !preview_is_folded
                && preview_action_anchor_bounds.is_some()
                && (preview_is_active
                    || editor.generated_preview_actions_visible_line == Some(*start_line)
                    || editor.hovering_code_block_start == Some(*start_line)
                    || editor.hovering_generated_preview_line == Some(*start_line)
                    || editor.hovering_code_block_copy_start == Some(*start_line)
                    || editor.hovering_generated_preview_zoom_line == Some(*start_line)
                    || is_copied);
            let is_visible = if preview_anchor.is_some() {
                preview_actions_visible
            } else {
                // A folded block hides its body, so the language label is the
                // only remaining hint about its content: keep it pinned.
                editor.folded_sections.contains(start_line)
                    || group_syntax_active
                    || editor.hovering_code_block_start == Some(*start_line)
                    || editor.hovering_code_block_copy_start == Some(*start_line)
                    || is_copied
            };
            if !is_visible {
                continue;
            }

            let is_preview_action = preview_anchor.is_some();
            if is_preview_action {
                let Some(action_anchor_bounds) = preview_action_anchor_bounds else {
                    continue;
                };
                let action_anchor_left = f32::from(action_anchor_bounds.origin.x);
                let action_anchor_top = f32::from(action_anchor_bounds.origin.y);
                let action_anchor_width = f32::from(action_anchor_bounds.size.width);
                let action_button_size = preview_action_button_size;
                let action_opacity = editor.generated_preview_actions_opacity.clamp(0.0, 1.0);
                if action_opacity <= 0.01 {
                    continue;
                }
                let actions = [
                    (
                        GeneratedPreviewAction::Fullscreen,
                        PREVIEW_FULLSCREEN_SVG_PATH,
                        "FULL".to_string(),
                    ),
                    (
                        GeneratedPreviewAction::Restore,
                        PREVIEW_RESTORE_SVG_PATH,
                        "RESET".to_string(),
                    ),
                    (
                        GeneratedPreviewAction::ZoomIn,
                        PREVIEW_ZOOM_IN_SVG_PATH,
                        "+".to_string(),
                    ),
                    (
                        GeneratedPreviewAction::ZoomOut,
                        PREVIEW_ZOOM_OUT_SVG_PATH,
                        "-".to_string(),
                    ),
                    (
                        GeneratedPreviewAction::Copy,
                        PREVIEW_COPY_SVG_PATH,
                        "COPY".to_string(),
                    ),
                    (
                        GeneratedPreviewAction::Code,
                        FMT_INLINE_CODE_SVG,
                        "CODE".to_string(),
                    ),
                ]
                .into_iter()
                .filter(|(action, _, _)| {
                    generated_preview_action_is_visible(*action, editor.active_tab_is_read_only())
                })
                .collect::<Vec<_>>();
                let action_group_width = actions.len() as f32 * action_button_size
                    + actions.len().saturating_sub(1) as f32 * preview_button_gap;
                let action_group_x = (action_anchor_left + action_anchor_width
                    - preview_action_padding
                    - action_group_width)
                    .max(action_anchor_left + preview_action_padding)
                    .round();
                let button_y = (action_anchor_top + preview_action_padding).round();
                for (index, (action, icon_path, label_text)) in actions.into_iter().enumerate() {
                    let button_x = (action_group_x
                        + index as f32 * (action_button_size + preview_button_gap))
                        .round();
                    let button_bounds = Bounds::new(
                        point(px(button_x), px(button_y)),
                        size(px(action_button_size), px(action_button_size)),
                    );
                    let hovered =
                        editor.hovering_generated_preview_action == Some((*start_line, action));
                    let copy_copied = action == GeneratedPreviewAction::Copy && is_copied;
                    let icon_path = if copy_copied {
                        CHECK_SVG_PATH
                    } else {
                        icon_path
                    };
                    let (mut button_bg, mut icon_color, mut border_color) = if copy_copied {
                        (
                            code_block_styles.code_block_copy_button_copied_bg,
                            code_block_styles.code_block_copy_button_copied_text_color,
                            preview_action_border_color,
                        )
                    } else if hovered {
                        (
                            code_block_styles.code_block_copy_button_hover_bg,
                            code_block_styles.code_block_copy_button_hover_text_color,
                            preview_action_border_color,
                        )
                    } else {
                        (
                            code_block_styles.code_block_copy_button_bg,
                            code_block_styles.code_block_copy_button_text_color,
                            preview_action_border_color,
                        )
                    };
                    button_bg.a *= action_opacity;
                    icon_color.a *= action_opacity;
                    border_color.a *= action_opacity;
                    generated_preview_action_areas_local.push((button_bounds, *start_line, action));
                    let hitbox = window.insert_hitbox(button_bounds, HitboxBehavior::Normal);
                    let tooltip_title = match action {
                        GeneratedPreviewAction::Fullscreen => {
                            crate::i18n::preview_action_label(editor.ui_language, "fullscreen")
                        }
                        GeneratedPreviewAction::Restore => editor.t(crate::i18n::TextKey::Reset),
                        GeneratedPreviewAction::ZoomIn => {
                            crate::i18n::preview_action_label(editor.ui_language, "zoom_in")
                        }
                        GeneratedPreviewAction::ZoomOut => {
                            crate::i18n::preview_action_label(editor.ui_language, "zoom_out")
                        }
                        GeneratedPreviewAction::Copy => editor.t(crate::i18n::TextKey::Copy),
                        GeneratedPreviewAction::Code => {
                            crate::i18n::preview_action_label(editor.ui_language, "edit_code")
                        }
                    };
                    generated_preview_tooltips.push((
                        hitbox.clone(),
                        button_bounds,
                        SharedString::from(tooltip_title),
                    ));
                    preview_zoom_hitboxes.push(hitbox);
                    preview_zoom_buttons.push(CodeBlockCopyButtonPaint {
                        quad: quad(
                            button_bounds,
                            Corners::all(preview_action_button_radius),
                            button_bg,
                            preview_action_border_width,
                            border_color,
                            BorderStyle::default(),
                        ),
                        label: None,
                        origin: point(px(button_x), px(button_y)),
                        line_height: px(button_text_line_height),
                        icon: Some({
                            let icon_left =
                                button_x + (action_button_size - preview_action_icon_size) * 0.5;
                            let icon_top =
                                button_y + (action_button_size - preview_action_icon_size) * 0.5;
                            (
                                Bounds::new(
                                    point(px(icon_left), px(icon_top)),
                                    size(
                                        px(preview_action_icon_size),
                                        px(preview_action_icon_size),
                                    ),
                                ),
                                icon_path,
                                icon_color,
                            )
                        }),
                        trailing_labels: Vec::new(),
                    });
                    let _ = label_text;
                }
                continue;
            }
            let is_button_hovered = editor.hovering_code_block_copy_start == Some(*start_line);
            let (label_text, button_bg, button_text_color, button_border_color) = if is_copied {
                (
                    editor.t(crate::i18n::TextKey::Copied).to_string(),
                    code_block_styles.code_block_copy_button_copied_bg,
                    code_block_styles.code_block_copy_button_copied_text_color,
                    if is_preview_action {
                        preview_action_border_color
                    } else {
                        code_block_styles.code_block_copy_button_copied_border_color
                    },
                )
            } else if is_button_hovered {
                (
                    language_label.clone(),
                    code_block_styles.code_block_copy_button_hover_bg,
                    code_block_styles.code_block_copy_button_hover_text_color,
                    if is_preview_action {
                        preview_action_border_color
                    } else {
                        code_block_styles.code_block_copy_button_hover_border_color
                    },
                )
            } else {
                (
                    language_label,
                    code_block_styles.code_block_copy_button_bg,
                    code_block_styles.code_block_copy_button_text_color,
                    if is_preview_action {
                        preview_action_border_color
                    } else {
                        code_block_styles.code_block_copy_button_border_color
                    },
                )
            };
            let (copy_button_padding_left_x, copy_button_padding_right_x) = if is_preview_action {
                (
                    preview_inner_button_padding_x,
                    preview_outer_button_padding_x,
                )
            } else {
                (preview_inner_button_padding_x, button_padding_x)
            };
            let button_height_raw = (button_text_line_height + button_padding_y * 2.0).ceil();
            let icon_size = preview_action_icon_size;
            let icon_button_width = preview_action_button_size;
            let (label, button_width, button_height) = if is_preview_action {
                (
                    None,
                    icon_button_width,
                    icon_button_width.max(button_height_raw),
                )
            } else {
                let Some((label, button_width_raw, button_height_raw)) = shape_preview_button(
                    window,
                    &button_font,
                    button_font_size,
                    copy_button_padding_left_x,
                    copy_button_padding_right_x,
                    button_padding_y,
                    button_text_line_height,
                    &label_text,
                    button_text_color,
                ) else {
                    continue;
                };
                (
                    Some(label),
                    button_width_raw.ceil(),
                    button_height_raw.ceil(),
                )
            };
            let mut preview_zoom_button = None;
            if editor
                .decorations
                .get(*start_line)
                .and_then(|decoration| decoration.block_preview.as_ref())
                .filter(|preview| preview.is_anchor)
                .is_some_and(|preview| {
                    image_cache
                        .get(&preview.cache_key)
                        .is_some_and(|entry| entry.is_some())
                })
            {
                let is_zoom_hovered =
                    editor.hovering_generated_preview_zoom_line == Some(*start_line);
                let (zoom_bg, zoom_text_color, zoom_border_color) = if is_zoom_hovered {
                    (
                        code_block_styles.code_block_copy_button_hover_bg,
                        code_block_styles.code_block_copy_button_hover_text_color,
                        preview_action_border_color,
                    )
                } else {
                    (
                        code_block_styles.code_block_copy_button_bg,
                        code_block_styles.code_block_copy_button_text_color,
                        preview_action_border_color,
                    )
                };
                preview_zoom_button = Some((
                    icon_button_width,
                    icon_button_width.max(button_height_raw),
                    zoom_bg,
                    zoom_border_color,
                    zoom_text_color,
                ));
            }

            let button_height = preview_zoom_button
                .as_ref()
                .map(|(_, zoom_height, _, _, _)| button_height.max(*zoom_height))
                .unwrap_or(button_height);
            let action_anchor_bounds = if is_preview_action {
                let Some(bounds) = preview_action_anchor_bounds else {
                    continue;
                };
                bounds
            } else {
                group_bounds
            };
            let action_anchor_left = f32::from(action_anchor_bounds.origin.x);
            let action_anchor_top = f32::from(action_anchor_bounds.origin.y);
            let action_anchor_width = f32::from(action_anchor_bounds.size.width);
            let action_group_width = preview_zoom_button
                .as_ref()
                .map(|(zoom_width, _, _, _, _)| zoom_width + preview_button_gap + button_width)
                .unwrap_or(button_width);
            let action_group_x = if is_preview_action {
                (action_anchor_left + action_anchor_width
                    - preview_action_padding
                    - action_group_width)
                    .max(action_anchor_left + preview_action_padding)
                    .round()
            } else {
                ((action_anchor_left + action_anchor_width - button_right - action_group_width)
                    .max(action_anchor_left + button_min_left))
                .round()
            };
            let button_x = preview_zoom_button
                .as_ref()
                .map(|(zoom_width, _, _, _, _)| action_group_x + zoom_width + preview_button_gap)
                .unwrap_or(action_group_x)
                .round();
            let button_y = if is_preview_action {
                (action_anchor_top + preview_action_padding).round()
            } else {
                (action_anchor_top + button_top).round()
            };
            let button_bounds = Bounds::new(
                point(px(button_x), px(button_y)),
                size(px(button_width), px(button_height)),
            );

            if !is_preview_action {
                let display_button_width =
                    editor.scaled(code_block_styles.code_block_display_action_button_size_px);
                let display_icon_size =
                    editor.scaled(code_block_styles.code_block_display_action_icon_size_px);
                let display_actions = [
                    (
                        CodeBlockDisplayAction::LineNumbers,
                        CODE_BLOCK_LINE_NUMBERS_SVG_PATH,
                        editor.code_block_line_numbers_enabled(*start_line),
                        crate::i18n::attachment_settings_label(
                            editor.ui_language,
                            "code_block_line_numbers",
                        ),
                    ),
                    (
                        CodeBlockDisplayAction::WordWrap,
                        CODE_BLOCK_WRAP_SVG_PATH,
                        editor.code_block_word_wrap_enabled(*start_line),
                        crate::i18n::attachment_settings_label(
                            editor.ui_language,
                            "code_block_word_wrap",
                        ),
                    ),
                ];
                let display_action_gap =
                    editor.scaled(code_block_styles.code_block_display_action_gap_px);
                let display_group_width = display_actions.len() as f32 * display_button_width
                    + display_actions.len() as f32 * display_action_gap;
                let display_group_x = button_x - display_group_width;
                if display_group_x >= action_anchor_left + button_min_left {
                    for (index, (action, icon_path, enabled, tooltip_title)) in
                        display_actions.into_iter().enumerate()
                    {
                        let action_x = display_group_x
                            + index as f32 * (display_button_width + display_action_gap);
                        let action_bounds = Bounds::new(
                            point(px(action_x), px(button_y)),
                            size(px(display_button_width), px(button_height)),
                        );
                        let hovered = editor.hovering_code_block_display_action
                            == Some((*start_line, action));
                        let (background, icon_color, border_color) = if hovered {
                            (
                                code_block_styles.code_block_copy_button_hover_bg,
                                code_block_styles.code_block_copy_button_hover_text_color,
                                code_block_styles.code_block_copy_button_hover_border_color,
                            )
                        } else if enabled {
                            (
                                code_block_styles.code_block_copy_button_copied_bg,
                                code_block_styles.code_block_copy_button_copied_text_color,
                                code_block_styles.code_block_copy_button_copied_border_color,
                            )
                        } else {
                            (
                                code_block_styles.code_block_copy_button_bg,
                                code_block_styles.code_block_copy_button_text_color,
                                code_block_styles.code_block_copy_button_border_color,
                            )
                        };
                        code_block_display_action_areas_local.push((
                            action_bounds,
                            *start_line,
                            action,
                        ));
                        code_block_copy_hitboxes
                            .push(window.insert_hitbox(action_bounds, HitboxBehavior::Normal));
                        editor_icon_tooltips
                            .push((action_bounds, SharedString::from(tooltip_title)));
                        code_block_copy_buttons.push(CodeBlockCopyButtonPaint {
                            quad: quad(
                                action_bounds,
                                Corners::all(button_radius),
                                background,
                                button_border_width,
                                border_color,
                                BorderStyle::default(),
                            ),
                            label: None,
                            origin: point(px(action_x), px(button_y)),
                            line_height: px(button_text_line_height),
                            icon: Some((
                                Bounds::new(
                                    point(
                                        px(action_x
                                            + (display_button_width - display_icon_size) * 0.5),
                                        px(button_y + (button_height - display_icon_size) * 0.5),
                                    ),
                                    size(px(display_icon_size), px(display_icon_size)),
                                ),
                                icon_path,
                                icon_color,
                            )),
                            trailing_labels: Vec::new(),
                        });
                    }
                }
            }

            code_block_copy_areas_local.push((button_bounds, *start_line));
            code_block_copy_hitboxes
                .push(window.insert_hitbox(button_bounds, HitboxBehavior::Normal));
            code_block_copy_buttons.push(CodeBlockCopyButtonPaint {
                quad: quad(
                    button_bounds,
                    Corners::all(if is_preview_action {
                        preview_action_button_radius
                    } else {
                        button_radius
                    }),
                    button_bg,
                    if is_preview_action {
                        preview_action_border_width
                    } else {
                        button_border_width
                    },
                    button_border_color,
                    BorderStyle::default(),
                ),
                label,
                origin: point(
                    px(button_x + copy_button_padding_left_x),
                    px(button_y + button_padding_y),
                ),
                line_height: px(button_text_line_height),
                icon: is_preview_action.then(|| {
                    let icon_left = button_x + (button_width - icon_size) * 0.5;
                    let icon_top = button_y + (button_height - icon_size) * 0.5;
                    (
                        Bounds::new(
                            point(px(icon_left), px(icon_top)),
                            size(px(icon_size), px(icon_size)),
                        ),
                        MENU_CLIP_SVG_PATH,
                        button_text_color,
                    )
                }),
                trailing_labels: Vec::new(),
            });

            if let Some((zoom_button_width, _, zoom_bg, zoom_border_color, zoom_text_color)) =
                preview_zoom_button
            {
                let zoom_x = action_group_x;
                let zoom_bounds = Bounds::new(
                    point(px(zoom_x), px(button_y)),
                    size(px(zoom_button_width), px(button_height)),
                );

                generated_preview_zoom_areas_local.push((zoom_bounds, *start_line));
                preview_zoom_hitboxes
                    .push(window.insert_hitbox(zoom_bounds, HitboxBehavior::Normal));
                preview_zoom_buttons.push(CodeBlockCopyButtonPaint {
                    quad: quad(
                        zoom_bounds,
                        Corners::all(preview_action_button_radius),
                        zoom_bg,
                        preview_action_border_width,
                        zoom_border_color,
                        BorderStyle::default(),
                    ),
                    label: None,
                    origin: point(px(zoom_x), px(button_y)),
                    line_height: px(button_text_line_height),
                    icon: Some({
                        let icon_left = zoom_x + (zoom_button_width - icon_size) * 0.5;
                        let icon_top = button_y + (button_height - icon_size) * 0.5;
                        (
                            Bounds::new(
                                point(px(icon_left), px(icon_top)),
                                size(px(icon_size), px(icon_size)),
                            ),
                            SEARCH2_SVG_PATH,
                            zoom_text_color,
                        )
                    }),
                    trailing_labels: Vec::new(),
                });
            }
        }

        if !overlay_active {
            for (target_bounds, line_idx) in &image_zoom_button_targets {
                let Some((zoom_label, zoom_button_width_raw, button_height_raw)) =
                    shape_preview_button(
                        window,
                        &button_font,
                        button_font_size,
                        button_padding_x,
                        button_padding_x,
                        button_padding_y,
                        button_text_line_height,
                        "ZOOM",
                        code_block_styles.code_block_copy_button_text_color,
                    )
                else {
                    continue;
                };
                let zoom_button_width = zoom_button_width_raw.ceil();
                let button_height = button_height_raw.ceil();
                let target_left = f32::from(target_bounds.origin.x);
                let target_top = f32::from(target_bounds.origin.y);
                let target_width = f32::from(target_bounds.size.width);
                let min_button_left = (target_left + editor.scaled(12.0)).round();
                let zoom_x = (target_left + target_width - button_right - zoom_button_width)
                    .max(min_button_left)
                    .round();
                let button_y = (target_top + button_top).round();
                let zoom_bounds = Bounds::new(
                    point(px(zoom_x), px(button_y)),
                    size(px(zoom_button_width), px(button_height)),
                );

                image_preview_zoom_areas_local.push((zoom_bounds, *line_idx));
                preview_zoom_hitboxes
                    .push(window.insert_hitbox(zoom_bounds, HitboxBehavior::Normal));
                preview_zoom_buttons.push(CodeBlockCopyButtonPaint {
                    quad: quad(
                        zoom_bounds,
                        Corners::all(button_radius),
                        code_block_styles.code_block_copy_button_bg,
                        button_border_width,
                        code_block_styles.code_block_copy_button_border_color,
                        BorderStyle::default(),
                    ),
                    label: Some(zoom_label),
                    origin: point(
                        px(zoom_x + button_padding_x),
                        px(button_y + button_padding_y),
                    ),
                    line_height: px(button_text_line_height),
                    icon: None,
                    trailing_labels: Vec::new(),
                });
            }
        }

        // ── In-place diff gutter indicators + hover revert button ───────
        // Draw the left-edge bars from the active inline diff and the geometry
        // captured for visible lines above. Added/modified hunks get a green bar
        // over their buffer lines; pure deletions get a short red marker. A
        // full-width hover region per hunk reveals a top-right "revert" button.
        if let Some(inline_diff) = editor.active_inline_diff()
            && !inline_diff.hunks.is_empty()
            && !inline_diff_line_geometry.is_empty()
        {
            let geometry: HashMap<usize, (f32, f32)> = inline_diff_line_geometry
                .iter()
                .map(|(line, top, height)| (*line, (*top, *height)))
                .collect();
            let selection_styles = &editor.styles.selection;
            let bar_width = editor.scaled(selection_styles.selection_diff_bar_width_px);
            // Flush against the editor's far-left edge with no margin. The click
            // target is exactly the bar width, so the gap between the bar and the
            // text content still behaves as normal editor text (I-beam cursor).
            let bar_x = origin_x;
            let gutter_hit_left = bar_x;
            let gutter_hit_width = bar_width;
            let added_bar = selection_styles.selection_diff_added_bar_color;
            let removed_bar = selection_styles.selection_diff_removed_bar_color;
            let collapsed_opacity = selection_styles.selection_diff_bar_collapsed_opacity;
            let expanded_opacity = selection_styles.selection_diff_bar_expanded_opacity;
            let removed_marker_h = editor.scaled(10.0);
            for (hunk_index, hunk) in inline_diff.hunks.iter().enumerate() {
                // Dimmer while collapsed, full brightness while expanded.
                let bar_opacity = if hunk.expanded {
                    expanded_opacity
                } else {
                    collapsed_opacity
                };
                let mut added_bar = added_bar;
                added_bar.a *= bar_opacity;
                let mut removed_bar = removed_bar;
                removed_bar.a *= bar_opacity;
                // Region spanning the hunk's visible buffer lines plus any
                // expanded phantom band, used for the hover hit area and to
                // anchor the revert button.
                let mut region_top = f32::MAX;
                let mut region_bottom = f32::MIN;
                if hunk.expanded
                    && !hunk.removed_lines.is_empty()
                    && let Some(&(top, _)) = geometry.get(&hunk.buffer_start)
                {
                    // `top` is the buffer_start block's top, which already sits
                    // above the removed-row band (the band lives inside that
                    // line's top padding). So the hunk's visual top is `top`
                    // itself — do not subtract the reserved band height again.
                    region_top = region_top.min(top);
                    region_bottom = region_bottom.max(top);
                }
                match hunk.kind {
                    InlineDiffHunkKind::Added | InlineDiffHunkKind::Modified => {
                        let mut bar_top = f32::MAX;
                        let mut bar_bottom = f32::MIN;
                        let visible_hunk_start = hunk.buffer_start.max(render_start_line);
                        let visible_hunk_end = hunk.buffer_end.min(render_end_line);
                        for line in visible_hunk_start..visible_hunk_end {
                            let Some(&(top, height)) = geometry.get(&line) else {
                                continue;
                            };
                            bar_top = bar_top.min(top);
                            bar_bottom = bar_bottom.max(top + height);
                        }
                        if bar_bottom > bar_top {
                            inline_diff_gutter_bars.push((
                                Bounds::new(
                                    point(px(bar_x), px(bar_top)),
                                    size(px(bar_width), px(bar_bottom - bar_top)),
                                ),
                                added_bar,
                            ));
                            region_top = region_top.min(bar_top);
                            region_bottom = region_bottom.max(bar_bottom);
                            // Toggle hit area covers only the visible bar so the
                            // text to its right stays a normal editor click target.
                            inline_diff_gutter_areas_local.push((
                                Bounds::new(
                                    point(px(gutter_hit_left), px(bar_top)),
                                    size(px(gutter_hit_width), px(bar_bottom - bar_top)),
                                ),
                                hunk_index,
                            ));
                        }
                    }
                    InlineDiffHunkKind::Removed => {
                        let anchor_y = geometry
                            .get(&hunk.buffer_start)
                            .map(|&(top, _)| top)
                            .or_else(|| {
                                if hunk.buffer_start >= line_count {
                                    geometry
                                        .get(&line_count.saturating_sub(1))
                                        .map(|&(top, height)| top + height)
                                } else {
                                    None
                                }
                            });
                        if let Some(anchor_y) = anchor_y {
                            let marker_top = anchor_y - removed_marker_h / 2.0;
                            inline_diff_gutter_bars.push((
                                Bounds::new(
                                    point(px(bar_x), px(marker_top)),
                                    size(px(bar_width), px(removed_marker_h)),
                                ),
                                removed_bar,
                            ));
                            inline_diff_gutter_areas_local.push((
                                Bounds::new(
                                    point(
                                        px(gutter_hit_left),
                                        px(marker_top - removed_marker_h / 2.0),
                                    ),
                                    size(px(gutter_hit_width), px(removed_marker_h * 2.0)),
                                ),
                                hunk_index,
                            ));
                            if region_bottom <= region_top {
                                region_top = marker_top - removed_marker_h / 2.0;
                                region_bottom = marker_top + removed_marker_h * 1.5;
                            }
                        }
                    }
                }

                if region_bottom <= region_top {
                    continue;
                }

                let mut hunk_hover_top = region_top;

                // Expanded hunks always expose their revert action, aligned
                // flush with the text column's left edge.
                if hunk.expanded {
                    let is_revert_hovered =
                        editor.hovering_inline_diff_revert_hunk == Some(hunk_index);
                    let revert_button_font_size =
                        editor.scaled(selection_styles.selection_diff_revert_button_font_size_px);
                    let revert_button_padding_x =
                        editor.scaled(selection_styles.selection_diff_revert_button_padding_x_px);
                    let revert_button_padding_y =
                        editor.scaled(selection_styles.selection_diff_revert_button_padding_y_px);
                    let revert_button_text_line_height = revert_button_font_size * 1.2;
                    let revert_button_radius =
                        px(editor.scaled(selection_styles.selection_diff_revert_button_radius_px));
                    let revert_button_border_width = px(editor
                        .scaled(selection_styles.selection_diff_revert_button_border_width_px));
                    let revert_button_gap_y =
                        editor.scaled(selection_styles.selection_diff_revert_button_gap_y_px);
                    let revert_button_icon_size =
                        editor.scaled(selection_styles.selection_diff_revert_button_icon_size_px);
                    let revert_button_icon_gap =
                        editor.scaled(selection_styles.selection_diff_revert_button_icon_gap_px);
                    let revert_button_stat_leading_gap = editor.scaled(8.0);
                    let revert_button_stat_inner_gap = editor.scaled(3.0);
                    let brighten_stat_color = |mut color: Hsla| {
                        color.l = (color.l + 0.24).min(0.92);
                        color
                    };
                    let added_count = hunk.buffer_end.saturating_sub(hunk.buffer_start);
                    let removed_count = hunk.removed_lines.len();
                    let (revert_bg, revert_text_color, revert_border_color) = if is_revert_hovered {
                        (
                            selection_styles.selection_diff_revert_button_hover_bg,
                            selection_styles.selection_diff_revert_button_hover_text_color,
                            selection_styles.selection_diff_revert_button_hover_border_color,
                        )
                    } else {
                        (
                            selection_styles.selection_diff_revert_button_bg,
                            selection_styles.selection_diff_revert_button_text_color,
                            selection_styles.selection_diff_revert_button_border_color,
                        )
                    };
                    if let Some((revert_label, revert_button_width, revert_button_height)) =
                        shape_preview_button(
                            window,
                            &button_font,
                            revert_button_font_size,
                            revert_button_padding_x
                                + revert_button_icon_size
                                + revert_button_icon_gap,
                            revert_button_padding_x,
                            revert_button_padding_y,
                            revert_button_text_line_height,
                            editor.t(crate::i18n::TextKey::InlineDiffRevert),
                            revert_text_color,
                        )
                    {
                        let stat_runs = [
                            (
                                format!("+{added_count}"),
                                brighten_stat_color(editor.theme.diff_added_bg),
                                added_count > 0,
                            ),
                            (
                                format!("-{removed_count}"),
                                brighten_stat_color(editor.theme.diff_removed_bg),
                                removed_count > 0,
                            ),
                        ];
                        let mut stat_labels = Vec::new();
                        let mut stat_width = 0.0;
                        for (text, color, visible) in stat_runs {
                            if !visible {
                                continue;
                            }
                            let stat_run = TextRun {
                                len: text.len(),
                                font: button_font.clone(),
                                color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            if let Ok(mut lines) = window.text_system().shape_text(
                                text.into(),
                                px(revert_button_font_size),
                                &[stat_run],
                                None,
                                None,
                            ) {
                                if let Some(label) = lines.pop() {
                                    stat_width += f32::from(label.width());
                                    stat_labels.push(label);
                                }
                            }
                        }
                        if stat_labels.len() > 1 {
                            stat_width += revert_button_stat_inner_gap
                                * stat_labels.len().saturating_sub(1) as f32;
                        }
                        let label_width = f32::from(revert_label.width());
                        let stat_leading_gap = if stat_labels.is_empty() {
                            0.0
                        } else {
                            revert_button_stat_leading_gap
                        };
                        let revert_button_width =
                            (revert_button_width + stat_leading_gap + stat_width).ceil();
                        let revert_button_height = revert_button_height.ceil();
                        let content_left = editor_left + editor.padding_left();
                        let revert_x = content_left.round();
                        let revert_y = (region_top - revert_button_height - revert_button_gap_y)
                            .max(origin_y)
                            .round();
                        hunk_hover_top = hunk_hover_top.min(revert_y);
                        let revert_bounds = Bounds::new(
                            point(px(revert_x), px(revert_y)),
                            size(px(revert_button_width), px(revert_button_height)),
                        );
                        let label_x = revert_x
                            + revert_button_padding_x
                            + revert_button_icon_size
                            + revert_button_icon_gap;
                        let mut stat_x = label_x + label_width + stat_leading_gap;
                        let trailing_labels = stat_labels
                            .into_iter()
                            .map(|label| {
                                let origin =
                                    point(px(stat_x), px(revert_y + revert_button_padding_y));
                                stat_x += f32::from(label.width()) + revert_button_stat_inner_gap;
                                (label, origin)
                            })
                            .collect();
                        inline_diff_revert_buttons.push(CodeBlockCopyButtonPaint {
                            quad: quad(
                                revert_bounds,
                                Corners::all(revert_button_radius),
                                revert_bg,
                                revert_button_border_width,
                                revert_border_color,
                                BorderStyle::default(),
                            ),
                            label: Some(revert_label),
                            origin: point(px(label_x), px(revert_y + revert_button_padding_y)),
                            line_height: px(revert_button_text_line_height),
                            icon: Some({
                                let icon_left = revert_x + revert_button_padding_x;
                                let icon_top = revert_y
                                    + (revert_button_height - revert_button_icon_size) * 0.5;
                                (
                                    Bounds::new(
                                        point(px(icon_left), px(icon_top)),
                                        size(
                                            px(revert_button_icon_size),
                                            px(revert_button_icon_size),
                                        ),
                                    ),
                                    BACK_SVG_PATH,
                                    revert_text_color,
                                )
                            }),
                            trailing_labels,
                        });
                        inline_diff_revert_areas_local.push((revert_bounds, hunk_index));
                    }
                }

                // Keep a full-width hunk hover region for pointer feedback. The
                // always-visible button is drawn above the rows, so include the
                // vertical gap through the button.
                inline_diff_hunk_areas_local.push((
                    Bounds::new(
                        point(px(editor_left), px(hunk_hover_top)),
                        size(px(editor_width), px(region_bottom - hunk_hover_top)),
                    ),
                    hunk_index,
                ));
            }
        }

        for (tooltip_bounds, tooltip_title) in editor_icon_tooltips {
            let hitbox = window.insert_hitbox(tooltip_bounds, HitboxBehavior::Normal);
            generated_preview_tooltips.push((hitbox, tooltip_bounds, tooltip_title));
        }
        frame.offscreen_wrap_reflows -=
            initial_offscreen_wrap_reflows - remaining_offscreen_wrap_reflows;
        let mut mode_geometry_changed = false;
        self.editor.update(cx, |editor, cx| {
            editor.release_retired_images(cx);
            editor.code_block_horizontal_scrollbars =
                code_block_horizontal_scrollbars_local.clone();
            if non_markdown_nowrap
                && editor.apply_non_markdown_horizontal_extent(non_markdown_horizontal_extent)
            {
                cx.notify();
            }
            if editor.apply_code_block_horizontal_extents(&code_block_horizontal_extents) {
                cx.notify();
            }
            editor.sync_code_block_horizontal_scrollbar(cx);
            // Compare before committing measurements. Inspect all preview rows,
            // not just anchors: collapsed source rows commit zero height via an
            // early return, and inline math can change text-row height too.
            frame.generated_preview_height_changed = frame.generated_preview_height_changed
                || generated_preview_heights_changed(
                    &editor.decorations,
                    &display_info_updates,
                    |line| editor.visual_height_for_line(line),
                );
            editor.apply_line_display_info_updates(line_count, display_info_updates);
            if editor.front_matter_group_geometry_dirty
                && editor
                    .decorations
                    .first()
                    .and_then(|d| d.table_info.as_ref())
                    .filter(|table| table.is_front_matter)
                    .is_none_or(|table| {
                        (table.table_start_line..table.table_end_line).all(|line| {
                            editor.front_matter_line_is_hidden(line)
                                || editor
                                    .line_display_info
                                    .get(line)
                                    .is_some_and(|info| info.is_some())
                        })
                    })
            {
                editor.front_matter_group_geometry_dirty = false;
            }
            editor.visible_line_geometry = inline_diff_line_geometry.clone();
            editor.last_viewport_height = viewport_height;
            editor.last_viewport_width = viewport_width;
            editor.checkbox_areas = checkbox_hit_areas.clone();
            editor.front_matter_type_areas = front_matter_type_areas_local.clone();
            editor.front_matter_text_areas = front_matter_text_areas;
            editor.front_matter_delete_areas = front_matter_delete_areas.clone();
            editor.front_matter_list_scroll_areas = front_matter_list_scroll_areas;

            editor.image_click_areas = image_hit_areas.clone();
            editor.image_preview_zoom_areas = image_preview_zoom_areas_local.clone();
            editor.generated_preview_click_areas = generated_preview_hit_areas.clone();
            editor.generated_preview_image_areas = generated_preview_image_areas.clone();
            editor.generated_preview_zoom_areas = generated_preview_zoom_areas_local.clone();
            editor.generated_preview_action_areas = generated_preview_action_areas_local.clone();
            editor.code_block_hover_areas = code_block_hover_areas_local.clone();
            editor.code_block_copy_areas = code_block_copy_areas_local.clone();
            editor.code_block_display_action_areas = code_block_display_action_areas_local.clone();
            editor.table_click_areas = table_click_areas_local.clone();
            editor.table_cursor_paint_bounds = table_cursor_paint_bounds;
            editor.table_border_segment_areas = table_col_handle_areas_local
                .iter()
                .map(|(bounds, table, col)| (*bounds, *table, true, *col))
                .chain(
                    table_row_handle_areas_local
                        .iter()
                        .map(|(bounds, table, row)| (*bounds, *table, false, *row)),
                )
                .collect();
            editor.table_column_border_segments = table_col_handle_areas_local.clone();
            editor.table_row_border_segments = table_row_handle_areas_local.clone();
            editor.table_boundary_caret_areas = table_boundary_caret_areas_local.clone();
            editor.gutter_fold_areas = gutter_fold_hit_areas_local.clone();
            editor.format_line_areas = format_line_areas_local.clone();
            editor.format_trigger_areas = format_trigger_areas_local.clone();
            editor.line_format_toolbar_geometries = line_format_toolbar_geometries_local.clone();
            editor.inline_diff_gutter_areas = inline_diff_gutter_areas_local.clone();
            editor.inline_diff_hunk_areas = inline_diff_hunk_areas_local.clone();
            editor.inline_diff_revert_areas = inline_diff_revert_areas_local.clone();
            editor.phantom_row_hits = phantom_row_hits_local;
            editor.image_edit_areas = image_edit_hit_areas_local.clone();
            editor.last_editor_left = editor_left;
            editor.last_editor_width = editor_width;
            editor.last_content_left = editor_left + editor.padding_left();
            // Reconcile measured phantom (removed-row) heights. A wrap-width
            // change invalidates the cached heights; apply this frame's
            // measurements and, if anything changed, invalidate the geometry
            // cache so the reserved bands match on the next frame.
            let phantom_width_changed = (editor.last_text_width - text_width).abs() > 0.5;
            let mut phantom_heights_changed = false;
            if phantom_width_changed
                && (!editor.inline_diff_phantom_height_above.is_empty()
                    || !editor.inline_diff_phantom_height_below.is_empty())
            {
                editor.inline_diff_phantom_height_above.clear();
                editor.inline_diff_phantom_height_below.clear();
                phantom_heights_changed = true;
            }
            for (line, height) in phantom_above_heights {
                match editor.inline_diff_phantom_height_above.get(&line) {
                    Some(&existing) if (existing - height).abs() <= 0.5 => {}
                    _ => {
                        editor.inline_diff_phantom_height_above.insert(line, height);
                        phantom_heights_changed = true;
                    }
                }
            }
            for (line, height) in phantom_below_heights {
                match editor.inline_diff_phantom_height_below.get(&line) {
                    Some(&existing) if (existing - height).abs() <= 0.5 => {}
                    _ => {
                        editor.inline_diff_phantom_height_below.insert(line, height);
                        phantom_heights_changed = true;
                    }
                }
            }
            mode_geometry_changed = phantom_heights_changed;
            if phantom_heights_changed {
                editor.invalidate_line_geometry_cache();
                cx.notify();
            }
            editor.last_text_width = text_width;
            editor.last_window_scale_factor = window_scale_factor;
            if editor.refresh_editor_scrollbar_drag_after_layout() {
                cx.notify();
            }
            let center_requested =
                editor.pending_center_cursor_in_view || editor.center_cursor_confirm_frames > 0;
            // The animation epoch is shared, unlike the guards swapped into
            // this pane. A background pane must not consume another pane's request.
            let mut navigation_in_progress = editor.current_pane_id() == editor.focused_pane_id
                && outline::outline_navigation_in_progress(
                    editor.outline_navigation_epoch,
                    editor.scroll_anim_epoch,
                );
            if center_requested {
                editor.pending_click_anchor = None;
                editor.pending_typing_scroll_guard = None;
            } else if (editor.pending_click_anchor.is_some()
                || editor.pending_typing_scroll_guard.is_some())
                && navigation_in_progress
            {
                // These guards arrived after navigation started: a newer user
                // click/edit supersedes it, unlike a passive preview refresh.
                editor.scroll_anim_epoch = editor.scroll_anim_epoch.wrapping_add(1);
                editor.outline_navigation_epoch = None;
                navigation_in_progress = false;
            }
            let explicit_navigation =
                center_requested || navigation_in_progress || frame.explicit_caret_reveal_requested;
            if explicit_navigation {
                editor.pending_preview_reveal = None;
            }
            let scroll_before_anchor = editor.scroll_offset;
            if scroll_anchor_delta.abs() > 0.5
                && !explicit_navigation
                && !editor.document_mode_scroll_anchor_is_current()
                && editor.pending_preview_reveal.is_none()
                && editor.editor_scrollbar_drag.is_none()
                && !editor.pending_center_cursor_in_view
                && !editor.pending_scroll_to_cursor
            {
                editor.scroll_offset = (editor.scroll_offset + scroll_anchor_delta).max(0.0);
                editor.clamp_scroll();
                cx.notify();
            }
            editor.content_origin = bounds.origin;
            editor.layout_dirty = false;
            if deferred_offscreen_wrap_lines {
                cx.notify();
            }
            if !frame.reconcile_scroll {
                editor.ensure_visible_generated_block_previews(cx);
            }
            if (!frame.reconcile_scroll || frame.can_retry())
                && editor.restore_document_mode_scroll_anchor(true)
            {
                cx.notify();
            }
            let mut did_adjust_click_anchor = false;
            if let Some(anchor) = editor.pending_click_anchor {
                let line = anchor.line.min(editor.buffer.line_count());
                let rel_y_after = editor.text_y_offset_for_line(line);
                let target = (anchor.scroll_before + (rel_y_after - anchor.rel_y_before)).max(0.0);
                if (target - editor.scroll_offset).abs() > 0.5 {
                    editor.pending_preview_reveal = None;
                    editor.pending_scroll_to_cursor = false;
                    editor.pending_center_cursor_in_view = false;
                    editor.scroll_offset = target;
                    editor.clamp_scroll();
                    did_adjust_click_anchor = true;
                }
            }
            let did_preserve_typing_scroll = editor.restore_scroll_if_typing_stayed_on_visual_line(
                editor.scroll_offset - scroll_before_anchor,
            );
            let mut did_adjust_preview_reveal = false;
            if let Some(pending) = editor.pending_preview_reveal.take() {
                editor.pending_scroll_to_cursor = false;
                editor.pending_center_cursor_in_view = false;
                editor.scroll_offset = pending.scroll_offset.max(0.0);
                did_adjust_preview_reveal = true;
            }
            if editor.pending_center_cursor_in_view {
                if editor.pending_center_cursor_in_view_immediately {
                    editor.pending_center_cursor_in_view_immediately = false;
                    editor.center_cursor_in_view_immediately(cx);
                } else {
                    editor.center_cursor_in_view(cx);
                }
            } else if editor.pending_scroll_to_cursor {
                editor.scroll_to_cursor();
                cx.notify();
            } else if editor.center_cursor_confirm_frames > 0 {
                if !did_preserve_typing_scroll
                    && !did_adjust_click_anchor
                    && !did_adjust_preview_reveal
                {
                    if let Some(target) = editor.cursor_center_scroll_target()
                        && (target - editor.scroll_offset).abs() > 0.5
                    {
                        editor.scroll_offset = target;
                        editor.clamp_scroll();
                        editor.table_cursor_paint_bounds = None;
                    }
                }
                cx.notify();
            } else if did_adjust_preview_reveal
                || did_adjust_click_anchor
                || did_preserve_typing_scroll
            {
                cx.notify();
            }
            if !explicit_navigation
                && !editor.document_mode_scroll_anchor_is_current()
                && caret_was_visible
                && editor.editor_scrollbar_drag.is_none()
                // Passive preview reflow must not invent a new caret reveal
                // after wheel input already cancelled the explicit guards.
                && frame.allows_fallback_caret_preservation()
                && editor.keep_previously_visible_cursor_in_view()
            {
                cx.notify();
            }
        });

        if frame.should_retry(
            scroll_offset,
            self.editor.read(cx).scroll_offset,
            mode_geometry_changed,
        ) {
            // Window::transact removes this attempt's hitboxes/tooltips. Keep
            // the measured line caches and offset for the next attempt, but do
            // not publish stale caret, drag, menu or input geometry.
            return Err(());
        }
        self.editor.update(cx, |editor, cx| {
            // Rejected attempts retain the same input/navigation intent. Consume
            // it only once the geometry at the final scroll offset is accepted.
            if frame.scroll_is_reconciled(scroll_offset, editor.scroll_offset) {
                editor.pending_typing_scroll_guard = None;
                editor.pending_click_anchor = None;
                editor.center_cursor_confirm_frames =
                    editor.center_cursor_confirm_frames.saturating_sub(1);
            } else {
                editor.pending_scroll_to_cursor |= frame.cursor_reveal_requested;
                cx.notify();
            }
            if frame.reconcile_scroll {
                // On budget exhaustion defer any further correction, rather
                // than exposing an offset different from the geometry painted.
                editor.scroll_offset = scroll_offset;
            }
            editor.sync_slash_menu_anchor_after_layout(cx);
            editor.sync_table_input_menu_after_layout(window, cx);
            // A preview refresh may install a new scroll guard for a later
            // frame; do not consume it as part of this frame's transaction.
            let focused_code_mode = editor.focused_code_mode_preview_key();
            if focused_code_mode != editor.code_mode_preview_key {
                let leaving_code_view = editor.code_mode_preview_key.is_some();
                if let Some(old_key) = editor.code_mode_preview_key.take() {
                    editor.reset_generated_preview_view(&old_key);
                }
                if let Some(new_key) = focused_code_mode.clone() {
                    editor.reset_generated_preview_view(&new_key);
                }
                editor.code_mode_preview_key = focused_code_mode;
                if leaving_code_view {
                    editor.generated_preview_rerender_task = None;
                    editor.generated_preview_rerender_pending = false;
                    editor.rerender_generated_block_previews();
                }
                cx.notify();
            }
            if editor.generated_preview_rerender_pending {
                editor.generated_preview_rerender_pending = false;
                editor.schedule_generated_preview_rerender(cx);
            }
        });

        // Trigger loading for any image paths not yet in cache
        let mut new_image_paths: Vec<String> = Vec::new();
        {
            // Scan decorations by reference; cloning the whole decoration vector
            // here used to allocate thousands of LineDecoration structs on every
            // prepaint frame (i.e. every scroll frame) on large documents.
            let editor = self.editor.read(cx);
            let decoration_start = render_start_line.min(editor.decorations.len());
            let decoration_end = render_end_line.min(editor.decorations.len());
            for dec in &editor.decorations[decoration_start..decoration_end] {
                if let Some(ref path) = dec.image_path
                    && !editor.image_cache.contains_key(path)
                {
                    new_image_paths.push(path.clone());
                }
            }
        }
        if !new_image_paths.is_empty() {
            for path in new_image_paths {
                self.editor.update(cx, |editor, cx| {
                    editor.queue_image_load(path, cx);
                });
            }
        }

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        // Keep the format triggers above the editor's full-size hitbox so they
        // receive hover events and can reveal the line toolbar.
        let format_trigger_hitboxes = format_trigger_areas_local
            .iter()
            .map(|(bounds, line)| (window.insert_hitbox(*bounds, HitboxBehavior::Normal), *line))
            .collect::<Vec<_>>();

        let front_matter_delete_hitboxes = front_matter_delete_areas
            .iter()
            .map(|(bounds, _, _)| window.insert_hitbox(*bounds, HitboxBehavior::Normal))
            .collect();
        let front_matter_type_hitboxes = front_matter_type_areas_local
            .iter()
            .map(|(bounds, line)| {
                let hitbox = window.insert_hitbox(*bounds, HitboxBehavior::Normal);

                (hitbox, *line)
            })
            .collect::<Vec<_>>();

        // Tooltips must be requested during prepaint (gpui asserts the phase).
        for (_, tooltip_bounds, tooltip_title) in &generated_preview_tooltips {
            tooltip::request_tooltip_for_hitbox(
                window,
                cx,
                self.editor.clone(),
                *tooltip_bounds,
                tooltip_title.clone(),
                None::<&'static str>,
            );
        }

        // Record the layout-accurate content height for embedded editors so
        // containers measuring it never see estimate flicker.
        {
            let editor = self.editor.read(cx);
            if editor.embedded_editor {
                let content_height = editor.total_content_height();
                if (content_height - editor.embedded_content_height.get()).abs() > 0.5 {
                    editor.embedded_content_height.set(content_height);
                    let entity = self.editor.clone();
                    cx.defer(move |cx| {
                        entity.update(cx, |_, cx| cx.notify());
                    });
                }
            }
        }

        let prepaint_state = EditorPrepaintState {
            hitbox,
            link_hitboxes,
            horizontal_editor_clip_bounds,
            lines,
            indent_guide_quads,
            clipped_code_block_decorations,
            clipped_table_decorations,
            clipped_code_block_search_highlights,
            clipped_code_block_selections,
            clipped_table_search_highlights,
            clipped_table_selections,
            whitespace_dots,
            selection_quads,
            search_match_quads,
            diff_line_bgs,
            diff_inline_quads,
            inline_diff_gutter_bars,
            phantom_text_lines,
            line_highlight,
            quote_block_bgs,
            code_block_bgs,
            unordered_list_markers,
            marked_underlines,
            gutter_quads,
            gutter_labels,
            gutter_svgs,
            inline_svgs,
            gutter_fold_icons,
            format_icons,
            front_matter_type_icons,

            front_matter_group_texts,
            front_matter_delete_hitboxes,
            hr_lines,
            image_previews,
            image_borders,
            table_borders,
            table_border_paths,
            table_bgs,
            table_icons,
            table_cell_texts,
            code_block_copy_buttons,
            inline_diff_revert_buttons,
            code_block_copy_hitboxes,
            format_trigger_hitboxes,
            front_matter_type_hitboxes,
            preview_zoom_buttons,
            preview_zoom_hitboxes,
            tooltips: generated_preview_tooltips,
            image_edit_icons,
            link_underlines,
            inline_overlays,
        };

        self.editor.update(cx, |editor, _| {
            if let Some(source) = editor.line_drag_ghost_source() {
                let viewport = bounds.intersect(&window.content_mask().bounds);
                let source_bounds = editor
                    .format_line_areas
                    .iter()
                    .filter(|(_, line)| source.contains(line))
                    .map(|(bounds, _)| *bounds)
                    .reduce(|left, right| left.union(&right));
                if let Some(source_bounds) = source_bounds {
                    let top = source_bounds.top().max(viewport.top());
                    let bottom = source_bounds.bottom().min(viewport.bottom());
                    if bottom > top {
                        let ghost_bounds = Bounds::new(
                            point(px(editor.last_content_left), top),
                            size(px(editor.last_text_width), bottom - top),
                        );
                        editor.set_line_drag_ghost(std::rc::Rc::new(LineDragGhost::capture(
                            &prepaint_state,
                            ghost_bounds,
                            editor.theme.border_decorative,
                        )));
                    }
                }
            }
        });

        // Only the focused pane publishes caret geometry to its lightweight overlay.
        if self.editor.read(cx).focused_pane_id != self.pane_id {
            cursor_paints.clear();
        }
        if let Some(clip_bounds) = horizontal_editor_clip_bounds {
            let clip_left = f32::from(clip_bounds.origin.x);
            let clip_right = clip_left + f32::from(clip_bounds.size.width);
            cursor_paints.retain(|paint| {
                let left = f32::from(paint.bounds.origin.x);
                let right = left + f32::from(paint.bounds.size.width);
                right > clip_left && left < clip_right
            });
        }
        let caret_horizontal_offset = {
            let editor = self.editor.read(cx);
            let cursor_line = editor.buffer.offset_to_line_col(caret_cursor_offset).0;
            editor.horizontal_scroll_offset_for_line(cursor_line)
        };
        self.caret_geometry
            .replace(super::caret::CaretGeometrySnapshot {
                cursor_offset: caret_cursor_offset,
                scroll_offset,
                horizontal_offset: caret_horizontal_offset,
                table_boundary_caret: self.editor.read(cx).table_boundary_caret,
                paints: cursor_paints,
            });
        Ok(prepaint_state)
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name(
            format!("editor-element-{}", self.pane_id).into(),
        ))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Keep pane-local caches and anchors active for the entire transaction,
        // including retries and final caret/confirmation publication.
        let pane_swapped = self
            .editor
            .update(cx, |editor, _| editor.begin_pane_swap(self.pane_id));
        let (caret_was_visible, reconcile_scroll) = self.editor.update(cx, |editor, cx| {
            let caret_was_visible = editor.focused_pane_id == self.pane_id
                && (editor.cursor_visible_in_viewport()
                    || editor.pending_typing_scroll_guard.is_some());
            if editor.focused_pane_id == self.pane_id {
                editor.begin_large_document_frame_work();
            }
            editor.schedule_large_markdown_outline_index(cx);
            editor.refresh_front_matter_input_geometry(window, bounds);
            (
                caret_was_visible,
                editor.document_mode_scroll_anchor_is_current()
                    || editor.editor_scrollbar_drag.is_some()
                    || editor.center_cursor_confirm_frames > 0
                    || editor.pending_typing_scroll_guard.is_some()
                    || editor.pending_click_anchor.is_some()
                    || editor.pending_scroll_to_cursor,
            )
        });
        let state = reconciled_editor_prepaint(reconcile_scroll, |frame| {
            if frame.reconcile_scroll {
                window.transact(|window| {
                    self.prepaint_document(bounds, frame, caret_was_visible, window, cx)
                })
            } else {
                self.prepaint_document(bounds, frame, caret_was_visible, window, cx)
            }
        });
        self.editor.update(cx, |editor, cx| {
            if editor.confirm_document_mode_scroll_anchor() {
                cx.notify();
            }
            if pane_swapped {
                editor.end_pane_swap(self.pane_id);
            }
        });
        state
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let pane_swapped = self
            .editor
            .update(cx, |editor, _| editor.begin_pane_swap(self.pane_id));
        let is_focused_pane = self.editor.read(cx).focused_pane_id == self.pane_id;
        for (hitbox, tooltip_bounds, _) in &prepaint.tooltips {
            tooltip::register_tooltip_hover_refresh(window, hitbox, *tooltip_bounds);
        }
        let editor = self.editor.read(cx);
        let focus_handle = editor.focus_handle.clone();
        let table_handles_enabled = !editor.active_tab_is_read_only();
        let hovering_handle = editor.hovering_handle.filter(|_| table_handles_enabled);
        let cursor_style = match hovering_handle {
            _ if table_handles_enabled && editor.table_drag.is_some() => CursorStyle::ClosedHand,
            Some(_) => CursorStyle::OpenHand,
            None if editor
                .front_matter_type_areas
                .iter()
                .any(|(bounds, _)| bounds.contains(&window.mouse_position())) =>
            {
                CursorStyle::Arrow
            }
            None if editor.hovering_checkbox_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_fold_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_image_edit_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_image_preview_line.is_some() => CursorStyle::Arrow,
            None if editor.image_drag_state.is_some()
                && editor.preview_overlay_state().is_some() =>
            {
                CursorStyle::ClosedHand
            }
            None if editor.generated_preview_drag_state.is_some() => CursorStyle::ClosedHand,
            None if editor.preview_overlay_state().is_some() => CursorStyle::OpenHand,
            None if editor.hovering_generated_preview_zoom_line.is_some() => CursorStyle::Arrow,
            None if editor.hovering_generated_preview_line.is_some() => CursorStyle::OpenHand,
            None if editor.hovering_inline_diff_gutter_hunk.is_some() => CursorStyle::Arrow,
            None if editor.hovering_inline_diff_revert_hunk.is_some() => CursorStyle::Arrow,
            None if editor.code_block_horizontal_scrollbar_hovered.is_some()
                || editor.code_block_horizontal_scrollbar_drag.is_some() =>
            {
                CursorStyle::Arrow
            }
            None if editor.editor_scrollbar_hovered => CursorStyle::Arrow,
            None => CursorStyle::IBeam,
        };
        window.set_cursor_style(cursor_style, &prepaint.hitbox);
        for hitbox in &prepaint.code_block_copy_hitboxes {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        for (hitbox, _) in &prepaint.format_trigger_hitboxes {
            window.set_cursor_style(CursorStyle::OpenHand, hitbox);
        }
        for (hitbox, _) in &prepaint.front_matter_type_hitboxes {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        for hitbox in &prepaint.front_matter_delete_hitboxes {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }
        if !prepaint.front_matter_delete_hitboxes.is_empty() {
            let hitboxes = prepaint.front_matter_delete_hitboxes.clone();
            let editor_entity = self.editor.clone();
            let pane_id = self.pane_id;
            window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Capture
                    || event.button != gpui::MouseButton::Left
                    || !hitboxes.iter().any(|hitbox| hitbox.is_hovered(window))
                {
                    return;
                }
                let handled = editor_entity.update(cx, |this, cx| {
                    this.with_pane(pane_id, |this| {
                        this.remove_front_matter_list_item_for_position(event.position, cx)
                    })
                });
                if handled {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            });
        }
        for hitbox in &prepaint.preview_zoom_hitboxes {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        for link_hitbox in &prepaint.link_hitboxes {
            window.set_cursor_style(CursorStyle::PointingHand, link_hitbox);
        }
        let format_trigger_hitboxes = prepaint.format_trigger_hitboxes.clone();
        let editor_entity = self.editor.clone();
        let pane_id = self.pane_id;
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
            if phase != gpui::DispatchPhase::Capture {
                return;
            }
            let hovered_line = format_trigger_hitboxes
                .iter()
                .find(|(hitbox, _)| hitbox.is_hovered(window))
                .map(|(_, line)| *line);
            if let Some(line) = hovered_line {
                editor_entity.update(cx, |this, cx| {
                    this.with_pane(pane_id, |this| {
                        this.update_line_format_hover(Some(line), Some(line), cx);
                    });
                });
            }
        });
        let format_trigger_hitboxes = prepaint.format_trigger_hitboxes.clone();
        let editor_entity = self.editor.clone();
        let pane_id = self.pane_id;
        window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
            if phase != gpui::DispatchPhase::Capture || event.button != gpui::MouseButton::Left {
                return;
            }
            let clicked_line = format_trigger_hitboxes
                .iter()
                .find(|(hitbox, _)| hitbox.is_hovered(window))
                .map(|(_, line)| *line);
            if let Some(line) = clicked_line {
                window.prevent_default();
                cx.stop_propagation();
                editor_entity.update(cx, |this, cx| {
                    this.focus_pane(pane_id, cx);
                    window.focus(&this.focus_handle);
                    this.begin_line_drag(line, event.position, cx);
                });
            }
        });
        if !prepaint.front_matter_type_hitboxes.is_empty() {
            let hover_hitboxes = prepaint.front_matter_type_hitboxes.clone();
            let hovered = std::cell::Cell::new(
                hover_hitboxes
                    .iter()
                    .position(|(hitbox, _)| hitbox.bounds.contains(&window.mouse_position())),
            );
            let hover_editor = self.editor.clone();
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Bubble {
                    return;
                }
                let next = hover_hitboxes
                    .iter()
                    .position(|(hitbox, _)| hitbox.is_hovered(window));
                if hovered.replace(next) != next {
                    hover_editor.update(cx, |_, cx| cx.notify());
                }
            });
            let front_matter_type_hitboxes = prepaint.front_matter_type_hitboxes.clone();
            let editor_entity = self.editor.clone();
            let pane_id = self.pane_id;

            window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Capture || event.button != gpui::MouseButton::Left
                {
                    return;
                }
                if front_matter_type_hitboxes
                    .iter()
                    .any(|(hitbox, _)| hitbox.is_hovered(window))
                {
                    let handled = editor_entity.update(cx, |this, cx| {
                        this.with_pane(pane_id, |this| {
                            this.open_front_matter_type_menu_for_position(event.position, cx)
                        })
                    });
                    if handled {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }
            });
        }
        // IME/text input is routed to the focused pane only.
        if is_focused_pane {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.editor.clone()),
                cx,
            );
        }

        {
            let editor = self.editor.clone();
            window.on_modifiers_changed(move |event, _window, cx| {
                editor.update(cx, |this, cx| {
                    let new_val = event.modifiers.secondary();
                    if this.command_held != new_val {
                        this.command_held = new_val;
                        cx.notify();
                    }
                });
            });
        }

        for quad in prepaint.quote_block_bgs.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.code_block_bgs.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.table_bgs.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.diff_line_bgs.drain(..) {
            window.paint_quad(quad);
        }

        paint_quads_with_optional_mask(
            &mut prepaint.indent_guide_quads,
            prepaint.horizontal_editor_clip_bounds,
            window,
        );

        paint_quads_with_optional_mask(
            &mut prepaint.diff_inline_quads,
            prepaint.horizontal_editor_clip_bounds,
            window,
        );

        for (clip_bounds, decorations) in prepaint.clipped_code_block_decorations.drain(..) {
            window.with_content_mask(
                Some(ContentMask {
                    bounds: clip_bounds,
                }),
                |window| {
                    for decoration in decorations {
                        window.paint_quad(decoration);
                    }
                },
            );
        }
        for (clip_bounds, decorations) in prepaint.clipped_table_decorations.drain(..) {
            window.with_content_mask(
                Some(ContentMask {
                    bounds: clip_bounds,
                }),
                |window| {
                    for decoration in decorations {
                        window.paint_quad(decoration);
                    }
                },
            );
        }

        if let Some(highlight) = prepaint.line_highlight.take() {
            window.paint_quad(highlight);
        }

        // Search match highlights (behind selection)
        paint_quads_with_optional_mask(
            &mut prepaint.search_match_quads,
            prepaint.horizontal_editor_clip_bounds,
            window,
        );
        for (clip_bounds, highlights) in prepaint.clipped_code_block_search_highlights.drain(..) {
            window.with_content_mask(
                Some(ContentMask {
                    bounds: clip_bounds,
                }),
                |window| {
                    for highlight in highlights {
                        window.paint_quad(highlight);
                    }
                },
            );
        }
        for (clip_bounds, highlights) in prepaint.clipped_table_search_highlights.drain(..) {
            window.with_content_mask(
                Some(ContentMask {
                    bounds: clip_bounds,
                }),
                |window| {
                    for highlight in highlights {
                        window.paint_quad(highlight);
                    }
                },
            );
        }

        paint_quads_with_optional_mask(
            &mut prepaint.selection_quads,
            prepaint.horizontal_editor_clip_bounds,
            window,
        );
        for (clip_bounds, selections) in prepaint.clipped_code_block_selections.drain(..) {
            window.with_content_mask(
                Some(ContentMask {
                    bounds: clip_bounds,
                }),
                |window| {
                    for selection in selections {
                        window.paint_quad(selection);
                    }
                },
            );
        }
        for (clip_bounds, selections) in prepaint.clipped_table_selections.drain(..) {
            window.with_content_mask(
                Some(ContentMask {
                    bounds: clip_bounds,
                }),
                |window| {
                    for selection in selections {
                        window.paint_quad(selection);
                    }
                },
            );
        }

        paint_quads_with_optional_mask(
            &mut prepaint.whitespace_dots,
            prepaint.horizontal_editor_clip_bounds,
            window,
        );

        for quad in prepaint.gutter_quads.drain(..) {
            window.paint_quad(quad);
        }
        // In-place diff gutter bars: a spaced diagonal hatch clipped to each
        // bar's strip. Sweeping the intercept on a stable absolute grid keeps
        // the pattern from crawling while scrolling.
        if !prepaint.inline_diff_gutter_bars.is_empty() {
            let selection_styles = &editor.styles.selection;
            let stripe_thickness = editor
                .scaled(selection_styles.selection_diff_bar_stripe_thickness_px)
                .max(0.5);
            let stripe_spacing = editor
                .scaled(selection_styles.selection_diff_bar_stripe_spacing_px)
                .max(0.0);
            let stripe_period = (stripe_thickness + stripe_spacing).max(1.0);
            for (bar_bounds, color) in prepaint.inline_diff_gutter_bars.drain(..) {
                let x0 = f32::from(bar_bounds.origin.x);
                let x1 = x0 + f32::from(bar_bounds.size.width);
                let top = f32::from(bar_bounds.origin.y);
                let bottom = top + f32::from(bar_bounds.size.height);
                window.with_content_mask(Some(ContentMask { bounds: bar_bounds }), |window| {
                    let c_min = x0 + top - stripe_thickness;
                    let c_max = x1 + bottom;
                    let mut c0 = (c_min / stripe_period).floor() * stripe_period;
                    while c0 < c_max {
                        let mut path = Path::new(point(px(x0), px(c0 - x0)));
                        path.line_to(point(px(x1), px(c0 - x1)));
                        path.line_to(point(px(x1), px(c0 + stripe_thickness - x1)));
                        path.line_to(point(px(x0), px(c0 + stripe_thickness - x0)));
                        window.paint_path(path, color);
                        c0 += stripe_period;
                    }
                });
            }
        }
        for (icon_bounds, icon_path, color) in &prepaint.gutter_svgs {
            window
                .paint_svg(
                    *icon_bounds,
                    (*icon_path).into(),
                    TransformationMatrix::default(),
                    *color,
                    cx,
                )
                .ok();
        }
        window.with_content_mask(
            Some(ContentMask {
                bounds: bounds.intersect(&window.content_mask().bounds),
            }),
            |window| {
                for (icon_bounds, icon_path, color) in &prepaint.front_matter_type_icons {
                    window
                        .paint_svg(
                            *icon_bounds,
                            (*icon_path).into(),
                            TransformationMatrix::default(),
                            *color,
                            cx,
                        )
                        .ok();
                }
            },
        );
        for (clip, shaped, origin, height) in &prepaint.front_matter_group_texts {
            window.with_content_mask(Some(ContentMask { bounds: *clip }), |window| {
                shaped
                    .paint(*origin, *height, TextAlign::Left, None, window, cx)
                    .ok();
            });
        }
        for (icon_bounds, icon_path, color) in &prepaint.inline_svgs {
            window
                .paint_svg(
                    *icon_bounds,
                    (*icon_path).into(),
                    TransformationMatrix::default(),
                    *color,
                    cx,
                )
                .ok();
        }
        for (shaped, origin, line_height) in &prepaint.gutter_labels {
            shaped.paint(*origin, *line_height, window, cx).ok();
        }

        for (shaped, origin, line_height) in &prepaint.inline_overlays {
            shaped.paint(*origin, *line_height, window, cx).ok();
        }

        for (path, color) in prepaint.table_border_paths.drain(..) {
            window.paint_path(path, color);
        }
        for quad in prepaint.table_borders.drain(..) {
            window.paint_quad(quad);
        }
        for (bounds, path, color) in &prepaint.table_icons {
            window
                .paint_svg(
                    *bounds,
                    (*path).into(),
                    TransformationMatrix::default(),
                    *color,
                    cx,
                )
                .ok();
        }

        // Table cell texts (per-cell rendering)
        for (shaped, origin, lh) in &prepaint.table_cell_texts {
            shaped
                .paint(*origin, *lh, TextAlign::Left, None, window, cx)
                .ok();
        }

        // HR lines: solid for `---`/`___`, dashed for `***`.
        for (hr_y, hr_start_x, hr_width, is_dashed) in &prepaint.hr_lines {
            let theme = &self.editor.read(cx).theme;
            if *is_dashed {
                let dash_len = 6.0_f32;
                let gap_len = 4.0_f32;
                let mut x = *hr_start_x;
                let end_x = *hr_start_x + hr_width;
                while x < end_x {
                    let seg_end = (x + dash_len).min(end_x);
                    window.paint_quad(fill(
                        Bounds::new(point(px(x), px(*hr_y)), size(px(seg_end - x), px(1.0))),
                        theme.border_decorative,
                    ));
                    x = seg_end + gap_len;
                }
            } else {
                window.paint_quad(fill(
                    Bounds::new(
                        point(px(*hr_start_x), px(*hr_y)),
                        size(px(*hr_width), px(1.0)),
                    ),
                    theme.border_decorative,
                ));
            }
        }

        for line in &prepaint.lines {
            // Skip normal text rendering for table rows (rendered via table_cell_texts)
            if line.is_table_block {
                continue;
            }
            if let Some(ref segments) = line.segments {
                if let Some(clip_bounds) = line.clip_bounds {
                    window.with_content_mask(
                        Some(ContentMask {
                            bounds: clip_bounds,
                        }),
                        |window| {
                            for (seg_wrapped, seg_origin) in segments {
                                seg_wrapped
                                    .paint(
                                        *seg_origin,
                                        line.line_height,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    )
                                    .ok();
                            }
                        },
                    );
                } else {
                    for (seg_wrapped, seg_origin) in segments {
                        seg_wrapped
                            .paint(
                                *seg_origin,
                                line.line_height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            )
                            .ok();
                    }
                }
            } else {
                if let Some(clip_bounds) = line.clip_bounds {
                    window.with_content_mask(
                        Some(ContentMask {
                            bounds: clip_bounds,
                        }),
                        |window| {
                            line.wrapped
                                .paint(
                                    line.origin,
                                    line.line_height,
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                )
                                .ok();
                        },
                    );
                } else {
                    line.wrapped
                        .paint(
                            line.origin,
                            line.line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        )
                        .ok();
                }
            }
        }

        for (wrapped, origin, line_height) in &prepaint.phantom_text_lines {
            wrapped
                .paint(*origin, *line_height, TextAlign::Left, None, window, cx)
                .ok();
        }

        for (marker, origin, line_height) in &prepaint.unordered_list_markers {
            marker
                .paint(*origin, *line_height, TextAlign::Left, None, window, cx)
                .ok();
        }

        for button in &prepaint.code_block_copy_buttons {
            paint_code_block_button(button, window, cx);
        }

        for button in &prepaint.inline_diff_revert_buttons {
            paint_code_block_button(button, window, cx);
        }

        // Image previews
        for (img_bounds, img_data, corner_radius, clip_bounds) in &prepaint.image_previews {
            if let Some(clip_bounds) = clip_bounds {
                window.with_content_mask(
                    Some(ContentMask {
                        bounds: *clip_bounds,
                    }),
                    |window| {
                        window
                            .paint_image(
                                *img_bounds,
                                Corners::all(*corner_radius),
                                img_data.clone(),
                                0,
                                false,
                            )
                            .ok();
                    },
                );
            } else {
                window
                    .paint_image(
                        *img_bounds,
                        Corners::all(*corner_radius),
                        img_data.clone(),
                        0,
                        false,
                    )
                    .ok();
            }
        }

        for quad in prepaint.image_borders.drain(..) {
            window.paint_quad(quad);
        }

        for button in &prepaint.preview_zoom_buttons {
            paint_code_block_button(button, window, cx);
        }

        // Image URL-edit icons
        for (icon_bounds, _line, alpha) in &prepaint.image_edit_icons {
            let theme = &self.editor.read(cx).theme;
            let mut color = theme.text_secondary;
            color.a = *alpha;
            window
                .paint_svg(
                    *icon_bounds,
                    OPEN_SVG_PATH.into(),
                    TransformationMatrix::default(),
                    color,
                    cx,
                )
                .ok();
        }

        for (icon_bounds, icon_path, color) in &prepaint.gutter_fold_icons {
            window
                .paint_svg(
                    *icon_bounds,
                    (*icon_path).into(),
                    TransformationMatrix::default(),
                    *color,
                    cx,
                )
                .ok();
        }
        for (icon_bounds, color) in &prepaint.format_icons {
            window
                .paint_svg(
                    *icon_bounds,
                    FMT_FORMAT_SVG.into(),
                    TransformationMatrix::default(),
                    *color,
                    cx,
                )
                .ok();
        }

        paint_quads_with_optional_mask(
            &mut prepaint.marked_underlines,
            prepaint.horizontal_editor_clip_bounds,
            window,
        );

        for quad in prepaint.link_underlines.drain(..) {
            window.paint_quad(quad);
        }

        let feedback_viewport = bounds.intersect(&window.content_mask().bounds);
        self.editor
            .read(cx)
            .paint_line_drag_feedback(feedback_viewport, window);

        if pane_swapped {
            self.editor
                .update(cx, |editor, _| editor.end_pane_swap(self.pane_id));
        }
        self.editor.update(cx, |editor, _| {
            if editor.focused_pane_id == self.pane_id {
                editor.end_large_document_frame_work();
            }
        });
    }
}
