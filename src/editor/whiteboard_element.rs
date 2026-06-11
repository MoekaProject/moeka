//! Custom GPUI element that paints the whiteboard canvas. Models on
//! `image_view.rs`: it owns no element tree, instead reading the active tab's
//! whiteboard state during `prepaint` and emitting GPUI draw primitives in
//! `paint` (filled quads for backgrounds/grid, filled `Path`s for shapes and
//! strokes, shaped text for labels). Everything is computed in screen space so
//! shapes stay crisp at any zoom.

use gpui::{
    App, BorderStyle, Bounds, Corners, CursorStyle, Element, ElementId, Entity, Font,
    FontFallbacks, GlobalElementId, Hitbox, HitboxBehavior, Hsla, LayoutId, Path, Pixels, Point,
    SharedString, Style, TextAlign, TextRun, Window, WrappedLine, fill, point, px, quad, relative,
    size,
};

use super::whiteboard::{
    ElementKind, FillStyle, StrokeStyle, WhiteboardElement as Shape, WhiteboardTabState,
    with_saturation,
};
use super::whiteboard_geometry::{
    CanvasView, GRID, Pt, arrowhead, bbox_handle_points, element_corners, element_seed,
    ellipse_perimeter, ellipse_segment_count, fill_polygon, hatch_segments, label_center,
    linear_endpoints, outline_corners, resize_handle_points, rotate_glyph, roughen_polyline,
    rounded_rect_polyline, route_handles, route_points, selection_bbox, selection_outline_pad,
    sketch_stroke_polyline, split_polyline_outside_rect, stroke_polyline,
};
use super::*;
use crate::theme::{Theme, with_alpha};

pub(crate) struct WhiteboardElement {
    pub editor: Entity<EditorView>,
}

pub(crate) struct WhiteboardPrepaintState {
    hitbox: Hitbox,
    cursor: CursorStyle,
    background: Option<(Bounds<Pixels>, Hsla)>,
    grid_dots: Vec<(Bounds<Pixels>, Hsla)>,
    /// Per-element paint items, already flattened in z-order (fill, then
    /// stroke, then label for each element) so layering is respected.
    shape_items: Vec<ShapePaintItem>,
    selection: Vec<(Path<Pixels>, Hsla)>,
    handles: Vec<(Bounds<Pixels>, Hsla, Hsla)>,
    marquee: Option<(Bounds<Pixels>, Hsla, Hsla)>,
    glyph: Vec<(Path<Pixels>, Hsla)>,
}

/// A single draw command for one element, kept in z-order.
enum ShapePaintItem {
    Path(Path<Pixels>, Hsla),
    Text(Box<(WrappedLine, Point<Pixels>, Pixels)>),
}

impl gpui::IntoElement for WhiteboardElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for WhiteboardElement {
    type RequestLayoutState = ();
    type PrepaintState = WhiteboardPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
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
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let viewport_width: f32 = bounds.size.width.into();
        let viewport_height: f32 = bounds.size.height.into();
        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();

        let mut grid_dots = Vec::new();
        let mut shape_items: Vec<ShapePaintItem> = Vec::new();
        let mut selection: Vec<(Path<Pixels>, Hsla)> = Vec::new();
        let mut handles: Vec<(Bounds<Pixels>, Hsla, Hsla)> = Vec::new();
        let mut marquee = None;
        let mut cursor = CursorStyle::Arrow;
        let mut glyph: Vec<(Path<Pixels>, Hsla)> = Vec::new();

