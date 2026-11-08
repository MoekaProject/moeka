//! Shared self-drawn control paint and measured geometry. Callers own events,
//! document coordinates and animation tasks; this module owns visual structure.
use crate::style::{ScrollbarStyles, SelfDrawnStyles};
use gpui::{
    App, Bounds, Corners, CursorStyle, Div, Font, Hsla, PaintQuad, Path, Pixels, Point, Size,
    TextAlign, TextRun, TransformationMatrix, Window, WrappedLine, div, point, prelude::*, px,
    size,
};

pub(crate) struct PaintedButton {
    pub(crate) quad: PaintQuad,
    pub(crate) label: Option<WrappedLine>,
    pub(crate) origin: Point<Pixels>,
    pub(crate) line_height: Pixels,
    pub(crate) icon: Option<(Bounds<Pixels>, &'static str, Hsla)>,
    pub(crate) trailing_labels: Vec<(WrappedLine, Point<Pixels>)>,
}

pub(crate) fn shape_preview_button(
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
    let (button_width, button_height) = PaintedButton::measure_label(
        f32::from(label.width()),
        button_padding_left_x,
        button_padding_right_x,
        button_padding_y,
        button_text_line_height,
    );
    Some((label, button_width, button_height))
}

pub(crate) fn paint_code_block_button(button: &PaintedButton, window: &mut Window, cx: &mut App) {
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

pub(crate) fn table_border_track_path(
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

/// One resolved scrollbar controls both the drawn thumb and its hit track.
#[derive(Clone, Copy)]
pub(crate) struct ScrollbarControl {
    pub style: ScrollbarStyles,
    pub scale: f32,
}
impl ScrollbarControl {
    pub fn new(style: ScrollbarStyles, scale: f32) -> Self {
        Self { style, scale }
    }
    pub fn hit_width(self) -> f32 {
        self.style
            .hit_width_px
            .max(self.style.width_px)
            .max(self.style.hover_width_px)
            * self.scale
    }
    pub fn width(self, active: bool) -> f32 {
        if active {
            self.style.hover_width_px
        } else {
            self.style.width_px
        }
    }
    pub fn opacity(self, visible: bool, active: bool, revealed: bool) -> f32 {
        if !visible {
            0.0
        } else if active {
            self.style.hover_opacity
        } else if revealed {
            self.style.opacity
        } else {
            self.style.idle_opacity
        }
    }
    pub fn color(self, sidebar: bool, opacity: f32) -> Hsla {
        let mut color = if sidebar {
            self.style.sidebar_thumb_color
        } else {
            self.style.thumb_color
        };
        color.a *= opacity.clamp(0.0, 1.0);
        color
    }
    pub fn thumb(self, width: f32, height: f32, color: Hsla) -> Div {
        div()
            .absolute()
            .w(px(width))
            .h(px(height))
            .rounded(px(self.style.radius_px * self.scale))
            .bg(color)
            .cursor(CursorStyle::Arrow)
    }
    /// Returns (offset, length, travel); viewport and content ranges are supplied
    /// by the scroll owner (including virtual and asynchronously reflowed content).
    pub fn thumb_geometry(
        self,
        track: f32,
        visible: f32,
        content: f32,
        offset: f32,
        max_scroll: f32,
        contain: bool,
    ) -> (f32, f32, f32) {
        let minimum = self.style.min_thumb_height_px * self.scale;
        let length = if contain {
            (track * (visible / content.max(1.0))).clamp(minimum.min(track), track)
        } else {
            (track * (visible / content.max(1.0))).clamp(minimum, track.max(minimum))
        };
        let travel = (track - length).max(0.0);
        let position = if max_scroll > 0.0 {
            (offset / max_scroll).clamp(0.0, 1.0) * travel
        } else {
            0.0
        };
        (position, length, travel)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ResizeHandleControl {
    pub thickness: f32,
    pub hit_width: f32,
    pub color: Hsla,
    pub active_color: Hsla,
}
impl ResizeHandleControl {
    pub fn new(styles: &SelfDrawnStyles, scale: f32, device_scale: f32, active: bool) -> Self {
        let device_scale = device_scale.max(1.0);
        Self {
            thickness: ((styles.pane_divider_width_px * scale * device_scale)
                .round()
                .max(1.0))
                / device_scale,
            hit_width: (styles.pane_divider_hit_width_px * scale)
                .max(styles.pane_divider_min_hit_width_px),
            color: if active {
                styles.colors.pane_divider_active
            } else {
                styles.colors.pane_divider
            },
            active_color: styles.colors.pane_divider_active,
        }
    }
    pub fn anchor(self, vertical: bool) -> Div {
        let element = div().relative().flex_none().bg(self.color);
        if vertical {
            element.w(px(self.thickness)).h_full()
        } else {
            element.h(px(self.thickness)).w_full()
        }
    }
    pub fn hit(self, vertical: bool) -> Div {
        let element = div().absolute();
        if vertical {
            element
                .top_0()
                .left(px(-(self.hit_width - self.thickness) / 2.0))
                .w(px(self.hit_width))
                .h_full()
                .cursor(CursorStyle::ResizeLeftRight)
        } else {
            element
                .left_0()
                .top(px(-(self.hit_width - self.thickness) / 2.0))
                .h(px(self.hit_width))
                .w_full()
                .cursor(CursorStyle::ResizeUpDown)
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ImagePreviewChrome {
    pub padding: f32,
    pub chart_fraction: f32,
    pub click_slop: f32,
}
impl ImagePreviewChrome {
    pub fn new(style: &SelfDrawnStyles) -> Self {
        // Preview viewport/pan coordinates are physical layout pixels; preserve
        // the existing fixed screen inset independently of document zoom.
        Self {
            padding: style.preview_overlay_padding_px,
            chart_fraction: style.preview_chart_initial_fraction,
            click_slop: style.preview_click_slop_px,
        }
    }
    pub fn inner_size(self, width: f32, height: f32) -> (f32, f32) {
        (
            (width - self.padding * 2.0).max(1.0),
            (height - self.padding * 2.0).max(1.0),
        )
    }
    pub fn fit_scale(
        self,
        width: f32,
        height: f32,
        viewport_width: f32,
        viewport_height: f32,
        chart: bool,
    ) -> f32 {
        let (available_width, available_height) = self.inner_size(viewport_width, viewport_height);
        let fraction = if chart { self.chart_fraction } else { 1.0 };
        let fit = (available_width * fraction / width.max(1.0))
            .min(available_height * fraction / height.max(1.0));
        if chart { fit } else { fit.min(1.0) }
    }
    pub fn moved(self, start: Point<Pixels>, position: Point<Pixels>) -> bool {
        let dx = f32::from(position.x - start.x);
        let dy = f32::from(position.y - start.y);
        dx * dx + dy * dy > self.click_slop * self.click_slop
    }
}

pub(crate) struct DropIndicatorControl;
impl DropIndicatorControl {
    pub fn overlay(color: Hsla) -> Div {
        div().absolute().bg(color)
    }
    pub fn line(left: f32, top: Pixels, width: f32, thickness: f32, color: Hsla) -> Div {
        div()
            .absolute()
            .left(px(left))
            .top(top)
            .w(px(width))
            .h(px(thickness))
            .bg(color)
    }
    pub fn feedback(bounds: Bounds<Pixels>, mut color: Hsla, opacity: f32, window: &mut Window) {
        color.a = opacity.clamp(0.0, 1.0);
        window.paint_quad(gpui::fill(bounds, color));
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LineFormatToolbarGeometry {
    pub(crate) line: usize,
    pub(crate) viewport: Bounds<Pixels>,
    pub(crate) content_left: Pixels,
    pub(crate) trigger_bounds: Bounds<Pixels>,
}

impl LineFormatToolbarGeometry {
    pub(crate) fn toolbar_origin(
        &self,
        measured: Size<Pixels>,
        margin: Pixels,
        gap: Pixels,
    ) -> Option<Point<Pixels>> {
        if !positive_bounds(Bounds::new(point(px(0.0), px(0.0)), measured))
            || measured.width + margin * 2.0 > self.viewport.size.width
            || measured.height + margin * 2.0 > self.viewport.size.height
        {
            return None;
        }
        let left = self.viewport.left() + margin;
        let right = self.viewport.right() - margin - measured.width;
        let x = self.content_left.clamp(left, right);
        let above = self.trigger_bounds.top() - gap - measured.height;
        let below = self.trigger_bounds.bottom() + gap;
        let y = if above >= self.viewport.top() + margin {
            above
        } else if below + measured.height <= self.viewport.bottom() - margin {
            below
        } else {
            return None;
        };
        Some(point(x, y))
    }
}

fn positive_bounds(bounds: Bounds<Pixels>) -> bool {
    f32::from(bounds.origin.x).is_finite()
        && f32::from(bounds.origin.y).is_finite()
        && f32::from(bounds.size.width).is_finite()
        && f32::from(bounds.size.height).is_finite()
        && bounds.size.width > px(0.0)
        && bounds.size.height > px(0.0)
}

/// Measured UI column shared with front-matter shaping and uncached pointer/IME geometry.
pub(crate) struct FrontMatterControl;
impl FrontMatterControl {
    pub fn column_width(style: &SelfDrawnStyles, scale: f32) -> f32 {
        (style.format_trigger_size_px + style.format_trigger_content_gap_px) * scale
    }
    pub fn content_geometry(
        is_property: bool,
        left: f32,
        width: f32,
        column_width: f32,
    ) -> (f32, f32) {
        if is_property {
            (left + column_width, (width - column_width).max(1.0))
        } else {
            (left, width)
        }
    }
    pub fn glyph_bounds(slot: Bounds<Pixels>, size: Pixels) -> Bounds<Pixels> {
        Bounds::new(
            point(slot.left(), slot.top() + (slot.size.height - size) / 2.0),
            gpui::size(size, size),
        )
    }
}

pub(crate) struct TableHandleControl;
impl TableHandleControl {
    pub fn segment(left: f32, top: f32, extent: f32, width: f32, column: bool) -> Bounds<Pixels> {
        if column {
            Bounds::new(
                point(px(left), px(top - width)),
                size(px(extent), px(width)),
            )
        } else {
            Bounds::new(
                point(px(left - width), px(top)),
                size(px(width), px(extent)),
            )
        }
    }
    pub fn track_path(
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        width: f32,
        radius: f32,
    ) -> Option<Path<Pixels>> {
        table_border_track_path(left, top, right, bottom, width, radius)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PaintedButtonRole {
    Copy,
    PreviewAction,
}
#[derive(Clone, Copy)]
pub(crate) struct PaintedButtonMetrics {
    pub font_size: f32,
    pub line_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub radius: Pixels,
    pub border_width: Pixels,
}
impl PaintedButtonMetrics {
    pub fn new(style: &SelfDrawnStyles, scale: f32, role: PaintedButtonRole) -> Self {
        let (font, x, y, radius, border) = match role {
            PaintedButtonRole::Copy => (
                style.code_block_copy_button_font_size_px,
                style.code_block_copy_button_padding_x_px,
                style.code_block_copy_button_padding_y_px,
                style.code_block_copy_button_radius_px,
                style.code_block_copy_button_border_width_px,
            ),
            PaintedButtonRole::PreviewAction => (
                style.code_block_copy_button_font_size_px,
                style.generated_preview_action_inner_padding_x_px,
                style.code_block_copy_button_padding_y_px,
                style.generated_preview_action_button_radius_px,
                style.generated_preview_action_border_width_px,
            ),
        };
        Self {
            font_size: font * scale,
            line_height: font * scale * style.preview_button_line_height_factor,
            padding_x: x * scale,
            padding_y: y * scale,
            radius: px(radius * scale),
            border_width: px(border * scale),
        }
    }
}
impl PaintedButton {
    pub fn measure_label(
        label_width: f32,
        left: f32,
        right: f32,
        vertical: f32,
        line_height: f32,
    ) -> (f32, f32) {
        (label_width + left + right, line_height + vertical * 2.0)
    }

    pub fn surface(
        bounds: Bounds<Pixels>,
        corners: Corners<Pixels>,
        background: Hsla,
        border_width: Pixels,
        border_color: Hsla,
    ) -> PaintQuad {
        gpui::quad(
            bounds,
            corners,
            background,
            border_width,
            border_color,
            gpui::BorderStyle::default(),
        )
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ImageEditControl {
    pub size: f32,
    pub gap: f32,
    pub opacity: f32,
}
impl ImageEditControl {
    pub fn new(style: &SelfDrawnStyles, scale: f32, hovered: bool) -> Self {
        Self {
            size: style.image_edit_icon_size_px * scale,
            gap: style.image_edit_icon_gap_px * scale,
            opacity: if hovered {
                style.image_edit_icon_hover_opacity
            } else {
                style.image_edit_icon_normal_opacity
            },
        }
    }
    pub fn bounds(self, syntax_right: f32, row_top: f32, row_height: f32) -> Bounds<Pixels> {
        Bounds::new(
            point(
                px(syntax_right + self.gap),
                px(row_top + (row_height - self.size) / 2.0),
            ),
            size(px(self.size), px(self.size)),
        )
    }
}

pub(crate) fn code_block_horizontal_thumb_geometry(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Styles, UiStyleSpec};
    use crate::theme::ThemeKind;

    fn resolve(spec: UiStyleSpec) -> Styles {
        let theme = ThemeKind::OneDark.build();
        Styles::from_theme_with_spec(&theme, &theme, false, spec)
    }
    #[test]
    fn self_drawn_scrollbar_defaults_preserve_native_track_and_palette() {
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let theme = kind.build();
            let styles = Styles::from_theme(&theme, &theme);
            let control = ScrollbarControl::new(styles.self_drawn.scrollbar, 1.0);
            assert_eq!(control.hit_width(), 8.0);
            assert_eq!(control.width(false), 6.0);
            assert_eq!(control.width(true), 6.0);
            assert_eq!(control.opacity(false, true, true), 0.0);
            assert_eq!(control.opacity(true, false, false), 0.0);
            assert_eq!(control.opacity(true, false, true), 0.2);
            assert_eq!(control.opacity(true, true, true), 0.3);
            assert_eq!(control.style.top_margin_px, 3.0);
            assert_eq!(control.style.bottom_margin_px, 3.0);
            assert_eq!(control.style.right_margin_px, 3.0);
            assert_eq!(control.style.hover_reveal_delay_ms, 100);
            assert_eq!(control.style.idle_delay_ms, 800);
            assert_eq!(
                control.thumb_geometry(200.0, 200.0, 500.0, 300.0, 300.0, true),
                (120.0, 80.0, 120.0)
            );
            assert_eq!(
                control.style.thumb_color,
                theme.with_light_surface_contrast().text_primary
            );
        }
    }
    #[test]
    fn self_drawn_scrollbar_nondefault_paint_hit_and_motion_share_metrics() {
        let mut spec = UiStyleSpec::default();
        let s = &mut spec.self_drawn.scrollbar;
        s.width_px = 9.0;
        s.hover_width_px = 13.0;
        s.hit_width_px = 10.0;
        s.radius_px = 4.0;
        s.min_thumb_height_px = 42.0;
        s.opacity = 0.4;
        s.hover_opacity = 0.8;
        s.idle_opacity = 0.1;
        s.animation_frame_ms = 11;
        s.fade_animation_ms = 123;
        s.width_animation_ms = 87;
        s.hover_reveal_delay_ms = 211;
        s.idle_delay_ms = 999;
        s.top_margin_px = 7.0;
        s.bottom_margin_px = 9.0;
        s.right_margin_px = 5.0;
        let styles = resolve(spec);
        let control = ScrollbarControl::new(styles.self_drawn.scrollbar, 1.5);
        assert_eq!(control.hit_width(), 19.5);
        assert_eq!(control.width(true), 13.0);
        let (offset, length, travel) =
            control.thumb_geometry(200.0, 200.0, 2000.0, 1800.0, 1800.0, true);
        assert_eq!((offset, length, travel), (137.0, 63.0, 137.0));
        let color = control.color(false, control.opacity(true, true, false));
        assert_eq!(color.a, 0.8);
        let mut thumb = control.thumb(control.width(true) * 1.5, length, color);
        assert_eq!(thumb.style().size.width, Some(px(19.5).into()));
        assert_eq!(thumb.style().size.height, Some(px(63.0).into()));
        assert_eq!(thumb.style().corner_radii.top_left, Some(px(6.0).into()));
        assert_eq!(thumb.style().background, Some(color.into()));
        assert_eq!(
            (
                control.style.animation_frame_ms,
                control.style.fade_animation_ms,
                control.style.width_animation_ms
            ),
            (11, 123, 87)
        );
        assert_eq!(
            (
                control.style.hover_reveal_delay_ms,
                control.style.idle_delay_ms
            ),
            (211, 999)
        );
        assert_eq!(
            (
                control.style.top_margin_px,
                control.style.bottom_margin_px,
                control.style.right_margin_px
            ),
            (7.0, 9.0, 5.0)
        );
        assert_eq!(control.opacity(true, false, false), 0.1);
        assert_eq!(control.opacity(true, false, true), 0.4);
    }
    #[test]
    fn self_drawn_button_measurement_and_surface_use_resolved_specs() {
        let mut spec = UiStyleSpec::default();
        spec.self_drawn.code_block_copy_button_font_size_px = 15.0;
        spec.self_drawn.code_block_copy_button_padding_x_px = 13.0;
        spec.self_drawn.code_block_copy_button_padding_y_px = 7.0;
        spec.self_drawn.code_block_copy_button_radius_px = 9.0;
        spec.self_drawn.code_block_copy_button_border_width_px = 2.0;
        spec.self_drawn.preview_button_line_height_factor = 1.5;
        spec.self_drawn.generated_preview_action_inner_padding_x_px = 8.0;
        spec.self_drawn.generated_preview_action_button_radius_px = 11.0;
        spec.self_drawn.generated_preview_action_border_width_px = 3.0;
        let styles = resolve(spec);
        for (role, font, x, y, radius, border) in [
            (PaintedButtonRole::Copy, 15.0, 13.0, 7.0, 9.0, 2.0),
            (PaintedButtonRole::PreviewAction, 15.0, 8.0, 7.0, 11.0, 3.0),
        ] {
            let metrics = PaintedButtonMetrics::new(&styles.self_drawn, 2.0, role);
            assert_eq!(
                (metrics.font_size, metrics.padding_x, metrics.padding_y),
                (font * 2.0, x * 2.0, y * 2.0)
            );
            let measured = PaintedButton::measure_label(
                60.0,
                metrics.padding_x,
                metrics.padding_x,
                metrics.padding_y,
                metrics.line_height,
            );
            assert_eq!(measured, (60.0 + x * 4.0, font * 3.0 + y * 4.0));
            let bounds = Bounds::new(
                point(px(20.0), px(30.0)),
                size(px(measured.0), px(measured.1)),
            );
            let color = styles.self_drawn.colors.code_block_copy_button_bg;
            let surface = PaintedButton::surface(
                bounds,
                Corners::all(metrics.radius),
                color,
                metrics.border_width,
                color,
            );
            assert_eq!(surface.bounds, bounds);
            assert_eq!(surface.corner_radii.top_left, px(radius * 2.0));
            assert_eq!(surface.border_widths.top, px(border * 2.0));
            assert!(bounds.contains(&point(bounds.right() - px(1.0), bounds.bottom() - px(1.0))));
        }
    }
    #[test]
    fn self_drawn_resize_hit_anchor_and_device_snapping_share_geometry() {
        let mut spec = UiStyleSpec::default();
        spec.self_drawn.pane_divider_width_px = 2.0;
        spec.self_drawn.pane_divider_hit_width_px = 12.0;
        let styles = resolve(spec);
        let control = ResizeHandleControl::new(&styles.self_drawn, 1.25, 2.0, true);
        assert_eq!(control.thickness, 2.5);
        assert_eq!(control.hit_width, 15.0);
        let mut anchor = control.anchor(true);
        let mut hit = control.hit(true);
        assert_eq!(anchor.style().size.width, Some(px(2.5).into()));
        assert_eq!(hit.style().size.width, Some(px(15.0).into()));
        assert_eq!(hit.style().inset.left, Some(px(-6.25).into()));
        assert_eq!(control.color, styles.self_drawn.colors.pane_divider_active);
    }
    #[test]
    fn self_drawn_preview_and_front_matter_share_nondefault_measurement_hit_geometry() {
        let mut spec = UiStyleSpec::default();
        spec.self_drawn.preview_overlay_padding_px = 50.0;
        spec.self_drawn.preview_chart_initial_fraction = 0.5;
        spec.self_drawn.preview_click_slop_px = 10.0;
        spec.self_drawn.format_trigger_size_px = 24.0;
        spec.self_drawn.format_trigger_content_gap_px = 10.0;
        let styles = resolve(spec);
        let chrome = ImagePreviewChrome::new(&styles.self_drawn);
        assert_eq!(chrome.inner_size(1000.0, 800.0), (900.0, 700.0));
        assert_eq!(chrome.fit_scale(900.0, 700.0, 1000.0, 800.0, true), 0.5);
        assert_eq!(chrome.fit_scale(900.0, 700.0, 1000.0, 800.0, false), 1.0);
        assert!(!chrome.moved(point(px(0.0), px(0.0)), point(px(8.0), px(0.0))));
        assert!(chrome.moved(point(px(0.0), px(0.0)), point(px(11.0), px(0.0))));
        let inset = FrontMatterControl::column_width(&styles.self_drawn, 1.5);
        assert_eq!(inset, 51.0);
        assert_eq!(
            FrontMatterControl::content_geometry(true, 100.0, 400.0, inset),
            (151.0, 349.0)
        );
        assert_eq!(
            FrontMatterControl::content_geometry(false, 100.0, 400.0, inset),
            (100.0, 400.0)
        );
        assert_eq!(
            FrontMatterControl::content_geometry(true, 100.0, 10.0, inset),
            (151.0, 1.0)
        );
    }
    #[test]
    fn self_drawn_table_segment_is_identical_for_paint_and_pointer_hit() {
        let mut spec = UiStyleSpec::default();
        spec.self_drawn.table_border_track_width_px = 9.0;
        let styles = resolve(spec);
        let width = styles.self_drawn.table_border_track_width_px * 1.5;
        for column in [false, true] {
            let bounds = TableHandleControl::segment(100.0, 200.0, 60.0, width, column);
            let quad = PaintedButton::surface(
                bounds,
                Corners::all(px(0.0)),
                styles.self_drawn.colors.table_handle_active,
                px(0.0),
                styles.self_drawn.colors.table_handle_active,
            );
            assert_eq!(quad.bounds, bounds);
            let hit = if column {
                point(px(120.0), px(190.0))
            } else {
                point(px(90.0), px(220.0))
            };
            assert!(bounds.contains(&hit));
        }
        assert!(TableHandleControl::track_path(100.0, 200.0, 500.0, 700.0, width, 4.0).is_some());
    }
}

#[cfg(test)]
mod auxiliary_tests {
    use super::*;
    #[test]
    fn self_drawn_image_action_uses_nondefault_size_gap_opacity_and_hit_bounds() {
        let theme = crate::theme::ThemeKind::OneDark.build();
        let mut spec = crate::style::UiStyleSpec::default();
        spec.self_drawn.image_edit_icon_size_px = 20.0;
        spec.self_drawn.image_edit_icon_gap_px = 7.0;
        spec.self_drawn.image_edit_icon_normal_opacity = 0.4;
        spec.self_drawn.image_edit_icon_hover_opacity = 0.8;
        let styles = crate::style::Styles::from_theme_with_spec(&theme, &theme, false, spec);
        for hovered in [false, true] {
            let control = ImageEditControl::new(&styles.self_drawn, 1.5, hovered);
            assert_eq!(control.opacity, if hovered { 0.8 } else { 0.4 });
            let bounds = control.bounds(200.0, 100.0, 40.0);
            assert_eq!(
                bounds,
                Bounds::new(point(px(210.5), px(105.0)), size(px(30.0), px(30.0)))
            );
            assert!(bounds.contains(&point(px(211.0), px(106.0))));
        }
    }
    #[test]
    fn self_drawn_toolbar_nondefault_margin_gap_constrain_both_visual_and_hit_surface() {
        let theme = crate::theme::ThemeKind::OneDark.build();
        let mut spec = crate::style::UiStyleSpec::default();
        spec.self_drawn.line_toolbar_viewport_margin_px = 12.0;
        spec.self_drawn.line_toolbar_trigger_gap_px = 8.0;
        let styles = crate::style::Styles::from_theme_with_spec(&theme, &theme, false, spec);
        let geometry = LineFormatToolbarGeometry {
            line: 1,
            viewport: Bounds::new(point(px(0.0), px(0.0)), size(px(300.0), px(300.0))),
            content_left: px(290.0),
            trigger_bounds: Bounds::new(point(px(200.0), px(150.0)), size(px(20.0), px(20.0))),
        };
        let origin = geometry
            .toolbar_origin(
                size(px(100.0), px(40.0)),
                px(styles.self_drawn.line_toolbar_viewport_margin_px),
                px(styles.self_drawn.line_toolbar_trigger_gap_px),
            )
            .unwrap();
        assert_eq!(origin, point(px(188.0), px(102.0)));
        let paint_bounds = Bounds::new(origin, size(px(100.0), px(40.0)));
        assert!(geometry.viewport.contains(&paint_bounds.origin));
        assert!(geometry.viewport.contains(&paint_bounds.bottom_right()));
    }
}
