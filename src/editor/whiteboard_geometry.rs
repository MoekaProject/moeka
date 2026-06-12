//! Geometry helpers for the whiteboard canvas: coordinate transforms between
//! canvas space and screen space, rotated element geometry, and builders that
//! turn polylines/polygons into filled GPUI `Path`s (GPUI only fills paths, so
//! strokes are emitted as thin filled quads — this keeps everything crisp at
//! any zoom level).

use gpui::{Path, Pixels, point, px};

use super::whiteboard::{ElementKind, RouteHandle, RouteStyle, StrokeStyle, WhiteboardElement};

/// A 2D point in either canvas or screen space, depending on context.
pub(crate) type Pt = (f32, f32);

/// Grid spacing in canvas units. Used for the painted grid lines, move
/// snapping, and keyboard nudges.
pub(crate) const GRID: f32 = 20.0;

/// Screen-pixel padding applied to multi-selection / group bounding boxes so
/// the outline and its handles sit outside the painted shapes.
pub(crate) const BBOX_OUTLINE_PAD: f32 = 6.0;

/// Snap a canvas-space coordinate to the grid.
pub(crate) fn snap_to_grid(value: f32) -> f32 {
    (value / GRID).round() * GRID
}

/// Maps between canvas coordinates (the document's own space) and screen
/// coordinates (pixels within the canvas element bounds).
///
/// `screen = (canvas - scroll) * zoom + origin`
#[derive(Clone, Copy, Debug)]
pub(crate) struct CanvasView {
    pub origin_x: f32,
    pub origin_y: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub zoom: f32,
}

impl CanvasView {
    pub(crate) fn to_screen(self, p: Pt) -> Pt {
        (
            (p.0 - self.scroll_x) * self.zoom + self.origin_x,
            (p.1 - self.scroll_y) * self.zoom + self.origin_y,
        )
    }

    pub(crate) fn to_canvas(self, p: Pt) -> Pt {
        (
            (p.0 - self.origin_x) / self.zoom + self.scroll_x,
            (p.1 - self.origin_y) / self.zoom + self.scroll_y,
        )
    }

    /// Scale a canvas-space length into screen pixels.
    pub(crate) fn scale(&self, value: f32) -> f32 {
        value * self.zoom
    }
}

fn rotate_around(p: Pt, center: Pt, sin: f32, cos: f32) -> Pt {
    let dx = p.0 - center.0;
    let dy = p.1 - center.1;
    (
        center.0 + dx * cos - dy * sin,
        center.1 + dx * sin + dy * cos,
    )
}

/// The four corners of an element's (possibly rotated) box in canvas space,
/// ordered top-left, top-right, bottom-right, bottom-left.
pub(crate) fn element_corners(element: &WhiteboardElement) -> [Pt; 4] {
    let center = element.center();
    let (sin, cos) = element.rotation.sin_cos();
    let x0 = element.x;
    let y0 = element.y;
    let x1 = element.x + element.width;
    let y1 = element.y + element.height;
    [
        rotate_around((x0, y0), center, sin, cos),
        rotate_around((x1, y0), center, sin, cos),
        rotate_around((x1, y1), center, sin, cos),
        rotate_around((x0, y1), center, sin, cos),
    ]
}

/// Canvas-units padding for an element's selection/hover outline so it sits
/// outside the painted stroke, however thick.
pub(crate) fn selection_outline_pad(element: &WhiteboardElement) -> f32 {
    element.stroke.width.px() * 0.5 + 4.0
}

/// The element's rotated corners inflated outward by `pad` canvas units on
/// every side (for selection outlines drawn outside the shape).
pub(crate) fn outline_corners(element: &WhiteboardElement, pad: f32) -> [Pt; 4] {
    let center = element.center();
    let (sin, cos) = element.rotation.sin_cos();
    let x0 = element.x.min(element.x + element.width) - pad;
    let y0 = element.y.min(element.y + element.height) - pad;
    let x1 = element.x.max(element.x + element.width) + pad;
    let y1 = element.y.max(element.y + element.height) + pad;
    [
        rotate_around((x0, y0), center, sin, cos),
        rotate_around((x1, y0), center, sin, cos),
        rotate_around((x1, y1), center, sin, cos),
        rotate_around((x0, y1), center, sin, cos),
    ]
}

/// The two endpoints of a linear element (arrow/line) in canvas space, with the
/// element's rotation applied around its center.
pub(crate) fn linear_endpoints(element: &WhiteboardElement) -> (Pt, Pt) {
    let center = element.center();
    let (sin, cos) = element.rotation.sin_cos();
    let start = rotate_around((element.x, element.y), center, sin, cos);
    let end = rotate_around(
        (element.x + element.width, element.y + element.height),
        center,
        sin,
        cos,
    );
    (start, end)
}