        let background = {
            let editor = self.editor.read(cx);
            let theme = &editor.theme;
            let mut background = Some((bounds, theme.bg_primary));

            if let Some(state) = editor.active_whiteboard() {
                let app_state = state.document.app_state;
                if let Some(tint) = app_state.background.resolve(theme) {
                    let alpha = app_state.background_opacity.clamp(0.0, 1.0);
                    background = Some((bounds, with_alpha(tint, alpha)));
                }
                let view = CanvasView {
                    origin_x,
                    origin_y,
                    scroll_x: app_state.scroll_x,
                    scroll_y: app_state.scroll_y,
                    zoom: app_state.zoom.clamp(0.05, 40.0),
                };

                build_grid(
                    &view,
                    theme,
                    viewport_width,
                    viewport_height,
                    &mut grid_dots,
                );

                let font = build_canvas_font(window, editor);
                let editing_id = editor.whiteboard_editing_element_id();
                for shape in &state.document.elements {
                    let editing = editing_id.as_deref() == Some(shape.id.as_str());
                    // Build each element into local buffers, then flatten in
                    // fill -> stroke -> label order so elements layer by their
                    // document (z) order.
                    let mut fills: Vec<(Path<Pixels>, Hsla)> = Vec::new();
                    let mut strokes: Vec<(Path<Pixels>, Hsla)> = Vec::new();
                    let mut texts: Vec<(WrappedLine, Point<Pixels>, Pixels)> = Vec::new();
                    build_shape(
                        shape,
                        &view,
                        theme,
                        &font,
                        window,
                        editing,
                        &mut fills,
                        &mut strokes,
                        &mut texts,
                    );
                    shape_items.extend(
                        fills
                            .into_iter()
                            .map(|(path, color)| ShapePaintItem::Path(path, color)),
                    );
                    shape_items.extend(
                        strokes
                            .into_iter()
                            .map(|(path, color)| ShapePaintItem::Path(path, color)),
                    );
                    shape_items.extend(texts.into_iter().map(|(line, origin, height)| {
                        ShapePaintItem::Text(Box::new((line, origin, height)))
                    }));
                }

                let hover_ids = editor.whiteboard_hover_highlight_ids(&view);
                build_selection(
                    state,
                    &view,
                    theme,
                    editor.whiteboard_drag.as_ref(),
                    &editor.whiteboard_bind_targets,
                    &hover_ids,
                    &mut selection,
                    &mut handles,
                    &mut marquee,
                );

                cursor = editor.whiteboard_cursor_at(&view);

                if let Some((center, orient)) = editor.whiteboard_rotate_glyph(&view) {
                    let color = theme.text_accent;
                    let (arcs, head) = rotate_glyph(center, orient, 9.0, 2.0);
                    for path in arcs {
                        glyph.push((path, color));
                    }
                    if let Some(path) = head {
                        glyph.push((path, color));
                    }
                }
            }

            background
        };

        self.editor.update(cx, |editor, _| {
            editor.last_viewport_width = viewport_width;
            editor.last_viewport_height = viewport_height;
            editor.whiteboard_canvas_bounds = Some(bounds);
        });

        WhiteboardPrepaintState {
            hitbox,
            cursor,
            background,
            grid_dots,
            shape_items,
            selection,
            handles,
            marquee,
            glyph,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.set_cursor_style(prepaint.cursor, &prepaint.hitbox);

        if let Some((bounds, color)) = prepaint.background {
            window.paint_quad(fill(bounds, color));
        }
        for (bounds, color) in prepaint.grid_dots.drain(..) {
            window.paint_quad(fill(bounds, color));
        }
        for item in prepaint.shape_items.drain(..) {
            match item {
                ShapePaintItem::Path(path, color) => {
                    window.paint_path(path, color);
                }
                ShapePaintItem::Text(text) => {
                    let (wrapped, origin, line_height) = *text;
                    wrapped
                        .paint(origin, line_height, TextAlign::Left, None, window, cx)
                        .ok();
                }
            }
        }
        for (path, color) in prepaint.selection.drain(..) {
            window.paint_path(path, color);
        }
        if let Some((bounds, fill_color, border_color)) = prepaint.marquee.take() {
            window.paint_quad(quad(
                bounds,
                Corners::all(px(0.0)),
                fill_color,
                px(1.0),
                border_color,
                BorderStyle::default(),
            ));
        }
        for (bounds, fill_color, border_color) in prepaint.handles.drain(..) {
            window.paint_quad(quad(
                bounds,
                Corners::all(px(2.0)),
                fill_color,
                px(1.5),
                border_color,
                BorderStyle::default(),
            ));
        }
        for (path, color) in prepaint.glyph.drain(..) {
            window.paint_path(path, color);
        }
    }
}

