use crate::style::{DetailRowStyle, PanelHeaderStyle, PanelSide, PanelStyle, TextStyle};
use gpui::{Div, SharedString, div, prelude::*, px};

pub(crate) fn panel(style: PanelStyle) -> Div {
    div()
        .h_full()
        .w_full()
        .relative()
        .overflow_hidden()
        .flex()
        .flex_col()
        .bg(style.background)
        .border_color(style.border_color)
        .when(style.side == PanelSide::Left, |this| {
            this.border_l(px(style.border_width_px))
        })
        .when(style.side == PanelSide::Right, |this| {
            this.border_r(px(style.border_width_px))
        })
}

pub(crate) fn header(style: PanelHeaderStyle, scale: f32) -> Div {
    super::typography::label(style.text, scale)
        .h(px(style.height_px * scale))
        .flex_shrink_0()
        .flex()
        .items_center()
        .px(px(style.padding_x_px * scale))
        .gap(px(style.gap_px * scale))
}

pub(crate) fn detail_row(style: DetailRowStyle, scale: f32) -> Div {
    div()
        .w_full()
        .min_h(px(style.min_height_px * scale))
        .px(px(style.padding_x_px * scale))
        .flex()
        .justify_between()
        .gap(px(style.gap_px * scale))
        .text_size(px(style.font_size_px * scale))
        .when(style.align_start, |this| this.items_start())
        .when(!style.align_start, |this| this.items_center())
        .when_some(style.hover_background, |this, hover| {
            this.cursor(gpui::CursorStyle::Arrow)
                .hover(move |s| s.bg(hover))
        })
}

pub(crate) fn empty(style: TextStyle, text: impl Into<SharedString>, scale: f32) -> Div {
    super::typography::label(style, scale)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .child(text.into())
}

pub(crate) fn binary_placeholder(style: crate::style::BinaryPlaceholderStyle, scale: f32) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(style.gap_px * scale))
        .text_color(style.detail.color)
}