/// Compute the polyline a linear element follows, honoring its routing mode.
/// For `Straight`, this is just `[start, end]`. For `Orthogonal`, an elbow path
/// with the bend at `route_mid` along the dominant axis. For `Curved`, a
/// quadratic Bézier sampled into segments, with the apex offset `route_curve`
/// perpendicular to the chord.
pub(crate) fn route_points(element: &WhiteboardElement) -> Vec<Pt> {
    let (start, end) = linear_endpoints(element);
    match element.route {
        RouteStyle::Straight => vec![start, end],
        RouteStyle::Orthogonal => {
            let dx = end.0 - start.0;
            let dy = end.1 - start.1;
            let t1 = element.route_mid.clamp(0.0, 1.0);
            let t2 = element.route_mid2.unwrap_or(element.route_mid).clamp(0.0, 1.0);
            let cross = element.route_cross;
            if dx.abs() >= dy.abs() {
                let x1 = start.0 + dx * t1;
                let x2 = start.0 + dx * t2;
                let ym = start.1 + dy * cross;
                vec![
                    start,
                    (x1, start.1),
                    (x1, ym),
                    (x2, ym),
                    (x2, end.1),
                    end,
                ]
            } else {
                let y1 = start.1 + dy * t1;
                let y2 = start.1 + dy * t2;
                let xm = start.0 + dx * cross;
                vec![
                    start,
                    (start.0, y1),
                    (xm, y1),
                    (xm, y2),
                    (end.0, y2),
                    end,
                ]
            }
        }
        RouteStyle::Curved => {
            let control = curve_control_point(
                start,
                end,
                element.route_curve,
                element.route_curve_t,
            );
            let segs = 28;
            (0..=segs)
                .map(|i| {
                    let t = i as f32 / segs as f32;
                    let mt = 1.0 - t;
                    (
                        mt * mt * start.0 + 2.0 * mt * t * control.0 + t * t * end.0,
                        mt * mt * start.1 + 2.0 * mt * t * control.1 + t * t * end.1,
                    )
                })
                .collect()
        }
    }
}

/// Quadratic-Bézier control point for a curved route: the chord midpoint moved
/// `2 * offset` along the chord normal and tangent (so the curve apex sits at
/// `(offset_n, offset_t)` in chord-local coordinates).
fn curve_control_point(start: Pt, end: Pt, offset_n: f32, offset_t: f32) -> Pt {
    let mid = ((start.0 + end.0) * 0.5, (start.1 + end.1) * 0.5);
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let tx = dx / len;
    let ty = dy / len;
    let nx = -ty;
    let ny = tx;
    (
        mid.0 + (nx * offset_n + tx * offset_t) * 2.0,
        mid.1 + (ny * offset_n + ty * offset_t) * 2.0,
    )
}

/// Screen-space positions of the adjustable route handles for a routed linear
/// element: two side bends plus the cross segment for orthogonal routes, the
/// curve apex for curved routes, nothing for straight ones.
pub(crate) fn route_handles(
    element: &WhiteboardElement,
    view: &CanvasView,
) -> Vec<(RouteHandle, Pt)> {
    match element.route {
        RouteStyle::Straight => Vec::new(),
        RouteStyle::Orthogonal => {
            let pts = route_points(element);
            if pts.len() < 6 {
                return Vec::new();
            }
            let mid = |a: Pt, b: Pt| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
            vec![
                (RouteHandle::SideStart, view.to_screen(mid(pts[1], pts[2]))),
                (RouteHandle::Cross, view.to_screen(mid(pts[2], pts[3]))),
                (RouteHandle::SideEnd, view.to_screen(mid(pts[3], pts[4]))),
            ]
        }
        RouteStyle::Curved => {
            let (start, end) = linear_endpoints(element);
            let mid = ((start.0 + end.0) * 0.5, (start.1 + end.1) * 0.5);
            let dx = end.0 - start.0;
            let dy = end.1 - start.1;
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            let tx = dx / len;
            let ty = dy / len;
            let nx = -ty;
            let ny = tx;
            vec![(
                RouteHandle::Cross,
                view.to_screen((
                    mid.0 + nx * element.route_curve + tx * element.route_curve_t,
                    mid.1 + ny * element.route_curve + ty * element.route_curve_t,
                )),
            )]
        }
    }
}

/// Position (0..1, clamped with overshoot) of `pointer` along the
/// non-dominant axis, used to drag an orthogonal route's cross segment.
pub(crate) fn route_cross_for_pointer(element: &WhiteboardElement, pointer: Pt) -> f32 {
    let (start, end) = linear_endpoints(element);
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    if dx.abs() >= dy.abs() {
        if dy.abs() < 1e-3 {
            0.5
        } else {
            ((pointer.1 - start.1) / dy).clamp(-1.5, 2.5)
        }
    } else if dx.abs() < 1e-3 {
        0.5
    } else {
        ((pointer.0 - start.0) / dx).clamp(-1.5, 2.5)
    }
}

