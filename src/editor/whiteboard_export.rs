//! Export whiteboard elements as SVG, PNG, or JSON, replicating the canvas
//! renderer's geometry pipeline (roughness, dash patterns, fills, labels) so
//! exports look identical to the on-screen drawing. PNG is produced by
//! rasterizing the generated SVG with `resvg` (system fonts for text).

use gpui::{ClipboardItem, Hsla, Image, ImageFormat};

use super::EditorView;
use super::whiteboard::{
    CanvasTextAlign, ElementKind, FillStyle, StrokeStyle, WhiteboardDocument, WhiteboardElement,
};
use super::whiteboard_geometry::{
    CanvasView, Pt, arrowhead_points, bounds_of, element_corners, element_seed, ellipse_perimeter,
    ellipse_segment_count, hatch_segments, label_center, roughen_polyline, rounded_rect_polyline,
    route_points, sketch_stroke_polyline_quads, split_polyline_outside_rect, stroke_polyline_quads,
};
use crate::theme::{Theme, with_alpha};

const EXPORT_PADDING: f32 = 24.0;
const PNG_SCALE: f32 = 2.0;

/// Convert an `Hsla` color to (`#rrggbb`, alpha).
fn hsla_to_svg(color: Hsla) -> (String, f32) {
    let h = (color.h.fract() + 1.0).fract() * 6.0;
    let c = (1.0 - (2.0 * color.l - 1.0).abs()) * color.s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = color.l - c * 0.5;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to8 = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    (
        format!("#{:02x}{:02x}{:02x}", to8(r), to8(g), to8(b)),
        color.a,
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn push_polygon(out: &mut String, points: &[Pt], color: Hsla) {
    if points.len() < 3 {
        return;
    }
    let (hex, alpha) = hsla_to_svg(color);
    out.push_str("<polygon points=\"");
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{:.2},{:.2}", p.0, p.1));
    }
    out.push_str(&format!("\" fill=\"{hex}\""));
    if alpha < 0.999 {
        out.push_str(&format!(" fill-opacity=\"{alpha:.3}\""));
    }
    out.push_str("/>\n");
}

/// Estimated text-block metrics for an element's label/text (the renderer's
/// shaping isn't available outside a window; this mirrors `label_hit_test`).
fn estimate_block(text: &str, font_size: f32) -> (f32, usize) {
    let mut max_width = 0.0_f32;
    let mut line_count = 0usize;
    for line in text.split('\n') {
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
    (max_width, line_count.max(1))
}

/// Emit `<text>` lines for an element's label (centered on `center`) or
/// standalone text (anchored at the element box).
#[allow(clippy::too_many_arguments)]
fn push_text(
    out: &mut String,
    element: &WhiteboardElement,
    view: &CanvasView,
    color: Hsla,
    font_family: &str,
) {
    if element.text.is_empty() {
        return;
    }
    let font_size = element.font.size.px();
    let line_height = font_size * 1.3;
    let (hex, alpha) = hsla_to_svg(color);
    let family = xml_escape(font_family);
    let (block_w, line_count) = estimate_block(&element.text, font_size);

    let (anchor, x, top) = if element.kind == ElementKind::Text {
        let (x0, y0) = view.to_screen((element.x, element.y));
        let w = element.width.abs().max(block_w);
        match element.font.align {
            CanvasTextAlign::Left => ("start", x0, y0),
            CanvasTextAlign::Center => ("middle", x0 + w * 0.5, y0),
            CanvasTextAlign::Right => ("end", x0 + w, y0),
        }
    } else {
        let (cx_, cy_) = view.to_screen(label_center(element));
        let top = cy_ - line_height * line_count as f32 * 0.5;
        match element.font.align {
            CanvasTextAlign::Left => ("start", cx_ - block_w * 0.5, top),
            CanvasTextAlign::Center => ("middle", cx_, top),
            CanvasTextAlign::Right => ("end", cx_ + block_w * 0.5, top),
        }
    };

    for (index, line) in element.text.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        // Baseline ≈ 80% into the line box.
        let y = top + line_height * index as f32 + font_size * 0.92;
        out.push_str(&format!(
            "<text x=\"{x:.2}\" y=\"{y:.2}\" font-family=\"{family}\" font-size=\"{font_size:.1}\" text-anchor=\"{anchor}\" fill=\"{hex}\"",
        ));
        if alpha < 0.999 {
            out.push_str(&format!(" fill-opacity=\"{alpha:.3}\""));
        }
        out.push_str(&format!(">{}</text>\n", xml_escape(line)));
    }
}

/// Estimated screen-space label rect for a linear element (for the line gap).
fn estimated_label_rect(element: &WhiteboardElement, view: &CanvasView) -> Option<(Pt, Pt)> {
    if element.text.is_empty() || !element.kind.is_linear() {
        return None;
    }
    let font_size = element.font.size.px();
    let line_height = font_size * 1.3;
    let (block_w, line_count) = estimate_block(&element.text, font_size);
    let (cx_, cy_) = view.to_screen(label_center(element));
    let half_h = line_height * line_count as f32 * 0.5;
    Some((
        (cx_ - block_w * 0.5, cy_ - half_h),
        (cx_ + block_w * 0.5, cy_ + half_h),
    ))
}

/// Replicate the renderer's stroke pipeline for one polyline, emitting SVG
/// polygons (same quads the canvas paints).
#[allow(clippy::too_many_arguments)]
fn push_stroke(
    out: &mut String,
    element: &WhiteboardElement,
    points: &[Pt],
    closed: bool,
    stroke_color: Hsla,
    stroke_width: f32,
) {
    let roughness = element.stroke.roughness;
    let amplitude = roughness.amplitude();
    let seed = element_seed(&element.id);
    if amplitude <= 0.0 {
        for quad in stroke_polyline_quads(points, stroke_width, element.stroke.style, closed) {
            push_polygon(out, &quad, stroke_color);
        }
        return;
    }
    for pass in 0..roughness.passes() {
        let pass_seed = seed.wrapping_add(pass as u64 * 0x9e37);
        let pass_amplitude = if pass == 0 {
            amplitude
        } else {
            amplitude * 1.4
        };
        let color = if pass == 0 {
            stroke_color
        } else {
            with_alpha(stroke_color, stroke_color.a * roughness.overdraw_alpha())
        };
        let mut pts = roughen_polyline(points, closed, pass_amplitude, pass_seed);
        if element.stroke.style == StrokeStyle::Solid {
            if closed && let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            for quad in sketch_stroke_polyline_quads(
                &pts,
                stroke_width,
                roughness.width_variation(),
                pass_seed,
            ) {
                push_polygon(out, &quad, color);
            }
        } else {
            for quad in stroke_polyline_quads(&pts, stroke_width, element.stroke.style, closed) {
                push_polygon(out, &quad, color);
            }
        }
    }
}

/// Emit the fill (solid or hatched) for a convex screen polygon.
fn push_fill(out: &mut String, screen: &[Pt], fill: FillStyle, color: Hsla) {
    match fill {
        FillStyle::Solid => push_polygon(out, screen, color),
        FillStyle::Hatch | FillStyle::CrossHatch => {
            let spacing = 9.0_f32;
            let width = 1.2_f32;
            let mut lines = |flip: bool| {
                for (a, b) in hatch_segments(screen, spacing, flip) {
                    for quad in stroke_polyline_quads(&[a, b], width, StrokeStyle::Solid, false) {
                        push_polygon(out, &quad, color);
                    }
                }
            };
            lines(true);
            if fill == FillStyle::CrossHatch {
                lines(false);
            }
        }
    }
}

/// Generate a standalone SVG document for the given elements (canvas units at
/// 1:1 scale, content padded). Returns `None` when there is nothing to draw.
pub(crate) fn whiteboard_svg(
    elements: &[WhiteboardElement],
    theme: &Theme,
    font_family: &str,
    background: Option<Hsla>,
) -> Option<String> {
    // Content bounds across element geometry (including routed lines).
    let mut pts: Vec<Pt> = Vec::new();
    for element in elements {
        let pad = element.stroke.width.px() * 0.5 + element.stroke.roughness.amplitude() + 1.0;
        let raw: Vec<Pt> = if element.kind.is_linear() {
            route_points(element)
        } else {
            element_corners(element).to_vec()
        };
        for p in raw {
            pts.push((p.0 - pad, p.1 - pad));
            pts.push((p.0 + pad, p.1 + pad));
        }
    }
    let (min, max) = bounds_of(&pts)?;
    let width = (max.0 - min.0) + EXPORT_PADDING * 2.0;
    let height = (max.1 - min.1) + EXPORT_PADDING * 2.0;
    let view = CanvasView {
        origin_x: EXPORT_PADDING - min.0,
        origin_y: EXPORT_PADDING - min.1,
        scroll_x: 0.0,
        scroll_y: 0.0,
        zoom: 1.0,
    };

    let mut out = String::with_capacity(4096);
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n",
    ));
    if let Some(bg) = background {
        let (hex, alpha) = hsla_to_svg(bg);
        out.push_str(&format!(
            "<rect width=\"100%\" height=\"100%\" fill=\"{hex}\" fill-opacity=\"{alpha:.3}\"/>\n",
        ));
    }

    for element in elements {
        let stroke_visible = element.stroke.color.resolve(theme).is_some();
        let stroke_color = element.stroke.color.resolve_stroke(theme);
        let stroke_width = element.stroke.width.px().max(0.75);
        let fill_color = element
            .background
            .color
            .resolve(theme)
            .map(|color| with_alpha(color, element.background.opacity.clamp(0.0, 1.0)));
        let text_color = with_alpha(
            element
                .font
                .effective_color(element.stroke.color)
                .resolve_stroke(theme),
            1.0,
        );
        let family = element.font.family.as_deref().unwrap_or(font_family);

        match element.kind {
            ElementKind::Rectangle => {
                let canvas_pts = rounded_rect_polyline(element, element.corner_radius.px(), 6);
                let screen: Vec<Pt> = canvas_pts.iter().map(|p| view.to_screen(*p)).collect();
                if let Some(color) = fill_color {
                    push_fill(&mut out, &screen, element.background.fill, color);
                }
                if stroke_visible {
                    push_stroke(&mut out, element, &screen, true, stroke_color, stroke_width);
                }
                push_text(&mut out, element, &view, text_color, family);
            }
            ElementKind::Ellipse => {
                let corners = element_corners(element).map(|p| view.to_screen(p));
                let w = (corners[1].0 - corners[0].0).hypot(corners[1].1 - corners[0].1);
                let h = (corners[3].0 - corners[0].0).hypot(corners[3].1 - corners[0].1);
                let segments = ellipse_segment_count(w, h);
                let screen: Vec<Pt> = ellipse_perimeter(element, segments)
                    .iter()
                    .map(|p| view.to_screen(*p))
                    .collect();
                if let Some(color) = fill_color {
                    push_fill(&mut out, &screen, element.background.fill, color);
                }
                if stroke_visible {
                    push_stroke(&mut out, element, &screen, true, stroke_color, stroke_width);
                }
                push_text(&mut out, element, &view, text_color, family);
            }
            ElementKind::Line | ElementKind::Arrow => {
                let screen: Vec<Pt> = route_points(element)
                    .iter()
                    .map(|p| view.to_screen(*p))
                    .collect();
                if stroke_visible {
                    if let Some((rect_min, rect_max)) = estimated_label_rect(element, &view) {
                        let pad = 7.0;
                        let pmin = (rect_min.0 - pad, rect_min.1 - pad);
                        let pmax = (rect_max.0 + pad, rect_max.1 + pad);
                        for piece in split_polyline_outside_rect(&screen, pmin, pmax) {
                            push_stroke(
                                &mut out,
                                element,
                                &piece,
                                false,
                                stroke_color,
                                stroke_width,
                            );
                        }
                    } else {
                        push_stroke(
                            &mut out,
                            element,
                            &screen,
                            false,
                            stroke_color,
                            stroke_width,
                        );
                    }
                    if element.kind == ElementKind::Arrow && screen.len() >= 2 {
                        let head = (stroke_width * 3.2 + 6.0).min(46.0);
                        let prev = screen[screen.len() - 2];
                        let end = screen[screen.len() - 1];
                        if let Some(tri) = arrowhead_points(prev, end, head) {
                            push_polygon(&mut out, &tri, stroke_color);
                        }
                    }
                }
                push_text(&mut out, element, &view, text_color, family);
            }
            ElementKind::Text => {
                push_text(&mut out, element, &view, text_color, family);
            }
        }
    }
    out.push_str("</svg>\n");
    Some(out)
}

