//! Whiteboard chrome: the bottom tool toolbar and the right-hand properties
//! panel, plus the style mutators they drive. Both are rendered as floating
//! overlays on top of the canvas when a whiteboard tab is active.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, ElementId, Hsla, MouseButton, Pixels, Window, canvas,
    div, fill, point, prelude::*, px, size, svg, transparent_black,
};

use super::settings_panel::FontRole;
use super::whiteboard::{
    CanvasTextAlign, CornerRadius, FillStyle, FontSize, PaletteColor, RouteStyle, StrokeRoughness,
    StrokeStyle, StrokeWidth, WhiteboardStyle, WhiteboardTool,
};
use super::whiteboard_geometry::{Pt, fill_polygon, hatch_segments, stroke_polyline};
use super::*;
use crate::theme::with_alpha;

const TOOL_ICON: &str = "assets/icons/whiteboard";

impl EditorView {
    // ── Bottom toolbar ───────────────────────────────────────────────────────

    pub(crate) fn render_whiteboard_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let tools = [
            (
                "wb-tool-select",
                "select.svg",
                WhiteboardTool::Select,
                "select",
                "V",
            ),
            (
                "wb-tool-rectangle",
                "rectangle.svg",
                WhiteboardTool::Rectangle,
                "rectangle",
                "R",
            ),
            (
                "wb-tool-ellipse",
                "ellipse.svg",
                WhiteboardTool::Ellipse,
                "ellipse",
                "O",
            ),
            (
                "wb-tool-arrow",
                "arrow.svg",
                WhiteboardTool::Arrow,
                "arrow",
                "A",
            ),
            (
                "wb-tool-line",
                "line.svg",
                WhiteboardTool::Line,
                "line",
                "L",
            ),
            (
                "wb-tool-text",
                "text.svg",
                WhiteboardTool::Text,
                "text",
                "T",
            ),
        ];
        let active = self
            .active_whiteboard()
            .map(|state| state.tool)
            .unwrap_or(WhiteboardTool::Select);
        let toolbar = &self.styles.toolbar;