/// Chord-local offsets of `pointer` from the element's chord midpoint
/// (perpendicular, tangential), used to place the curve apex freely while
/// dragging the waypoint of a curved route.
pub(crate) fn route_curve_offsets_for_pointer(
    element: &WhiteboardElement,
    pointer: Pt,
) -> (f32, f32) {
    let (start, end) = linear_endpoints(element);
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let tx = dx / len;
    let ty = dy / len;
    let nx = -ty;
    let ny = tx;
    let mid = ((start.0 + end.0) * 0.5, (start.1 + end.1) * 0.5);
    let px_ = pointer.0 - mid.0;
    let py_ = pointer.1 - mid.1;
    (
        (px_ * nx + py_ * ny).clamp(-600.0, 600.0),
        (px_ * tx + py_ * ty).clamp(-600.0, 600.0),
    )
}

/// Update `route_mid` from a pointer position (canvas space) for the waypoint
/// drag.
pub(crate) fn route_mid_for_pointer(element: &WhiteboardElement, pointer: Pt) -> f32 {
    let (start, end) = linear_endpoints(element);
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let mid = if dx.abs() >= dy.abs() {
        if dx.abs() < 1e-3 {
            0.5
        } else {
            (pointer.0 - start.0) / dx
        }
    } else if dy.abs() < 1e-3 {
        0.5
    } else {
        (pointer.1 - start.1) / dy
    };
    mid.clamp(0.05, 0.95)
}

/// The point on `target`'s boundary facing `from` (canvas space), used to snap a
/// magnetically bound linear endpoint to the element edge. Honors the target's
/// rotation and shape (rectangle/ellipse vs box for others).
pub(crate) fn element_edge_point(target: &WhiteboardElement, from: Pt) -> Pt {
    let center = target.center();
    // Transform `from` into the target's unrotated local frame.
    let (sin_inv, cos_inv) = (-target.rotation).sin_cos();
    let local = rotate_around(from, center, sin_inv, cos_inv);
    let dx = local.0 - center.0;
    let dy = local.1 - center.1;
    let hx = (target.width.abs() * 0.5).max(1.0);
    let hy = (target.height.abs() * 0.5).max(1.0);
    if dx.abs() < 1e-3 && dy.abs() < 1e-3 {
        return center;
    }
    // Axis-aligned elements: snap to the midpoint of the facing side so
    // connectors attach at edge centers.
    if target.rotation.abs() < 0.05 {
        let sx = dx / hx;
        let sy = dy / hy;
        return if sx.abs() >= sy.abs() {
            (center.0 + hx * sx.signum(), center.1)
        } else {
            (center.0, center.1 + hy * sy.signum())
        };
    }
    let t = match target.kind {
        ElementKind::Ellipse => {
            let nx = dx / hx;
            let ny = dy / hy;
            1.0 / (nx * nx + ny * ny).sqrt()
        }
        _ => {
            let tx = hx / dx.abs().max(1e-3);
            let ty = hy / dy.abs().max(1e-3);
            tx.min(ty)
        }
    };
    let edge_local = (center.0 + dx * t, center.1 + dy * t);
    // Rotate back into world space.
    let (sin, cos) = target.rotation.sin_cos();
    rotate_around(edge_local, center, sin, cos)
}

/// Sample points around an ellipse perimeter in canvas space, with rotation
/// applied. `segments` controls smoothness.
pub(crate) fn ellipse_perimeter(element: &WhiteboardElement, segments: usize) -> Vec<Pt> {
    let center = element.center();
    let rx = element.width * 0.5;
    let ry = element.height * 0.5;
    let (sin, cos) = element.rotation.sin_cos();
    let segments = segments.max(8);
    (0..segments)
        .map(|i| {
            let angle = std::f32::consts::TAU * (i as f32) / (segments as f32);
            let lx = rx * angle.cos();
            let ly = ry * angle.sin();
            (
                center.0 + lx * cos - ly * sin,
                center.1 + lx * sin + ly * cos,
            )
        })
        .collect()
}

/// Number of perimeter segments to use for an ellipse of the given on-screen
/// size (more segments when larger, for smoothness).
pub(crate) fn ellipse_segment_count(screen_width: f32, screen_height: f32) -> usize {
    let larger = screen_width.abs().max(screen_height.abs());
    ((larger * 0.6) as usize).clamp(24, 160)
}

/// Build a filled convex polygon path from screen-space points (triangle fan
/// from the first vertex — correct for the convex shapes we draw).
pub(crate) fn fill_polygon(points: &[Pt]) -> Option<Path<Pixels>> {
    if points.len() < 3 {
        return None;
    }
    let mut path = Path::new(point(px(points[0].0), px(points[0].1)));
    for p in &points[1..] {
        path.line_to(point(px(p.0), px(p.1)));
    }
    Some(path)
}

/// Vertex quad for a single thick segment between two screen points.
pub(crate) fn segment_quad_points(a: Pt, b: Pt, half_width: f32) -> Option<[Pt; 4]> {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return None;
    }
    let nx = -dy / len * half_width;
    let ny = dx / len * half_width;
    Some([
        (a.0 + nx, a.1 + ny),
        (b.0 + nx, b.1 + ny),
        (b.0 - nx, b.1 - ny),
        (a.0 - nx, a.1 - ny),
    ])
}