/// Build subtle grid lines across the visible viewport.
fn build_grid(
    view: &CanvasView,
    theme: &Theme,
    viewport_width: f32,
    viewport_height: f32,
    grid_dots: &mut Vec<(Bounds<Pixels>, Hsla)>,
) {
    let spacing_screen = view.scale(GRID);
    if spacing_screen < 8.0 {
        return;
    }
    let dot_color = with_alpha(theme.text_secondary, 0.28);
    let dot_size = (0.1 * spacing_screen).clamp(1.5, 2.5);

    let left = view.origin_x;
    let top = view.origin_y;
    let top_left = view.to_canvas((left, top));
    let bottom_right = view.to_canvas((left + viewport_width, top + viewport_height));

    let start_gx = (top_left.0 / GRID).floor() as i32;
    let end_gx = (bottom_right.0 / GRID).ceil() as i32;
    let start_gy = (top_left.1 / GRID).floor() as i32;
    let end_gy = (bottom_right.1 / GRID).ceil() as i32;
    let nx = (end_gx - start_gx).max(0) as i64 + 1;
    let ny = (end_gy - start_gy).max(0) as i64 + 1;
    if nx * ny > 40_000 {
        return;
    }

    // A dot at every grid intersection.
    let half = dot_size * 0.5;
    for gx in start_gx..=end_gx {
        let (sx, _) = view.to_screen((gx as f32 * GRID, 0.0));
        for gy in start_gy..=end_gy {
            let (_, sy) = view.to_screen((0.0, gy as f32 * GRID));
            grid_dots.push((
                Bounds::new(
                    point(px(sx - half), px(sy - half)),
                    size(px(dot_size), px(dot_size)),
                ),
                dot_color,
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_shape(
    shape: &Shape,
    view: &CanvasView,
    theme: &Theme,
    font: &Font,
    window: &Window,
    editing: bool,
    fills: &mut Vec<(Path<Pixels>, Hsla)>,
    strokes: &mut Vec<(Path<Pixels>, Hsla)>,
    texts: &mut Vec<(WrappedLine, Point<Pixels>, Pixels)>,
) {
    // The opacity setting applies to the whole element: stroke, fill, and
    // any label text. A transparent stroke color draws no stroke at all.
    let opacity = shape.background.opacity.clamp(0.0, 1.0);
    let stroke_visible = shape.stroke.color.resolve(theme).is_some();
    let stroke_color = with_alpha(shape.stroke.color.resolve_stroke(theme), opacity);
    let stroke_width = (shape.stroke.width.px() * view.zoom).max(0.75);
    let fill_color = shape
        .background
        .color
        .resolve(theme)
        .map(|color| with_alpha(with_saturation(color, shape.background.saturation), opacity));

    let roughness = shape.stroke.roughness;
    let amplitude = roughness.amplitude() * view.zoom;
    let passes = roughness.passes();
    let width_variation = roughness.width_variation();
    let overdraw_alpha = roughness.overdraw_alpha();
    let seed = element_seed(&shape.id);

    // Stroke a closed/open polyline. The hand-drawn styles draw two layered
    // passes — a full-strength stroke plus a lighter, rougher overdraw — with
    // pen-pressure width variation on solid strokes.
    let mut push_outline = |points: &[Pt], closed: bool| {
        if !stroke_visible {
            return;
        }
        if amplitude <= 0.0 {
            for path in stroke_polyline(points, stroke_width, shape.stroke.style, closed) {
                strokes.push((path, stroke_color));
            }
            return;
        }
        for pass in 0..passes {
            let pass_seed = seed.wrapping_add(pass as u64 * 0x9e37);
            let pass_amplitude = if pass == 0 {
                amplitude
            } else {
                amplitude * 1.4
            };
            let color = if pass == 0 {
                stroke_color
            } else {
                with_alpha(stroke_color, stroke_color.a * overdraw_alpha)
            };
            let mut pts = roughen_polyline(points, closed, pass_amplitude, pass_seed);
            if shape.stroke.style == StrokeStyle::Solid {
                if closed && let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
                for path in sketch_stroke_polyline(&pts, stroke_width, width_variation, pass_seed) {
                    strokes.push((path, color));
                }
            } else {
                for path in stroke_polyline(&pts, stroke_width, shape.stroke.style, closed) {
                    strokes.push((path, color));
                }
            }
        }
    };

    match shape.kind {
        ElementKind::Rectangle => {
            let radius = shape.corner_radius.px();
            let canvas_pts = rounded_rect_polyline(shape, radius, 6);
            let screen: Vec<Pt> = canvas_pts
                .iter()
                .map(|corner| view.to_screen(*corner))
                .collect();
            if let Some(color) = fill_color {
                push_fill(&screen, shape.background.fill, color, view.zoom, fills);
            }
            push_outline(&screen, true);
            if !editing {
                build_label(shape, view, theme, font, window, texts);
            }
        }
        ElementKind::Ellipse => {
            let corners = element_corners(shape).map(|corner| view.to_screen(corner));
            let screen_w = (corners[1].0 - corners[0].0).hypot(corners[1].1 - corners[0].1);
            let screen_h = (corners[3].0 - corners[0].0).hypot(corners[3].1 - corners[0].1);
            let segments = ellipse_segment_count(screen_w, screen_h);
            let screen: Vec<Pt> = ellipse_perimeter(shape, segments)
                .iter()
                .map(|corner| view.to_screen(*corner))
                .collect();
            if let Some(color) = fill_color {
                push_fill(&screen, shape.background.fill, color, view.zoom, fills);
            }
            push_outline(&screen, true);
            if !editing {
                build_label(shape, view, theme, font, window, texts);
            }
        }
        ElementKind::Line => {
            let screen: Vec<Pt> = route_points(shape)
                .iter()
                .map(|p| view.to_screen(*p))
                .collect();
            // The label gap stays live while editing (text is synced into the
            // element), but the text itself is drawn by the edit overlay.
            let label = layout_label(shape, view, theme, font, window);
            push_linear_outline(
                &screen,
                label.as_ref().map(|l| l.1),
                view,
                &mut push_outline,
            );
            if !editing && let Some((items, _)) = label {
                texts.extend(items);
            }
        }
        ElementKind::Arrow => {
            let route: Vec<Pt> = route_points(shape)
                .iter()
                .map(|p| view.to_screen(*p))
                .collect();
            let label = layout_label(shape, view, theme, font, window);
            push_linear_outline(&route, label.as_ref().map(|l| l.1), view, &mut push_outline);
            let head_size = (stroke_width * 3.2 + 6.0).min(46.0);
            if stroke_visible && route.len() >= 2 {
                let prev = route[route.len() - 2];
                let end = route[route.len() - 1];
                if let Some(path) = arrowhead(prev, end, head_size) {
                    fills.push((path, stroke_color));
                }
            }
            if !editing && let Some((items, _)) = label {
                texts.extend(items);
            }
        }
        ElementKind::Text => {
            if !editing {
                build_text(shape, view, theme, font, window, texts);
            }
        }
    }
}

/// Fill a convex screen-space polygon with the given style: solid, or diagonal
/// hatching (single or crossed) in the background color.
fn push_fill(
    screen: &[Pt],
    fill: FillStyle,
    color: Hsla,
    zoom: f32,
    fills: &mut Vec<(Path<Pixels>, Hsla)>,
) {
    match fill {
        FillStyle::Solid => {
            if let Some(path) = fill_polygon(screen) {
                fills.push((path, color));
            }
        }
        FillStyle::Hatch | FillStyle::CrossHatch => {
            let spacing = (9.0 * zoom).max(3.5);
            let half_width = (0.6 * zoom).clamp(0.35, 2.0);
            let mut push_lines = |flip: bool| {
                for (a, b) in hatch_segments(screen, spacing, flip) {
                    for path in
                        stroke_polyline(&[a, b], half_width * 2.0, StrokeStyle::Solid, false)
                    {
                        fills.push((path, color));
                    }
                }
            };
            push_lines(true);
            if fill == FillStyle::CrossHatch {
                push_lines(false);
            }
        }
    }
}

/// Stroke a linear element's route, leaving a padded gap around its label
/// rect (if any) so the line never crosses the text.
fn push_linear_outline(
    screen: &[Pt],
    label_bounds: Option<(Pt, Pt)>,
    view: &CanvasView,
    push_outline: &mut impl FnMut(&[Pt], bool),
) {
    if let Some((min, max)) = label_bounds {
        let pad = 4.0 + 3.0 * view.zoom;
        let padded_min = (min.0 - pad, min.1 - pad);
        let padded_max = (max.0 + pad, max.1 + pad);
        for piece in split_polyline_outside_rect(screen, padded_min, padded_max) {
            push_outline(&piece, false);
        }
    } else {
        push_outline(screen, false);
    }
}

/// Shape a standalone text element, anchored at its box top-left and aligned
/// within the box width.
fn build_text(
    shape: &Shape,
    view: &CanvasView,
    theme: &Theme,
    font: &Font,
    window: &Window,
    texts: &mut Vec<(WrappedLine, Point<Pixels>, Pixels)>,
) {
    if shape.text.is_empty() {
        return;
    }
    let font_size = (shape.font.size.px() * view.zoom).max(6.0);
    let color = with_alpha(
        shape
            .font
            .effective_color(shape.stroke.color)
            .resolve_stroke(theme),
        shape.background.opacity.clamp(0.0, 1.0),
    );
    let font = canvas_font_for(shape, font);
    let run = TextRun {
        len: shape.text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let Ok(lines) = window.text_system().shape_text(
        shape.text.clone().into(),
        px(font_size),
        &[run],
        None,
        None,
    ) else {
        return;
    };
    let line_height = px(font_size * 1.3);
    let (origin_x, origin_y) = view.to_screen((shape.x, shape.y));
    // Align lines within the wider of the element box and the text block.
    let max_line_w = lines
        .iter()
        .map(|line| f32::from(line.width()))
        .fold(0.0_f32, f32::max);
    let block_w = (shape.width.abs() * view.zoom).max(max_line_w);
    let mut y = origin_y;
    for wrapped in lines {
        let w = f32::from(wrapped.width());
        let x = match shape.font.align {
            CanvasTextAlign::Left => origin_x,
            CanvasTextAlign::Center => origin_x + (block_w - w) * 0.5,
            CanvasTextAlign::Right => origin_x + block_w - w,
        };
        texts.push((wrapped, point(px(x), px(y)), line_height));
        y += f32::from(line_height);
    }
}

/// Shape a centered text label for a shape or linear element.
fn build_label(
    shape: &Shape,
    view: &CanvasView,
    theme: &Theme,
    font: &Font,
    window: &Window,
    texts: &mut Vec<(WrappedLine, Point<Pixels>, Pixels)>,
) {
    if let Some((items, _)) = layout_label(shape, view, theme, font, window) {
        texts.extend(items);
    }
}

/// Lay out an element's text label: shaped lines with their screen positions,
/// plus the screen-space bounds of the text block. The label is centered on
/// the label anchor (route midpoint for linear elements, box center for
/// shapes); individual lines align within the block per the text-align
/// setting.
#[allow(clippy::type_complexity)]
fn layout_label(
    shape: &Shape,
    view: &CanvasView,
    theme: &Theme,
    font: &Font,
    window: &Window,
) -> Option<(Vec<(WrappedLine, Point<Pixels>, Pixels)>, (Pt, Pt))> {
    if shape.text.is_empty() || !shape.kind.supports_label() {
        return None;
    }
    let font_size = (shape.font.size.px() * view.zoom).max(6.0);
    let color = with_alpha(
        shape
            .font
            .effective_color(shape.stroke.color)
            .resolve_stroke(theme),
        shape.background.opacity.clamp(0.0, 1.0),
    );
    let font = canvas_font_for(shape, font);
    let run = TextRun {
        len: shape.text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let lines = window
        .text_system()
        .shape_text(shape.text.clone().into(), px(font_size), &[run], None, None)
        .ok()?;
    let line_height = font_size * 1.3;
    let total_height = line_height * lines.len() as f32;
    let center = label_center(shape);
    let (center_x, center_y) = view.to_screen(center);
    let mut y = center_y - total_height * 0.5;
    let top = y;
    // The text block is centered on the label anchor; individual lines align
    // within the block according to the text-align setting.
    let block_w = lines
        .iter()
        .map(|line| f32::from(line.width()))
        .fold(0.0_f32, f32::max);
    let block_left = center_x - block_w * 0.5;
    let mut items = Vec::with_capacity(lines.len());
    for wrapped in lines {
        let width = f32::from(wrapped.width());
        let x = match shape.font.align {
            CanvasTextAlign::Left => block_left,
            CanvasTextAlign::Center => center_x - width * 0.5,
            CanvasTextAlign::Right => block_left + block_w - width,
        };
        items.push((wrapped, point(px(x), px(y)), px(line_height)));
        y += line_height;
    }
    Some((
        items,
        (
            (block_left, top),
            (block_left + block_w, top + total_height),
        ),
    ))
}

/// The shaped-text font for an element: the base canvas font with the
/// element's family override applied (CJK fallbacks preserved).
fn canvas_font_for(shape: &Shape, base: &Font) -> Font {
    let mut font = base.clone();
    if let Some(family) = &shape.font.family {
        font.family = family.clone().into();
    }
    font
}

/// Build selection overlays: bounding-box outlines, transform handles, and the
/// marquee rectangle.
#[allow(clippy::too_many_arguments)]
fn build_selection(
    state: &WhiteboardTabState,
    view: &CanvasView,
    theme: &Theme,
    drag: Option<&WhiteboardDrag>,
    bind_targets: &[String],
    hover_ids: &[String],
    selection: &mut Vec<(Path<Pixels>, Hsla)>,
    handles: &mut Vec<(Bounds<Pixels>, Hsla, Hsla)>,
    marquee: &mut Option<(Bounds<Pixels>, Hsla, Hsla)>,
) {
    let accent = theme.text_accent;
    let handle_fill = theme.bg_primary;
    // Outlines and combined boxes sit slightly outside the painted strokes.
    let bbox_pad = super::whiteboard_geometry::BBOX_OUTLINE_PAD;

    // Hover highlight: a blue outline without handles (single element, or the
    // combined box of a hovered group).
    if !hover_ids.is_empty() {
        if hover_ids.len() > 1 {
            if let Some((min, max)) = selection_bbox(
                state
                    .document
                    .elements
                    .iter()
                    .filter(|element| hover_ids.iter().any(|id| id == &element.id)),
            ) {
                let smin = view.to_screen(min);
                let smax = view.to_screen(max);
                let smin = (smin.0 - bbox_pad, smin.1 - bbox_pad);
                let smax = (smax.0 + bbox_pad, smax.1 + bbox_pad);
                let outline = vec![
                    (smin.0, smin.1),
                    (smax.0, smin.1),
                    (smax.0, smax.1),
                    (smin.0, smax.1),
                ];
                for path in stroke_polyline(&outline, 1.5, StrokeStyle::Solid, true) {
                    selection.push((path, accent));
                }
            }
        } else if let Some(element) = state.document.element(&hover_ids[0]) {
            if element.kind.is_linear() {
                let pts: Vec<Pt> = route_points(element)
                    .iter()
                    .map(|p| view.to_screen(*p))
                    .collect();
                for path in stroke_polyline(&pts, 1.5, StrokeStyle::Solid, false) {
                    selection.push((path, accent));
                }
            } else {
                let pad = selection_outline_pad(element);
                let corners: Vec<Pt> = outline_corners(element, pad)
                    .iter()
                    .map(|corner| view.to_screen(*corner))
                    .collect();
                for path in stroke_polyline(&corners, 1.5, StrokeStyle::Solid, true) {
                    selection.push((path, accent));
                }
            }
        }
    }

    // Highlight elements a linear endpoint is about to bind to, using the
    // same thin blue outline as hover.
    for target_id in bind_targets {
        let Some(target) = state.document.element(target_id) else {
            continue;
        };
        let highlight = accent;
        if target.kind.is_linear() {
            let pts: Vec<Pt> = route_points(target)
                .iter()
                .map(|p| view.to_screen(*p))
                .collect();
            for path in stroke_polyline(&pts, 1.5, StrokeStyle::Solid, false) {
                selection.push((path, highlight));
            }
        } else {
            let corners: Vec<Pt> = element_corners(target)
                .iter()
                .map(|corner| view.to_screen(*corner))
                .collect();
            for path in stroke_polyline(&corners, 1.5, StrokeStyle::Solid, true) {
                selection.push((path, highlight));
            }
        }
    }

    // Per-element outlines/handles only for a single selection; multi-element
    // selections (groups) show just the combined bounding box below.
    if state.selected_ids.len() == 1 {
        for shape in state.selected_elements() {
            if shape.kind.is_linear() {
                let (a, b) = linear_endpoints(shape);
                push_handle(handles, view.to_screen(a), handle_fill, accent);
                push_handle(handles, view.to_screen(b), handle_fill, accent);
            } else {
                let pad = selection_outline_pad(shape);
                let corners: Vec<Pt> = outline_corners(shape, pad)
                    .iter()
                    .map(|corner| view.to_screen(*corner))
                    .collect();
                for path in stroke_polyline(&corners, 1.5, StrokeStyle::Solid, true) {
                    selection.push((path, accent));
                }
            }
        }
    }

    // Resize handles are shown only for a single non-linear selection.
    if state.selected_ids.len() == 1
        && let Some(shape) = state.document.element(&state.selected_ids[0])
        && !shape.kind.is_linear()
    {
        for handle_point in resize_handle_points(shape, view) {
            push_handle(handles, handle_point, handle_fill, accent);
        }
    }

    // Multi-selection (e.g. a group): one combined bounding box with its own
    // transform handles.
    if state.selected_ids.len() > 1
        && let Some((min, max)) = selection_bbox(state.selected_elements())
    {
        let smin = view.to_screen(min);
        let smax = view.to_screen(max);
        let smin = (smin.0 - bbox_pad, smin.1 - bbox_pad);
        let smax = (smax.0 + bbox_pad, smax.1 + bbox_pad);
        let outline = vec![
            (smin.0, smin.1),
            (smax.0, smin.1),
            (smax.0, smax.1),
            (smin.0, smax.1),
        ];
        for path in stroke_polyline(&outline, 1.5, StrokeStyle::Solid, true) {
            selection.push((path, accent));
        }
        for handle_point in bbox_handle_points(smin, smax) {
            push_handle(handles, handle_point, handle_fill, accent);
        }
    }

    // Route handles (orthogonal bends / curve apex) for a single linear
    // selection.
    if state.selected_ids.len() == 1
        && let Some(shape) = state.document.element(&state.selected_ids[0])
    {
        for (_, handle_point) in route_handles(shape, view) {
            push_handle(handles, handle_point, accent, theme.bg_primary);
        }
    }

    if let Some(WhiteboardDrag::Marquee { start, current }) = drag {
        let a = view.to_screen(*start);
        let b = view.to_screen(*current);
        let min = (a.0.min(b.0), a.1.min(b.1));
        let max = (a.0.max(b.0), a.1.max(b.1));
        let bounds = Bounds::new(
            point(px(min.0), px(min.1)),
            size(px(max.0 - min.0), px(max.1 - min.1)),
        );
        *marquee = Some((bounds, with_alpha(accent, 0.08), with_alpha(accent, 0.5)));
    }
}

fn push_handle(
    handles: &mut Vec<(Bounds<Pixels>, Hsla, Hsla)>,
    center: Pt,
    fill_color: Hsla,
    border_color: Hsla,
) {
    const SIZE: f32 = 8.0;
    handles.push((
        Bounds::new(
            point(px(center.0 - SIZE * 0.5), px(center.1 - SIZE * 0.5)),
            size(px(SIZE), px(SIZE)),
        ),
        fill_color,
        border_color,
    ));
}

/// Build the canvas text font from the editor's body font configuration so CJK
/// text renders correctly.
fn build_canvas_font(window: &Window, editor: &EditorView) -> Font {
    let text_style = window.text_style();
    let styles = &editor.styles.editor;
    let mut font = text_style.font();
    let latin = styles.editor_body_latin_font_family;
    let family: SharedString = if latin == ".SystemUIFont" {
        text_style.font().family.clone()
    } else {
        latin.into()
    };
    font.family = family;
    font.fallbacks = (latin != styles.editor_body_cjk_font_family)
        .then(|| FontFallbacks::from_fonts(vec![styles.editor_body_cjk_font_family.to_string()]));
    font
}