/// Rasterize an SVG document to PNG bytes via resvg (loads system fonts so
/// `<text>` renders with the same families as the canvas).
pub(crate) fn svg_to_png(svg: &str) -> Option<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options).ok()?;
    let size = tree.size();
    let width = (size.width() * PNG_SCALE).ceil() as u32;
    let height = (size.height() * PNG_SCALE).ceil() as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width.max(1), height.max(1))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(PNG_SCALE, PNG_SCALE),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}

impl EditorView {
    /// Elements to export: the current selection (document z-order), or every
    /// element when nothing is selected.
    fn whiteboard_export_elements(&self) -> Vec<WhiteboardElement> {
        let Some(state) = self.active_whiteboard() else {
            return Vec::new();
        };
        if state.selected_ids.is_empty() {
            state.document.elements.clone()
        } else {
            state
                .document
                .elements
                .iter()
                .filter(|element| state.selected_ids.iter().any(|id| id == &element.id))
                .cloned()
                .collect()
        }
    }

    /// The canvas background tint, only when exporting the whole canvas.
    fn whiteboard_export_background(&self) -> Option<Hsla> {
        let state = self.active_whiteboard()?;
        if !state.selected_ids.is_empty() {
            return None;
        }
        let app_state = &state.document.app_state;
        app_state
            .background
            .resolve(&self.theme)
            .map(|tint| with_alpha(tint, app_state.background_opacity.clamp(0.0, 1.0)))
    }