/// A single thick segment between two screen points, as a filled quad path.
#[allow(dead_code)]
fn segment_quad(a: Pt, b: Pt, half_width: f32) -> Option<Path<Pixels>> {
    fill_polygon(&segment_quad_points(a, b, half_width)?)
}

/// Emit dashed/dotted quads along a whole polyline with a continuous dash
/// phase, so densely sampled curves (ellipses, rounded corners) keep the same
/// dash/gap rhythm as straight segments.
fn push_dashed_quads(
    out: &mut Vec<[Pt; 4]>,
    points: &[Pt],
    closed: bool,
    half_width: f32,
    dash: f32,
    gap: f32,
) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let period = (dash + gap).max(0.5);
    let count = if closed { n } else { n - 1 };
    // Arc length traveled along the polyline before the current segment. The
    // k-th dash occupies [k*period, k*period + dash) in this coordinate;
    // iterating dash indices always terminates, unlike accumulating float
    // steps, which can underflow near a period boundary and stall forever.
    let mut traveled = 0.0_f32;
    for i in 0..count {
        let a = points[i];
        let b = points[(i + 1) % n];
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            continue;
        }
        let ux = dx / len;
        let uy = dy / len;
        let seg_start = traveled;
        let seg_end = traveled + len;
        let mut k = (seg_start / period).floor().max(0.0);
        loop {
            let dash_start = k * period;
            if dash_start >= seg_end {
                break;
            }
            let s0 = dash_start.max(seg_start);
            let s1 = (dash_start + dash).min(seg_end);
            if s1 > s0 {
                let t0 = s0 - seg_start;
                let t1 = s1 - seg_start;
                let p0 = (a.0 + ux * t0, a.1 + uy * t0);
                let p1 = (a.0 + ux * t1, a.1 + uy * t1);
                if let Some(quad) = segment_quad_points(p0, p1, half_width) {
                    out.push(quad);
                }
            }
            k += 1.0;
        }
        traveled = seg_end;
    }
}

/// Stroke a polyline (or closed polygon) into filled vertex quads, honoring
/// the dash/dot style. `width` is in screen pixels.
pub(crate) fn stroke_polyline_quads(
    points: &[Pt],
    width: f32,
    style: StrokeStyle,
    closed: bool,
) -> Vec<[Pt; 4]> {
    let mut out = Vec::new();
    let n = points.len();
    if n < 2 {
        return out;
    }
    let half = (width * 0.5).max(0.35);
    match style {
        StrokeStyle::Solid => {
            let count = if closed { n } else { n - 1 };
            for i in 0..count {
                if let Some(quad) = segment_quad_points(points[i], points[(i + 1) % n], half) {
                    out.push(quad);
                }
            }
        }
        StrokeStyle::Dashed => {
            push_dashed_quads(&mut out, points, closed, half, width * 2.6, width * 2.0);
        }
        StrokeStyle::Dotted => {
            push_dashed_quads(&mut out, points, closed, half, width.max(1.0), width * 1.6);
        }
    }
    out
}

/// Build the stroke for a polyline (or closed polygon) as a list of filled
/// quad paths, honoring the dash/dot style. `width` is in screen pixels.
pub(crate) fn stroke_polyline(
    points: &[Pt],
    width: f32,
    style: StrokeStyle,
    closed: bool,
) -> Vec<Path<Pixels>> {
    stroke_polyline_quads(points, width, style, closed)
        .into_iter()
        .filter_map(|quad| fill_polygon(&quad))
        .collect()
}

/// Triangle vertices for an arrowhead pointing from `start` toward `end`
/// (screen space). `size` is the barb length in pixels.
pub(crate) fn arrowhead_points(start: Pt, end: Pt, size: f32) -> Option<[Pt; 3]> {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return None;
    }
    let ux = dx / len;
    let uy = dy / len;
    let angle = 0.45_f32;
    let (sin, cos) = angle.sin_cos();
    let barb1 = (-ux * cos + uy * sin, -uy * cos - ux * sin);
    let barb2 = (-ux * cos - uy * sin, -uy * cos + ux * sin);
    let p1 = (end.0 + barb1.0 * size, end.1 + barb1.1 * size);
    let p2 = (end.0 + barb2.0 * size, end.1 + barb2.1 * size);
    Some([end, p1, p2])
}

/// A filled triangular arrowhead pointing from `start` toward `end` (screen
/// space). `size` is the barb length in pixels.
pub(crate) fn arrowhead(start: Pt, end: Pt, size: f32) -> Option<Path<Pixels>> {
    fill_polygon(&arrowhead_points(start, end, size)?)
}

