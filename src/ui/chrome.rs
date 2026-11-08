use crate::style::{StatusBarStyle, TabSurfaceStyle};
use gpui::{Div, div, prelude::*, px};

pub(crate) fn status_bar(style: StatusBarStyle, scale: f32, padding_left_px: f32) -> Div {
    div()
        .h(px(style.height_px * scale))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .pl(px(padding_left_px * scale))
        .pr(px(0.0))
        .border_t(px(style.border_width_px))
        .border_color(style.border)
        .bg(style.background)
        .text_size(px(style.font_size_px * scale))
}

pub(crate) fn tab_surface(style: TabSurfaceStyle, scale: f32, height: f32, active: bool) -> Div {
    div()
        .relative()
        .h(px(height))
        .pl(px(style.padding_left_px * scale))
        .pr(px(style.padding_right_px * scale))
        .flex()
        .items_center()
        .gap(px(style.gap_px * scale))
        .bg(if active {
            style.active_background
        } else {
            style.background
        })
        .border_r(px(style.border_width_px))
        .border_color(style.border_color)
        .when(!active, |this| {
            this.hover(move |s| s.bg(style.hover_background))
        })
}

/// Fullscreen/CSD corner policy stays with the platform-aware window owner.
pub(crate) fn window_frame(style: crate::style::WindowFrameStyle, radius: f32, scale: f32) -> Div {
    div()
        .relative()
        .size_full()
        .flex()
        .flex_row()
        .overflow_hidden()
        .font_family(gpui::SharedString::from(style.font_family))
        .when_some(style.font_fallbacks, |mut this, families| {
            this.text_style()
                .get_or_insert_with(Default::default)
                .font_fallbacks = Some(gpui::FontFallbacks::from_fonts(
                families.iter().map(|family| (*family).to_owned()).collect(),
            ));
            this
        })
        .text_size(px(style.font_size_px * scale))
        .bg(style.background)
        .border(px(style.border_width_px))
        .border_color(style.border)
        .rounded(px(radius))
}

pub(crate) fn navigation_header(
    style: crate::style::NavigationHeaderStyle,
    scale: f32,
) -> gpui::Div {
    gpui::div()
        .w_full()
        .h(px(style.height_px * scale))
        .flex_none()
        .pl(px(style.left_px * scale))
        .pr(px(style.right_px * scale))
        .flex()
        .items_center()
        .gap(px(style.gap_px * scale))
        .bg(style.background)
        .border_b(px(style.border_width_px))
        .border_color(style.border)
        .hover(move |s| s.bg(style.hover))
}