    fn whiteboard_export_svg_string(&self) -> Option<String> {
        let elements = self.whiteboard_export_elements();
        let family = self.styles.editor.editor_body_latin_font_family;
        let cjk = self.styles.editor.editor_body_cjk_font_family;
        let family = format!("{family}, {cjk}");
        whiteboard_svg(
            &elements,
            &self.theme,
            &family,
            self.whiteboard_export_background(),
        )
    }

    fn whiteboard_export_file_stem(&self) -> String {
        self.active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| tab.path.as_ref())
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "whiteboard".to_string())
    }

    fn export_whiteboard_bytes(
        &mut self,
        name: String,
        bytes: Vec<u8>,
        cx: &mut gpui::Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let path = match this.update(cx, |_, cx| {
                cx.prompt_for_new_path(&std::env::home_dir().unwrap_or_default(), Some(&name))
            }) {
                Ok(receiver) => receiver.await.ok().and_then(Result::ok).flatten(),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
            let path = cx
                .background_executor()
                .spawn(async move {
                    crate::platform::dialog::run(move || {
                        rfd::FileDialog::new().set_file_name(name).save_file()
                    })
                })
                .await;
            let Some(path) = path else { return };
            let result =
                crate::platform::fs_task::run("Export whiteboard", path.clone(), move || {
                    crate::platform::fs_task::check_deadline()?;
                    std::fs::write(path, bytes)
                })
                .await;
            if let Err(error) = result {
                this.update(cx, |editor, cx| {
                    editor.notify_error(
                        format!("{}: {error}", editor.t(crate::i18n::TextKey::ExportFailed)),
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn whiteboard_export_svg(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(svg) = self.whiteboard_export_svg_string() else {
            return;
        };
        let name = format!("{}.svg", self.whiteboard_export_file_stem());
        self.export_whiteboard_bytes(name, svg.into_bytes(), cx);
    }

    pub(crate) fn whiteboard_export_png(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(png) = self
            .whiteboard_export_svg_string()
            .and_then(|svg| svg_to_png(&svg))
        else {
            return;
        };
        let name = format!("{}.png", self.whiteboard_export_file_stem());
        self.export_whiteboard_bytes(name, png, cx);
    }

    pub(crate) fn whiteboard_export_json(&mut self, cx: &mut gpui::Context<Self>) {
        let elements = self.whiteboard_export_elements();
        if elements.is_empty() {
            return;
        }
        let document = WhiteboardDocument {
            elements,
            ..Default::default()
        };
        let name = format!("{}.json", self.whiteboard_export_file_stem());
        self.export_whiteboard_bytes(name, document.to_json_string().into_bytes(), cx);
    }

    pub(crate) fn whiteboard_copy_as_svg(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(svg) = self.whiteboard_export_svg_string() {
            cx.write_to_clipboard(ClipboardItem::new_string(svg));
        }
    }

    pub(crate) fn whiteboard_copy_as_png(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(png) = self
            .whiteboard_export_svg_string()
            .and_then(|svg| svg_to_png(&svg))
        {
            cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                ImageFormat::Png,
                png,
            )));
        }
    }
}