/// Sample the perimeter of a (possibly rounded) rectangle in canvas space, with
/// rotation applied. `radius` is the corner radius in canvas units; `0` yields
/// the four sharp corners. `arc_segments` controls the smoothness of each
/// rounded corner.
pub(crate) fn rounded_rect_polyline(
    element: &WhiteboardElement,
    radius: f32,
    arc_segments: usize,
) -> Vec<Pt> {
    let center = element.center();
    let (sin, cos) = element.rotation.sin_cos();
    let x0 = element.x.min(element.x + element.width);
    let y0 = element.y.min(element.y + element.height);
    let w = element.width.abs();
    let h = element.height.abs();
    let x1 = x0 + w;
    let y1 = y0 + h;

    let max_radius = (w.min(h) * 0.5).max(0.0);
    let r = radius.clamp(0.0, max_radius);

    let mut local: Vec<Pt> = Vec::new();
    if r <= 0.5 {
        local.push((x0, y0));
        local.push((x1, y0));
        local.push((x1, y1));
        local.push((x0, y1));
    } else {
        let seg = arc_segments.max(2);
        // Each corner: center point and the quarter-arc start angle (radians).
        let corners = [
            ((x1 - r, y0 + r), -std::f32::consts::FRAC_PI_2), // top-right
            ((x1 - r, y1 - r), 0.0),                          // bottom-right
            ((x0 + r, y1 - r), std::f32::consts::FRAC_PI_2),  // bottom-left
            ((x0 + r, y0 + r), std::f32::consts::PI),         // top-left
        ];
        for (cpt, start_angle) in corners {
            for i in 0..=seg {
                let angle = start_angle + std::f32::consts::FRAC_PI_2 * (i as f32) / (seg as f32);
                local.push((cpt.0 + r * angle.cos(), cpt.1 + r * angle.sin()));
            }
        }
    }

    local
        .into_iter()
        .map(|p| rotate_around(p, center, sin, cos))
        .collect()
}

/// A tiny deterministic pseudo-random generator (xorshift) used to perturb
/// hand-drawn outlines. Seeded per element so the roughness is stable across
/// frames (no shimmering).
struct RoughRng {
    state: u64,
}