        let mut pill = div()
            .id("wb-toolbar-pill")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(toolbar.toolbar_whiteboard_gap_px)))
            .p(px(self.scaled(toolbar.toolbar_whiteboard_padding_px)))
            .rounded_full()
            .bg(toolbar.toolbar_whiteboard_bg)
            .border(px(self.scaled(toolbar.toolbar_whiteboard_border_width_px)))
            .border_color(toolbar.toolbar_whiteboard_border_color)
            .when(toolbar.toolbar_whiteboard_shadow, |this| this.shadow_xs())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &gpui::MouseDownEvent, _, cx| cx.stop_propagation()),
            );
        for (id, icon, tool, label, shortcut) in tools {
            pill = pill.child(self.wb_tool_button(
                id,
                icon,
                tool,
                active == tool,
                crate::i18n::whiteboard_tool_label(self.ui_language, label),
                shortcut,
                cx,
            ));
        }

        div()
            .absolute()
            .bottom(px(self.scaled(toolbar.toolbar_whiteboard_bottom_px)))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(pill)
            .into_any_element()
    }

    fn wb_tool_button(
        &self,
        id: &'static str,
        icon: &'static str,
        tool: WhiteboardTool,
        active: bool,
        tooltip_title: &'static str,
        shortcut: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toolbar = &self.styles.toolbar;
        let icon_color = if active {
            toolbar.toolbar_whiteboard_icon_active_color
        } else {
            toolbar.toolbar_whiteboard_icon_color
        };
        let bg = if active {
            toolbar.toolbar_whiteboard_btn_active_bg
        } else {
            toolbar.toolbar_whiteboard_btn_bg
        };
        let hover_bg = toolbar.toolbar_whiteboard_btn_hover_bg;
        let button = div()
            .id(id)
            .w(px(self.scaled(toolbar.toolbar_whiteboard_btn_size_px)))
            .h(px(self.scaled(toolbar.toolbar_whiteboard_btn_size_px)))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(bg)
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(hover_bg))
            .child(
                svg()
                    .path(format!("{TOOL_ICON}/{icon}"))
                    .size(px(self.scaled(toolbar.toolbar_whiteboard_icon_size_px)))
                    .text_color(icon_color),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.whiteboard_set_tool(tool, cx);
                }),
            );
        tooltip::with_tooltip(button, cx.entity().clone(), tooltip_title, Some(shortcut))
    }

    // ── Properties panel (rendered inside the right utility panel) ──────────

    /// Whiteboard properties, shown in place of the file-info panel when a
    /// whiteboard tab is active.
    pub(crate) fn render_whiteboard_properties_panel(
        &mut self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let style = self.whiteboard_current_style();
        let ups = &self.styles.utility_panel;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let panel_bg = ups.utility_panel_bg;
        let border_width = ups.utility_panel_border_width_px;
        let border_color = ups.utility_panel_border_color;

        let sections = div()
            .px(px(section_padding_x))
            .flex()
            .flex_col()
            .gap(px(self.scaled(14.0)))
            .children(self.wb_property_fields(&style, cx));

        div()
            .id("wb-properties-panel")
            .h_full()
            .w_full()
            .bg(panel_bg)
            .overflow_hidden()
            .border_l(px(border_width))
            .border_color(border_color)
            .child(
                div()
                    .id("wb-properties-scroll")
                    .h_full()
                    .w_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(div().h(px(top_spacer)).flex_shrink_0())
                    .child(sections)
                    .child(div().h(px(bottom_spacer)).flex_shrink_0()),
            )
            .into_any_element()
    }

    /// All property fields, each with a caption, in display order: stroke
    /// color, background color, stroke width, stroke style, stroke roughness,
    /// corner radius, (route, when linear), fill style, font size, text align,
    /// opacity (whole element).
    fn wb_property_fields(
        &mut self,
        style: &WhiteboardStyle,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut fields: Vec<AnyElement> = Vec::new();

        // 描边颜色
        let stroke_swatches = self.wb_swatch_grid(
            "wb-stroke-swatch",
            &PaletteColor::STROKE_SWATCHES,
            style.stroke.color,
            cx,
            Self::whiteboard_apply_stroke_color,
        );
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "stroke_color"),
            stroke_swatches,
        ));

        // 填充颜色 (edits the canvas background when nothing is selected).
        let canvas_mode = !self.whiteboard_has_selection();
        let (current_bg_color, current_opacity) = if canvas_mode {
            self.active_whiteboard()
                .map(|state| {
                    (
                        state.document.app_state.background,
                        state.document.app_state.background_opacity,
                    )
                })
                .unwrap_or((PaletteColor::Transparent, 1.0))
        } else {
            (style.background.color, style.background.opacity)
        };
        let bg_swatches = self.wb_swatch_grid(
            "wb-bg-swatch",
            &PaletteColor::BACKGROUND_SWATCHES,
            current_bg_color,
            cx,
            Self::whiteboard_apply_background_color,
        );
        fields.push(self.wb_field(
            if canvas_mode {
                crate::i18n::whiteboard_label(self.ui_language, "canvas_color")
            } else {
                crate::i18n::whiteboard_label(self.ui_language, "fill_color")
            },
            bg_swatches,
        ));

        // 填充颜色透明度: preview the current fill from 20% to 100% opacity.
        if !canvas_mode && let Some(base_fill) = style.background.color.resolve(&self.theme) {
            let whiteboard_styles = &self.styles.whiteboard;
            let mut fill_opacity_row = div().w_full().flex().justify_between();
            for (index, opacity) in whiteboard_styles
                .whiteboard_fill_opacity_levels
                .into_iter()
                .enumerate()
            {
                let active = (style.background.opacity - opacity).abs() < 0.05;
                fill_opacity_row = fill_opacity_row.child(self.wb_color_swatch(
                    ("wb-fill-opacity", index),
                    with_alpha(base_fill, opacity),
                    active,
                    cx,
                    move |this, cx| this.whiteboard_apply_background_opacity(opacity, cx),
                ));
            }
            fields.push(self.wb_field(
                crate::i18n::whiteboard_label(self.ui_language, "fill_opacity"),
                fill_opacity_row.into_any_element(),
            ));
        }

        // 描边宽度 (fixed clean solid previews).
        let width = style.stroke.width;
        let mut width_row = div().w_full().flex();
        for (index, option) in [StrokeWidth::Small, StrokeWidth::Medium, StrokeWidth::Large]
            .into_iter()
            .enumerate()
        {
            let fg = self.wb_option_fg(width == option);
            width_row = width_row.child(self.wb_custom_pill(
                ("wb-sw", index),
                index == 0,
                width == option,
                line_glyph(
                    option.px(),
                    StrokeStyle::Solid,
                    StrokeRoughness::Clean,
                    fg,
                    self.ui_scale,
                ),
                cx,
                move |this, cx| this.whiteboard_apply_stroke_width(option, cx),
            ));
        }
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "stroke_width"),
            self.wb_segmented(width_row),
        ));

        // 描边样式 (solid / dashed / dotted at a fixed width, clean).
        let line_style = style.stroke.style;
        let mut style_row = div().w_full().flex();
        for (index, option) in [StrokeStyle::Solid, StrokeStyle::Dashed, StrokeStyle::Dotted]
            .into_iter()
            .enumerate()
        {
            let fg = self.wb_option_fg(line_style == option);
            style_row = style_row.child(self.wb_custom_pill(
                ("wb-ss", index),
                index == 0,
                line_style == option,
                line_glyph(2.0, option, StrokeRoughness::Clean, fg, self.ui_scale),
                cx,
                move |this, cx| this.whiteboard_apply_stroke_style(option, cx),
            ));
        }
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "stroke_style"),
            self.wb_segmented(style_row),
        ));

        // 描边风格 (clean / sketch / draft, exaggerated previews).
        let roughness = style.stroke.roughness;
        let mut roughness_row = div().w_full().flex();
        for (index, option) in [
            StrokeRoughness::Clean,
            StrokeRoughness::Sketch,
            StrokeRoughness::Draft,
        ]
        .into_iter()
        .enumerate()
        {
            let fg = self.wb_option_fg(roughness == option);
            roughness_row = roughness_row.child(self.wb_custom_pill(
                ("wb-rough", index),
                index == 0,
                roughness == option,
                line_glyph(1.8, StrokeStyle::Solid, option, fg, self.ui_scale),
                cx,
                move |this, cx| this.whiteboard_apply_roughness(option, cx),
            ));
        }
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "stroke_roughness"),
            self.wb_segmented(roughness_row),
        ));

        // 描边圆角 (rounded squares).
        let radius = style.corner_radius;
        let mut radius_row = div().w_full().flex();
        for (index, (option, preview_radius)) in [
            (CornerRadius::None, 0.0_f32),
            (CornerRadius::Small, 3.0),
            (CornerRadius::Medium, 5.0),
            (CornerRadius::Large, 7.0),
        ]
        .into_iter()
        .enumerate()
        {
            let fg = self.wb_option_fg(radius == option);
            let square = div()
                .w(px(self.scaled(14.0)))
                .h(px(self.scaled(14.0)))
                .border(px(self.scaled(1.5)))
                .border_color(fg)
                .rounded(px(preview_radius))
                .into_any_element();
            radius_row = radius_row.child(self.wb_custom_pill(
                ("wb-radius", index),
                index == 0,
                radius == option,
                square,
                cx,
                move |this, cx| this.whiteboard_apply_corner_radius(option, cx),
            ));
        }
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "corner_radius"),
            self.wb_segmented(radius_row),
        ));

        // 路径 (linear elements only).
        if self.whiteboard_show_route() {
            let route = style.route;
            let mut route_row = div().w_full().flex();
            for (index, option) in [
                RouteStyle::Straight,
                RouteStyle::Orthogonal,
                RouteStyle::Curved,
            ]
            .into_iter()
            .enumerate()
            {
                let fg = self.wb_option_fg(route == option);
                route_row = route_row.child(self.wb_custom_pill(
                    ("wb-route", index),
                    index == 0,
                    route == option,
                    route_glyph(option, fg, self.ui_scale),
                    cx,
                    move |this, cx| this.whiteboard_apply_route(option, cx),
                ));
            }
            fields.push(self.wb_field(
                crate::i18n::whiteboard_label(self.ui_language, "route"),
                self.wb_segmented(route_row),
            ));
        }

        // 填充样式 (fixed neutral previews).
        let fill = style.background.fill;
        let mut fill_row = div().w_full().flex();
        for (index, option) in [FillStyle::Solid, FillStyle::CrossHatch, FillStyle::Hatch]
            .into_iter()
            .enumerate()
        {
            let fg = self.wb_option_fg(fill == option);
            fill_row = fill_row.child(self.wb_custom_pill(
                ("wb-fill", index),
                index == 0,
                fill == option,
                fill_glyph(option, fg, fg, self.ui_scale),
                cx,
                move |this, cx| this.whiteboard_apply_fill(option, cx),
            ));
        }
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "fill_style"),
            self.wb_segmented(fill_row),
        ));

        // 字体大小 ("A" previews at proportional sizes).
        let size = style.font.size;
        let mut size_row = div().w_full().flex().items_end();
        for (index, (option, preview_px)) in [
            (FontSize::Small, 10.0_f32),
            (FontSize::Medium, 13.0),
            (FontSize::Large, 17.0),
        ]
        .into_iter()
        .enumerate()
        {
            let fg = self.wb_option_fg(size == option);
            let glyph = div()
                .text_size(px(self.scaled(preview_px)))
                .text_color(fg)
                .child("A")
                .into_any_element();
            size_row = size_row.child(self.wb_custom_pill(
                ("wb-fs", index),
                index == 0,
                size == option,
                glyph,
                cx,
                move |this, cx| this.whiteboard_apply_font_size(option, cx),
            ));
        }
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "font_size"),
            self.wb_segmented(size_row),
        ));

        // 字体颜色 (9 colors, no transparent; falls back to stroke color).
        let font_color = style.font.effective_color(style.stroke.color);
        let font_swatches = self.wb_swatch_grid(
            "wb-font-swatch",
            &PaletteColor::FONT_SWATCHES,
            font_color,
            cx,
            Self::whiteboard_apply_font_color,
        );
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "font_color"),
            font_swatches,
        ));

        // 字体 (searchable system-font dropdown, shared with settings).
        let family_trigger = self.wb_font_family_trigger(&style.font.family, cx);
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "font"),
            family_trigger,
        ));

        // 文本对齐
        let align = style.font.align;
        let align_row = div()
            .w_full()
            .flex()
            .child(self.wb_icon_option(
                "wb-al-left",
                true,
                "align-left.svg",
                align == CanvasTextAlign::Left,
                cx,
                |this, cx| this.whiteboard_apply_align(CanvasTextAlign::Left, cx),
            ))
            .child(self.wb_icon_option(
                "wb-al-center",
                false,
                "align-center.svg",
                align == CanvasTextAlign::Center,
                cx,
                |this, cx| this.whiteboard_apply_align(CanvasTextAlign::Center, cx),
            ))
            .child(self.wb_icon_option(
                "wb-al-right",
                false,
                "align-right.svg",
                align == CanvasTextAlign::Right,
                cx,
                |this, cx| this.whiteboard_apply_align(CanvasTextAlign::Right, cx),
            ));
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "text_align"),
            self.wb_segmented(align_row),
        ));

        // 画布颜色透明度 (only when nothing is selected).
        if canvas_mode {
            let opacity_preview = self.theme.text_secondary;
            let checker = self.theme.border_secondary;
            let levels = self.styles.whiteboard.whiteboard_fill_opacity_levels;
            let mut opacity_row = div().w_full().flex();
            for (index, value) in levels.into_iter().enumerate() {
                let active = (current_opacity - value).abs() < 0.05;
                let glyph = div()
                    .w(px(self.scaled(14.0)))
                    .h(px(self.scaled(14.0)))
                    .rounded(px(self.scaled(4.0)))
                    .border_1()
                    .border_color(checker)
                    .bg(with_alpha(opacity_preview, value))
                    .into_any_element();
                opacity_row = opacity_row.child(self.wb_custom_pill(
                    ("wb-bg-opacity", index),
                    index == 0,
                    active,
                    glyph,
                    cx,
                    move |this, cx| this.whiteboard_apply_background_opacity(value, cx),
                ));
            }
            fields.push(self.wb_field(
                crate::i18n::whiteboard_label(self.ui_language, "canvas_opacity"),
                self.wb_segmented(opacity_row),
            ));
        }

        // 导出 (selected elements, or the whole canvas when unselected).
        let export_row = div()
            .w_full()
            .flex()
            .child(
                self.wb_text_option("wb-export-png", true, "PNG", cx, |this, cx| {
                    this.whiteboard_export_png(cx)
                }),
            )
            .child(
                self.wb_text_option("wb-export-svg", false, "SVG", cx, |this, cx| {
                    this.whiteboard_export_svg(cx)
                }),
            )
            .child(
                self.wb_text_option("wb-export-json", false, "JSON", cx, |this, cx| {
                    this.whiteboard_export_json(cx)
                }),
            );
        fields.push(self.wb_field(
            crate::i18n::whiteboard_label(self.ui_language, "export"),
            self.wb_segmented(export_row),
        ));

        fields
    }

    /// A captioned property field: small label above its control row.
    fn wb_field(&self, label: &'static str, row: AnyElement) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(6.0)))
            .child(self.wb_label(label))
            .child(row)
            .into_any_element()
    }

    /// Whether to show the linear-routing controls (a linear element is
    /// selected, or a linear drawing tool is active).
    fn whiteboard_show_route(&self) -> bool {
        if let Some(state) = self.active_whiteboard() {
            if matches!(state.tool, WhiteboardTool::Arrow | WhiteboardTool::Line) {
                return true;
            }
            return state.selected_elements().any(|e| e.kind.is_linear());
        }
        false
    }

    fn wb_label(&self, text: &'static str) -> AnyElement {
        div()
            .text_size(px(self.scaled(11.0)))
            .text_color(self.theme.text_secondary)
            .child(text)
            .into_any_element()
    }

    fn wb_swatch(
        &self,
        id: impl Into<ElementId>,
        color: PaletteColor,
        active: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        if color == PaletteColor::Transparent {
            return self.wb_transparent_swatch(id, active, cx, on_click);
        }
        let resolved = color.resolve(&self.theme);
        let fill = resolved.unwrap_or(self.theme.bg_primary);
        self.wb_color_swatch(id, fill, active, cx, on_click)
    }

    fn wb_swatch_frame(
        &self,
        id: impl Into<ElementId>,
        active: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        child: AnyElement,
    ) -> AnyElement {
        let style = &self.styles.whiteboard;
        let ring_color = if active {
            style.whiteboard_swatch_ring_color
        } else {
            style.whiteboard_swatch_ring_idle_color
        };
        let hover_ring_color = style.whiteboard_swatch_ring_color;

        div()
            .id(id)
            .w(px(self.scaled(style.whiteboard_swatch_size_px)))
            .h(px(self.scaled(style.whiteboard_swatch_size_px)))
            .rounded_full()
            .border(px(self.scaled(style.whiteboard_swatch_ring_width_px)))
            .border_color(ring_color)
            .p(px(self.scaled(style.whiteboard_swatch_ring_gap_px)))
            .cursor(CursorStyle::Arrow)
            .hover(move |s| s.border_color(hover_ring_color))
            .child(child)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    on_click(this, cx);
                }),
            )
            .into_any_element()
    }

    fn wb_transparent_swatch(
        &self,
        id: impl Into<ElementId>,
        active: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let style = &self.styles.whiteboard;
        let light = style.whiteboard_swatch_checker_light_color;
        let dark = style.whiteboard_swatch_checker_dark_color;
        let cell = self
            .scaled(style.whiteboard_swatch_checker_cell_px)
            .max(1.0);
        let checker = div().w_full().h_full().rounded_full().child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
                    let origin_x = f32::from(bounds.origin.x);
                    let origin_y = f32::from(bounds.origin.y);
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    let diameter = width.min(height);
                    let radius = diameter * 0.5;
                    let center_x = origin_x + width * 0.5;
                    let center_y = origin_y + height * 0.5;
                    let top = center_y - radius;
                    let bottom = center_y + radius;
                    let strip_h = 1.0_f32;
                    let mut y = top;
                    while y < bottom {
                        let h = (bottom - y).min(strip_h);
                        let mid_y = y + h * 0.5;
                        let dy = mid_y - center_y;
                        let half_width = (radius * radius - dy * dy).max(0.0).sqrt();
                        let mut x = center_x - half_width;
                        let right = center_x + half_width;
                        let row = ((mid_y - origin_y) / cell).floor() as i32;
                        while x < right {
                            let col = ((x - origin_x) / cell).floor() as i32;
                            let cell_right = origin_x + (col + 1) as f32 * cell;
                            let next_x = right.min(cell_right).max(x + 0.1);
                            let color = if (row + col) % 2 == 0 { light } else { dark };
                            window.paint_quad(fill(
                                Bounds::new(point(px(x), px(y)), size(px(next_x - x), px(h))),
                                color,
                            ));
                            x = next_x;
                        }
                        y += h;
                    }
                },
            )
            .w_full()
            .h_full(),
        );
        self.wb_swatch_frame(id, active, cx, on_click, checker.into_any_element())
    }

    fn wb_color_swatch(
        &self,
        id: impl Into<ElementId>,
        fill: Hsla,
        active: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let inner = div()
            .w_full()
            .h_full()
            .rounded_full()
            .bg(fill)
            .into_any_element();
        self.wb_swatch_frame(id, active, cx, on_click, inner)
    }

    /// A grid of circular color swatches, 5 per row, spread edge-to-edge with
    /// space-between (partial rows are padded with invisible placeholders so
    /// columns stay aligned).
    fn wb_swatch_grid(
        &self,
        id_prefix: &'static str,
        colors: &[PaletteColor],
        current: PaletteColor,
        cx: &mut Context<Self>,
        apply: fn(&mut Self, PaletteColor, &mut Context<Self>),
    ) -> AnyElement {
        let style = &self.styles.whiteboard;
        let mut column = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(self.scaled(style.whiteboard_swatch_grid_gap_y_px)));
        for (row_index, chunk) in colors.chunks(5).enumerate() {
            let mut row = div().w_full().flex().flex_row().justify_between();
            for (col_index, color) in chunk.iter().enumerate() {
                let color = *color;
                let index = row_index * 5 + col_index;
                row = row.child(self.wb_swatch(
                    (id_prefix, index),
                    color,
                    current == color,
                    cx,
                    move |this, cx| apply(this, color, cx),
                ));
            }
            for _ in chunk.len()..5 {
                row = row.child(
                    div()
                        .w(px(self.scaled(style.whiteboard_swatch_size_px)))
                        .h(px(self.scaled(style.whiteboard_swatch_size_px))),
                );
            }
            column = column.child(row);
        }
        column.into_any_element()
    }

    /// The trigger button for the whiteboard font-family picker. Opens the
    /// shared searchable font dropdown anchored to this button.
    fn wb_font_family_trigger(
        &mut self,
        family: &Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = family.clone().unwrap_or_else(|| {
            crate::i18n::whiteboard_label(self.ui_language, "default").to_string()
        });
        let bounds_cell = self
            .font_picker_button_bounds
            .entry((FontRole::WhiteboardFont, 0))
            .or_insert_with(|| Rc::new(Cell::new(None)))
            .clone();
        let border_color = self.theme.border_primary;
        let hover_bg = self.theme.bg_hover;
        div()
            .w_full()
            .on_children_prepainted({
                let bounds_cell = bounds_cell.clone();
                move |bounds: Vec<Bounds<Pixels>>, _, _| {
                    if let Some(b) = bounds.first() {
                        bounds_cell.set(Some(*b));
                    }
                }
            })
            .child(
                div()
                    .id("wb-font-family")
                    .w_full()
                    .h(px(self.scaled(28.0)))
                    .px(px(self.scaled(8.0)))
                    .flex()
                    .items_center()
                    .rounded(px(self.scaled(6.0)))
                    .border_1()
                    .border_color(border_color)
                    .text_size(px(self.scaled(12.0)))
                    .text_color(self.theme.text_primary)
                    .cursor(CursorStyle::Arrow)
                    .hover(move |style| style.bg(hover_bg))
                    .child(label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.open_font_picker(FontRole::WhiteboardFont, 0, window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    /// Foreground color for an option glyph, by active state (matches the
    /// settings theme-mode toggle).
    fn wb_option_fg(&self, active: bool) -> Hsla {
        if active {
            self.theme.text_bright
        } else {
            self.theme.text_secondary
        }
    }

    /// Wrap a row of options in the bordered segmented-control container used
    /// by the settings toggles.
    fn wb_segmented(&self, row: gpui::Div) -> AnyElement {
        row.w_full()
            .flex()
            .rounded(px(self.scaled(6.0)))
            .border_1()
            .border_color(self.theme.border_primary)
            .overflow_hidden()
            .into_any_element()
    }

    /// A segment in a segmented-control row hosting an arbitrary (usually
    /// drawn) child instead of a text label. Styled like the settings
    /// theme-mode toggle: selected background fill and a separator before
    /// every segment but the first.
    fn wb_custom_pill(
        &self,
        id: impl Into<ElementId>,
        first: bool,
        active: bool,
        child: AnyElement,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let bg = if active {
            self.theme.bg_selected
        } else {
            transparent_black()
        };
        let hover_bg = self.theme.bg_hover;
        div()
            .id(id)
            .px(px(self.scaled(8.0)))
            .h(px(self.scaled(26.0)))
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .cursor(CursorStyle::Arrow)
            .when(!first, |this| {
                this.border_l_1().border_color(self.theme.border_primary)
            })
            .when(!active, |this| this.hover(move |style| style.bg(hover_bg)))
            .child(child)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    on_click(this, cx);
                }),
            )
            .into_any_element()
    }

    /// A plain-text action button in a segmented row (used for export).
    fn wb_text_option(
        &self,
        id: impl Into<ElementId>,
        first: bool,
        label: &'static str,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let hover_bg = self.theme.bg_hover;
        div()
            .id(id)
            .flex_1()
            .h(px(self.scaled(26.0)))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(self.scaled(11.0)))
            .text_color(self.theme.text_secondary)
            .cursor(CursorStyle::Arrow)
            .when(!first, |this| {
                this.border_l_1().border_color(self.theme.border_primary)
            })
            .hover(move |style| style.bg(hover_bg))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    on_click(this, cx);
                }),
            )
            .into_any_element()
    }

    fn wb_icon_option(
        &self,
        id: impl Into<ElementId>,
        first: bool,
        icon: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let (bg, fg) = if active {
            (self.theme.bg_selected, self.theme.text_bright)
        } else {
            (transparent_black(), self.theme.text_secondary)
        };
        let hover_bg = self.theme.bg_hover;
        div()
            .id(id)
            .flex_1()
            .h(px(self.scaled(26.0)))
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .cursor(CursorStyle::Arrow)
            .when(!first, |this| {
                this.border_l_1().border_color(self.theme.border_primary)
            })
            .when(!active, |this| this.hover(move |style| style.bg(hover_bg)))
            .child(
                svg()
                    .path(format!("{TOOL_ICON}/{icon}"))
                    .size(px(self.scaled(16.0)))
                    .text_color(fg),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    on_click(this, cx);
                }),
            )
            .into_any_element()
    }

    // ── Style mutators ───────────────────────────────────────────────────────

    /// The style shown in the panel: the single selected element's style, or
    /// the default style when nothing (or multiple) is selected.
    fn whiteboard_current_style(&self) -> WhiteboardStyle {
        if let Some(state) = self.active_whiteboard() {
            if let Some(first) = state.selected_ids.first()
                && let Some(element) = state.document.element(first)
            {
                return element.style();
            }
            return state.default_style.clone();
        }
        WhiteboardStyle::default()
    }

    pub(crate) fn whiteboard_set_tool(&mut self, tool: WhiteboardTool, cx: &mut Context<Self>) {
        if let Some(state) = self.active_whiteboard_mut() {
            state.tool = tool;
        }
        cx.notify();
    }

    fn whiteboard_apply_stroke_color(&mut self, color: PaletteColor, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.stroke.color = color;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.stroke.color = color;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_stroke_width(&mut self, width: StrokeWidth, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.stroke.width = width;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.stroke.width = width;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_stroke_style(&mut self, line_style: StrokeStyle, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.stroke.style = line_style;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.stroke.style = line_style;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_roughness(&mut self, roughness: StrokeRoughness, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.stroke.roughness = roughness;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.stroke.roughness = roughness;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_corner_radius(&mut self, radius: CornerRadius, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.corner_radius = radius;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.corner_radius = radius;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_route(&mut self, route: RouteStyle, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.route = route;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) && element.kind.is_linear() {
                    element.route = route;
                    // Seed a visible curvature when first switching to curved.
                    if route == RouteStyle::Curved && element.route_curve.abs() < 1.0 {
                        element.route_curve = 36.0;
                    }
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_fill(&mut self, fill: FillStyle, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.background.fill = fill;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.background.fill = fill;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_background_color(&mut self, color: PaletteColor, cx: &mut Context<Self>) {
        if !self.whiteboard_has_selection() {
            // No selection: edit the canvas background.
            self.whiteboard_push_undo();
            if let Some(state) = self.active_whiteboard_mut() {
                state.document.app_state.background = color;
            }
            self.mark_whiteboard_dirty();
            cx.notify();
            return;
        }
        self.whiteboard_push_undo();
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.background.color = color;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.background.color = color;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_background_opacity(&mut self, opacity: f32, cx: &mut Context<Self>) {
        if !self.whiteboard_has_selection() {
            // No selection: edit the canvas background opacity.
            self.whiteboard_push_undo();
            if let Some(state) = self.active_whiteboard_mut() {
                state.document.app_state.background_opacity = opacity;
            }
            self.mark_whiteboard_dirty();
            cx.notify();
            return;
        }
        self.whiteboard_push_undo();
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.background.opacity = opacity;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.background.opacity = opacity;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_font_size(&mut self, size: FontSize, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.font.size = size;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.font.size = size;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_align(&mut self, align: CanvasTextAlign, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.font.align = align;
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.font.align = align;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    fn whiteboard_apply_font_color(&mut self, color: PaletteColor, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.font.color = Some(color);
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.font.color = Some(color);
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }

    /// Apply a font family to the selection (and the default style). Called
    /// from the shared font picker when its role is `WhiteboardFont`.
    pub(crate) fn whiteboard_apply_font_family(&mut self, family: String, cx: &mut Context<Self>) {
        if self.whiteboard_has_selection() {
            self.whiteboard_push_undo();
        }
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut() {
            state.default_style.font.family = Some(family.clone());
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.font.family = Some(family.clone());
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_whiteboard_dirty();
        }
        cx.notify();
    }
}

// ── Drawn option glyphs ──────────────────────────────────────────────────────

/// A horizontal line preview drawn at a fixed pure style. Roughness is
/// rendered as an exaggerated hand-drawn wobble so the three levels are
/// clearly distinguishable at glyph size.
fn line_glyph(
    width: f32,
    style: StrokeStyle,
    roughness: StrokeRoughness,
    color: Hsla,
    scale: f32,
) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
            let x0: f32 = f32::from(bounds.origin.x) + 1.0;
            let x1: f32 = f32::from(bounds.origin.x) + f32::from(bounds.size.width) - 1.0;
            let cy: f32 = f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5;
            let (amplitude, passes) = match roughness {
                StrokeRoughness::Clean => (0.0_f32, 1usize),
                StrokeRoughness::Sketch => (2.4, 1),
                StrokeRoughness::Draft => (3.6, 2),
            };
            if amplitude <= 0.0 {
                for path in stroke_polyline(&[(x0, cy), (x1, cy)], width, style, false) {
                    window.paint_path(path, color);
                }
                return;
            }
            // Fixed exaggerated wave (deterministic, not derived from element
            // roughness math) so the preview reads clearly.
            let offsets = [0.0_f32, 0.9, -1.0, 0.6, -0.85, 1.0, -0.4, 0.0];
            for pass in 0..passes {
                let flip = if pass == 0 { 1.0 } else { -0.75 };
                let pts: Vec<Pt> = offsets
                    .iter()
                    .enumerate()
                    .map(|(i, offset)| {
                        let t = i as f32 / (offsets.len() - 1) as f32;
                        (x0 + (x1 - x0) * t, cy + offset * amplitude * flip)
                    })
                    .collect();
                for path in stroke_polyline(&pts, width, style, false) {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .w(px(26.0 * scale))
    .h(px(14.0 * scale))
    .into_any_element()
}

/// A connector-route preview: straight diagonal, orthogonal elbow, or curve.
fn route_glyph(route: RouteStyle, color: Hsla, scale: f32) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
            let x0: f32 = f32::from(bounds.origin.x) + 1.5;
            let x1: f32 = f32::from(bounds.origin.x) + f32::from(bounds.size.width) - 1.5;
            let y0: f32 = f32::from(bounds.origin.y) + 1.5;
            let y1: f32 = f32::from(bounds.origin.y) + f32::from(bounds.size.height) - 1.5;
            let pts: Vec<Pt> = match route {
                RouteStyle::Straight => vec![(x0, y1), (x1, y0)],
                RouteStyle::Orthogonal => {
                    let mid_x = (x0 + x1) * 0.5;
                    vec![(x0, y1), (mid_x, y1), (mid_x, y0), (x1, y0)]
                }
                RouteStyle::Curved => {
                    let control = ((x0 + x1) * 0.5 - (y1 - y0) * 0.6, y0);
                    let segments = 14;
                    (0..=segments)
                        .map(|i| {
                            let t = i as f32 / segments as f32;
                            let mt = 1.0 - t;
                            (
                                mt * mt * x0 + 2.0 * mt * t * control.0 + t * t * x1,
                                mt * mt * y1 + 2.0 * mt * t * control.1 + t * t * y0,
                            )
                        })
                        .collect()
                }
            };
            for path in stroke_polyline(&pts, 1.6, StrokeStyle::Solid, false) {
                window.paint_path(path, color);
            }
        },
    )
    .w(px(22.0 * scale))
    .h(px(14.0 * scale))
    .into_any_element()
}

/// A fill-style preview swatch: solid, cross-hatched, or hatched square.
fn fill_glyph(fill: FillStyle, color: Hsla, border: Hsla, scale: f32) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
            let x0: f32 = f32::from(bounds.origin.x) + 1.0;
            let x1: f32 = f32::from(bounds.origin.x) + f32::from(bounds.size.width) - 1.0;
            let y0: f32 = f32::from(bounds.origin.y) + 1.0;
            let y1: f32 = f32::from(bounds.origin.y) + f32::from(bounds.size.height) - 1.0;
            let poly = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
            match fill {
                FillStyle::Solid => {
                    if let Some(path) = fill_polygon(&poly) {
                        window.paint_path(path, color);
                    }
                }
                FillStyle::Hatch | FillStyle::CrossHatch => {
                    let mut draw = |flip: bool| {
                        for (a, b) in hatch_segments(&poly, 4.0, flip) {
                            for path in stroke_polyline(&[a, b], 1.1, StrokeStyle::Solid, false) {
                                window.paint_path(path, color);
                            }
                        }
                    };
                    draw(true);
                    if fill == FillStyle::CrossHatch {
                        draw(false);
                    }
                }
            }
            // Outline so light colors stay visible.
            for path in stroke_polyline(&poly, 1.0, StrokeStyle::Solid, true) {
                window.paint_path(path, border);
            }
        },
    )
    .w(px(16.0 * scale))
    .h(px(14.0 * scale))
    .into_any_element()
}
