use crate::style::{InputFrameStyle, ThemeCardStyle};
use gpui::{Div, div, prelude::*, px};

pub(crate) fn theme_card(style: ThemeCardStyle, scale: f32) -> Div {
    div()
        .w_full()
        .min_w(px(0.0))
        .p(px(style.padding_px * scale))
        .flex()
        .flex_col()
        .gap(px(style.gap_px * scale))
        .rounded(px(style.radius_px * scale))
        .border(px(style.border_width_px))
        .border_color(style.border_color)
        .bg(style.background)
        .when_some(style.elevation, |this, elevation| {
            this.shadow(elevation.shadows(scale))
        })
        .cursor(gpui::CursorStyle::Arrow)
        .hover(move |s| s.border_color(style.hover_border_color))
}

pub(crate) fn swatch_row(style: ThemeCardStyle, scale: f32) -> Div {
    div()
        .flex()
        .gap(px(style.swatch_gap_px * scale))
        .children(style.swatches.into_iter().map(|color| {
            div()
                .size(px(style.swatch_size_px * scale))
                .rounded(px(style.swatch_size_px * scale * 0.5))
                .bg(color)
        }))
}

pub(crate) fn input_frame(style: InputFrameStyle, scale: f32) -> Div {
    div()
        .w_full()
        .h(px(style.height_px * scale))
        .border(px(style.border_width_px))
        .border_color(style.border_color)
        .rounded(px(style.radius_px * scale))
        .bg(style.background)
        .overflow_hidden()
}