impl RoughRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed | 1, // never zero
        }
    }

    fn next_f32(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        // Map to [-1, 1].
        ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

/// Derive a stable numeric seed from an element id string.
pub(crate) fn element_seed(id: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in id.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Perturb a screen-space polyline to imitate a hand-drawn outline. Each edge
/// bows smoothly with a cubic-Bézier offset profile (zero at the true
/// vertices, random at the interior controls), like rough.js. Short edges
/// (dense curve samples) get a single gentle midpoint bow. `amplitude` is in
/// screen pixels; the same `seed` always produces the same result.
pub(crate) fn roughen_polyline(points: &[Pt], closed: bool, amplitude: f32, seed: u64) -> Vec<Pt> {
    if amplitude <= 0.0 || points.len() < 2 {
        return points.to_vec();
    }
    let mut rng = RoughRng::new(seed);
    let n = points.len();
    let count = if closed { n } else { n - 1 };
    let mut out: Vec<Pt> = Vec::with_capacity(count * 6 + 1);
    for i in 0..count {
        let a = points[i];
        let b = points[(i + 1) % n];
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let nx = -dy / len;
        let ny = dx / len;
        if len < 18.0 {
            // Dense sampling (curves): one gentle bow per edge.
            out.push(a);
            let offset = rng.next_f32() * amplitude * 0.5;
            out.push((
                a.0 + dx * 0.5 + nx * offset,
                a.1 + dy * 0.5 + ny * offset,
            ));
            continue;
        }
        // Random control offsets at 1/3 and 2/3; zero at the vertices.
        let o1 = rng.next_f32() * amplitude;
        let o2 = rng.next_f32() * amplitude;
        let steps = ((len / 12.0) as usize).clamp(3, 16);
        for step in 0..steps {
            let t = step as f32 / steps as f32;
            let mt = 1.0 - t;
            let offset = 3.0 * mt * mt * t * o1 + 3.0 * mt * t * t * o2;
            out.push((a.0 + dx * t + nx * offset, a.1 + dy * t + ny * offset));
        }
    }
    if !closed {
        out.push(points[n - 1]);
    }
    out
}

/// Stroke a (jittered) open polyline with smoothly varying width as filled
/// trapezoid vertex quads — the pen-pressure feel of the hand-drawn styles.
/// `variation` is the relative width wobble (0..1). Deterministic per `seed`.
pub(crate) fn sketch_stroke_polyline_quads(
    points: &[Pt],
    width: f32,
    variation: f32,
    seed: u64,
) -> Vec<[Pt; 4]> {
    let n = points.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    let mut rng = RoughRng::new(seed ^ 0x5bd1_e995);
    // Smooth per-vertex width factors: random targets every few vertices,
    // linearly interpolated in between.
    let span = 4usize;
    let mut prev_target = rng.next_f32();
    let mut next_target = rng.next_f32();
    let mut factors = Vec::with_capacity(n);
    for i in 0..n {
        if i > 0 && i % span == 0 {
            prev_target = next_target;
            next_target = rng.next_f32();
        }
        let phase = (i % span) as f32 / span as f32;
        let noise = prev_target + (next_target - prev_target) * phase;
        factors.push((1.0 + variation * noise).max(0.25));
    }
    let half = (width * 0.5).max(0.35);
    for i in 0..n - 1 {
        let a = points[i];
        let b = points[i + 1];
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-4 {
            continue;
        }
        let nx = -dy / len;
        let ny = dx / len;
        let ha = (half * factors[i]).max(0.3);
        let hb = (half * factors[i + 1]).max(0.3);
        out.push([
            (a.0 + nx * ha, a.1 + ny * ha),
            (b.0 + nx * hb, b.1 + ny * hb),
            (b.0 - nx * hb, b.1 - ny * hb),
            (a.0 - nx * ha, a.1 - ny * ha),
        ]);
    }
    out
}

/// Path wrapper over [`sketch_stroke_polyline_quads`].
pub(crate) fn sketch_stroke_polyline(
    points: &[Pt],
    width: f32,
    variation: f32,
    seed: u64,
) -> Vec<Path<Pixels>> {
    sketch_stroke_polyline_quads(points, width, variation, seed)
        .into_iter()
        .filter_map(|quad| fill_polygon(&quad))
        .collect()
}

/// Build a circular "rotate" glyph (an arc with an arrowhead) centered at
/// `center` in screen space, oriented by `orient` radians. Returns the stroke
/// segment paths plus the arrowhead fill path.
pub(crate) fn rotate_glyph(
    center: Pt,
    orient: f32,
    radius: f32,
    stroke_width: f32,
) -> (Vec<Path<Pixels>>, Option<Path<Pixels>>) {
    // A ~270° arc, leaving a gap, rotated by `orient`.
    let a0 = orient + 0.55;
    let a1 = orient + 0.55 + std::f32::consts::PI * 1.5;
    let segs = 20;
    let arc: Vec<Pt> = (0..=segs)
        .map(|i| {
            let a = a0 + (a1 - a0) * (i as f32) / (segs as f32);
            (center.0 + radius * a.cos(), center.1 + radius * a.sin())
        })
        .collect();
    let strokes = stroke_polyline(&arc, stroke_width, StrokeStyle::Solid, false);
    // Arrowhead at the arc's end, pointing along the tangent.
    let head = if arc.len() >= 2 {
        let prev = arc[arc.len() - 2];
        let end = arc[arc.len() - 1];
        arrowhead(prev, end, stroke_width * 2.6 + 3.0)
    } else {
        None
    };
    (strokes, head)
}

/// Clip the segment `p0`–`p1` to a convex polygon (Cyrus–Beck). Returns the
/// clipped segment, or `None` when fully outside.
fn clip_segment_convex(p0: Pt, p1: Pt, poly: &[Pt]) -> Option<(Pt, Pt)> {
    if poly.len() < 3 {
        return None;
    }
    // Signed area decides which side is the interior.
    let mut area = 0.0;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        area += a.0 * b.1 - b.0 * a.1;
    }
    let ccw = area > 0.0;

    let dx = p1.0 - p0.0;
    let dy = p1.1 - p0.1;
    let mut t0 = 0.0_f32;
    let mut t1 = 1.0_f32;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let ex = b.0 - a.0;
        let ey = b.1 - a.1;
        let (nx, ny) = if ccw { (-ey, ex) } else { (ey, -ex) };
        let d = nx * dx + ny * dy;
        let w = nx * (p0.0 - a.0) + ny * (p0.1 - a.1);
        if d.abs() < 1e-6 {
            if w < 0.0 {
                return None;
            }
            continue;
        }
        let t = -w / d;
        if d > 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some((
        (p0.0 + dx * t0, p0.1 + dy * t0),
        (p0.0 + dx * t1, p0.1 + dy * t1),
    ))
}

/// Generate diagonal hatch segments covering a convex polygon (screen space).
/// `spacing` is the distance between lines; `flip` mirrors the direction (for
/// the second pass of cross-hatching).
pub(crate) fn hatch_segments(poly: &[Pt], spacing: f32, flip: bool) -> Vec<(Pt, Pt)> {
    let mut out = Vec::new();
    if poly.len() < 3 {
        return out;
    }
    let inv = std::f32::consts::FRAC_1_SQRT_2;
    let dir = if flip { (inv, -inv) } else { (inv, inv) };
    let normal = (-dir.1, dir.0);

    let mut c_min = f32::MAX;
    let mut c_max = f32::MIN;
    let mut t_min = f32::MAX;
    let mut t_max = f32::MIN;
    for p in poly {
        let c = p.0 * normal.0 + p.1 * normal.1;
        let t = p.0 * dir.0 + p.1 * dir.1;
        c_min = c_min.min(c);
        c_max = c_max.max(c);
        t_min = t_min.min(t);
        t_max = t_max.max(t);
    }

    let spacing = spacing.max(2.0);
    let mut c = (c_min / spacing).ceil() * spacing;
    while c <= c_max {
        let p0 = (
            normal.0 * c + dir.0 * (t_min - 2.0),
            normal.1 * c + dir.1 * (t_min - 2.0),
        );
        let p1 = (
            normal.0 * c + dir.0 * (t_max + 2.0),
            normal.1 * c + dir.1 * (t_max + 2.0),
        );
        if let Some(segment) = clip_segment_convex(p0, p1, poly) {
            out.push(segment);
        }
        c += spacing;
    }
    out
}

/// Convenience: screen-space axis-aligned bounding box of a set of points.
pub(crate) fn bounds_of(points: &[Pt]) -> Option<(Pt, Pt)> {
    let first = *points.first()?;
    let mut min = first;
    let mut max = first;
    for p in points {
        min.0 = min.0.min(p.0);
        min.1 = min.1.min(p.1);
        max.0 = max.0.max(p.0);
        max.1 = max.1.max(p.1);
    }
    Some((min, max))
}

pub(crate) fn bounds_intersect(a_min: Pt, a_max: Pt, b_min: Pt, b_max: Pt) -> bool {
    a_min.0 <= b_max.0 && a_max.0 >= b_min.0 && a_min.1 <= b_max.1 && a_max.1 >= b_min.1
}

pub(crate) fn element_bounds(element: &WhiteboardElement) -> Option<(Pt, Pt)> {
    if element.kind.is_linear() {
        bounds_of(&route_points(element))
    } else {
        bounds_of(&element_corners(element))
    }
}

pub(crate) fn element_intersects_rect(
    element: &WhiteboardElement,
    min: Pt,
    max: Pt,
    pad: f32,
) -> bool {
    let Some((emin, emax)) = element_bounds(element) else {
        return false;
    };
    let emin = (emin.0 - pad, emin.1 - pad);
    let emax = (emax.0 + pad, emax.1 + pad);
    bounds_intersect(emin, emax, min, max)
}

pub(crate) fn point_near_element_bounds(element: &WhiteboardElement, p: Pt, pad: f32) -> bool {
    let Some((min, max)) = element_bounds(element) else {
        return false;
    };
    p.0 >= min.0 - pad && p.0 <= max.0 + pad && p.1 >= min.1 - pad && p.1 <= max.1 + pad
}

// ── Hit testing (canvas space) ───────────────────────────────────────────────

/// Even-odd ray-cast point-in-polygon test.
pub(crate) fn point_in_polygon(p: Pt, poly: &[Pt]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        let intersects = ((yi > p.1) != (yj > p.1))
            && (p.0 < (xj - xi) * (p.1 - yi) / (yj - yi + f32::EPSILON) + xi);
        if intersects {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Shortest distance from point `p` to segment `a`–`b`.
pub(crate) fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-6 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj = (a.0 + t * dx, a.1 + t * dy);
    (p.0 - proj.0).hypot(p.1 - proj.1)
}

/// Whether a canvas-space point hits the element (with `tol` canvas-unit
/// tolerance for thin shapes / stroke width).
pub(crate) fn hit_test(element: &WhiteboardElement, p: Pt, tol: f32) -> bool {
    if !point_near_element_bounds(element, p, tol + element.stroke.width.px() * 0.5 + 4.0) {
        return false;
    }

    match element.kind {
        ElementKind::Rectangle | ElementKind::Text => {
            point_in_polygon(p, &element_corners(element))
        }
        ElementKind::Ellipse => {
            let center = element.center();
            // Un-rotate the point into the ellipse's local frame.
            let (sin, cos) = (-element.rotation).sin_cos();
            let local = rotate_around(p, center, sin, cos);
            let rx = (element.width.abs() * 0.5).max(1.0) + tol;
            let ry = (element.height.abs() * 0.5).max(1.0) + tol;
            let nx = (local.0 - center.0) / rx;
            let ny = (local.1 - center.1) / ry;
            nx * nx + ny * ny <= 1.0
        }
        ElementKind::Line | ElementKind::Arrow => {
            // Test against the routed polyline so orthogonal/curved lines are
            // clickable along their actual path, not just the chord.
            let pts = route_points(element);
            let reach = tol + element.stroke.width.px() * 0.5 + 2.0;
            pts.windows(2)
                .any(|pair| dist_to_segment(p, pair[0], pair[1]) <= reach)
        }
    }
}

/// Combined axis-aligned bounding box of several elements (canvas space).
pub(crate) fn selection_bbox<'a>(
    elements: impl Iterator<Item = &'a WhiteboardElement>,
) -> Option<(Pt, Pt)> {
    let mut pts: Vec<Pt> = Vec::new();
    for element in elements {
        pts.extend(element_corners(element));
    }
    bounds_of(&pts)
}

/// The 8 handle positions of an axis-aligned box, in `ResizeHandle::ALL`
/// order (corners TL/TR/BR/BL then edges T/R/B/L). Space-agnostic.
pub(crate) fn bbox_handle_points(min: Pt, max: Pt) -> [Pt; 8] {
    let mid_x = (min.0 + max.0) * 0.5;
    let mid_y = (min.1 + max.1) * 0.5;
    [
        (min.0, min.1),
        (max.0, min.1),
        (max.0, max.1),
        (min.0, max.1),
        (mid_x, min.1),
        (max.0, mid_y),
        (mid_x, max.1),
        (min.0, mid_y),
    ]
}

/// Canvas-space anchor (fixed opposite point) for a bbox resize handle.
pub(crate) fn bbox_handle_anchor(min: Pt, max: Pt, index: usize) -> Pt {
    let pts = bbox_handle_points(min, max);
    // Corners oppose corners; edges oppose edges: TL<->BR, TR<->BL, T<->B, R<->L.
    let opposite = [2, 3, 0, 1, 6, 7, 4, 5];
    pts[opposite[index.min(7)]]
}

/// Canvas-space center where an element's text label is drawn: the routed
/// midpoint for linear elements, the box center otherwise.
pub(crate) fn label_center(element: &WhiteboardElement) -> Pt {
    if element.kind.is_linear() {
        let pts = route_points(element);
        if pts.len() >= 3 {
            pts[pts.len() / 2]
        } else if pts.len() == 2 {
            ((pts[0].0 + pts[1].0) * 0.5, (pts[0].1 + pts[1].1) * 0.5)
        } else {
            element.center()
        }
    } else {
        element.center()
    }
}

/// Approximate canvas-space hit test for a linear element's text label (drawn
/// centered on the line). Width is estimated from character widths since the
/// exact shaped width isn't available outside the renderer.
pub(crate) fn label_hit_test(element: &WhiteboardElement, p: Pt) -> bool {
    if !element.kind.is_linear() || element.text.is_empty() {
        return false;
    }
    let font_size = element.font.size.px();
    let line_height = font_size * 1.3;
    let mut max_width = 0.0_f32;
    let mut line_count = 0usize;
    for line in element.text.split('\n') {
        line_count += 1;
        let width: f32 = line
            .chars()
            .map(|ch| {
                if ch.is_ascii() {
                    font_size * 0.55
                } else {
                    font_size
                }
            })
            .sum();
        max_width = max_width.max(width);
    }
    let total_height = line_height * line_count.max(1) as f32;
    let (cx_, cy_) = label_center(element);
    let half_height = total_height * 0.5 + 4.0;
    let half_width = (max_width * 0.5 + 6.0).max(20.0);
    p.0 >= cx_ - half_width
        && p.0 <= cx_ + half_width
        && p.1 >= cy_ - half_height
        && p.1 <= cy_ + half_height
}

/// Split an open polyline into the pieces lying outside an axis-aligned rect
/// (used to leave a gap in a line where its label is drawn). Segments are
/// clipped against the rect with the slab method.
pub(crate) fn split_polyline_outside_rect(points: &[Pt], min: Pt, max: Pt) -> Vec<Vec<Pt>> {
    let mut runs: Vec<Vec<Pt>> = Vec::new();
    let mut current: Vec<Pt> = Vec::new();
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        // Parametric interval [t0, t1] of the segment inside the rect.
        let mut t0 = 0.0_f32;
        let mut t1 = 1.0_f32;
        let mut outside = false;
        for (p, d, lo, hi) in [(a.0, dx, min.0, max.0), (a.1, dy, min.1, max.1)] {
            if d.abs() < 1e-6 {
                if p < lo || p > hi {
                    outside = true;
                    break;
                }
            } else {
                let mut ta = (lo - p) / d;
                let mut tb = (hi - p) / d;
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
                if t0 >= t1 {
                    outside = true;
                    break;
                }
            }
        }
        if outside || t1 <= 0.0 || t0 >= 1.0 {
            // Fully outside: extend the current run.
            if current.is_empty() {
                current.push(a);
            }
            current.push(b);
            continue;
        }
        let t0c = t0.max(0.0);
        let t1c = t1.min(1.0);
        // Portion before entering the rect.
        if t0c > 1e-4 {
            if current.is_empty() {
                current.push(a);
            }
            current.push((a.0 + dx * t0c, a.1 + dy * t0c));
        }
        if current.len() >= 2 {
            runs.push(std::mem::take(&mut current));
        } else {
            current.clear();
        }
        // Portion after exiting the rect starts a new run.
        if t1c < 1.0 - 1e-4 {
            current.push((a.0 + dx * t1c, a.1 + dy * t1c));
            current.push(b);
        }
    }
    if current.len() >= 2 {
        runs.push(current);
    }
    runs
}

/// Whether the element's bounding box intersects the given canvas-space rect
/// (used for marquee selection). Uses the element's axis-aligned bounds.
pub(crate) fn intersects_rect(element: &WhiteboardElement, min: Pt, max: Pt) -> bool {
    element_intersects_rect(element, min, max, 0.0)
}

/// Screen positions of the 8 resize handles for a non-linear element, ordered
/// to match `ResizeHandle`'s corner-then-edge layout.
pub(crate) fn resize_handle_points(element: &WhiteboardElement, view: &CanvasView) -> [Pt; 8] {
    let pad = selection_outline_pad(element);
    let c = outline_corners(element, pad).map(|corner| view.to_screen(corner));
    let mid = |a: Pt, b: Pt| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    [
        c[0],
        c[1],
        c[2],
        c[3],
        mid(c[0], c[1]),
        mid(c[1], c[2]),
        mid(c[2], c[3]),
        mid(c[3], c[0]),
    ]
}
