use crate::style::{DecorationStyle, EllipsisStyle, FileNameStyle, ThumbnailStyle};
use gpui::{Div, SharedString, div, prelude::*, px};

pub(crate) fn swatch(style: DecorationStyle, scale: f32) -> Div {
    div()
        .size(px(style.size_px * scale))
        .rounded(px(style.radius_px * scale))
        .bg(style.background)
        .border(px(style.border_width_px))
        .border_color(style.border)
}

pub(crate) fn ellipsis(style: EllipsisStyle, scale: f32) -> Div {
    div()
        .flex()
        .gap(px(style.gap_px * scale))
        .children((0..style.count).map(|_| {
            div()
                .size(px(style.dot_size_px * scale))
                .rounded_full()
                .bg(style.color)
        }))
}

pub(crate) fn file_name(style: FileNameStyle, scale: f32) -> Div {
    super::typography::label(style.text, scale)
        .opacity(style.opacity)
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .flex()
        .gap(px(0.0))
        .items_center()
}

pub(crate) fn file_name_base(text: impl Into<SharedString>) -> Div {
    file_name_base_content(text.into())
}

pub(crate) fn file_name_base_content(content: impl IntoElement) -> Div {
    div()
        .flex_initial()
        .min_w(px(0.0))
        .overflow_hidden()
        .truncate()
        .whitespace_nowrap()
        .child(content)
}

pub(crate) fn suffix(opacity: f32, text: impl Into<SharedString>) -> Div {
    suffix_content(opacity, text.into())
}

pub(crate) fn suffix_content(opacity: f32, content: impl IntoElement) -> Div {
    div()
        .flex_none()
        .whitespace_nowrap()
        .opacity(opacity)
        .child(content)
}

/// The decoded image painter receives the same radius as its clipping frame.
pub(crate) fn thumbnail(style: ThumbnailStyle, size: f32, scale: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .overflow_hidden()
        .rounded(px(style.radius_px * scale))
        .border(px(style.border_width_px))
        .border_color(style.border)
}

pub(crate) fn icon(style: crate::style::IconStyle, path: impl Into<SharedString>) -> gpui::Svg {
    gpui::svg()
        .path(path)
        .size(px(style.size_px))
        .text_color(style.color)
}

pub(crate) fn info_section(
    style: crate::style::InfoSectionStyle,
    scale: f32,
    icon_path: &'static str,
    title: impl Into<SharedString>,
) -> Div {
    super::typography::label(style.text, scale)
        .mb(px(style.margin_bottom_px * scale))
        .px(px(style.padding_x_px * scale))
        .flex()
        .items_center()
        .gap(px(style.gap_px * scale))
        .child(icon(
            crate::style::IconStyle::new(style.icon_size_px * scale, style.text.color),
            icon_path,
        ))
        .child(title.into())
}

/// Positions already come from the owner's shared layout/hit geometry.
pub(crate) fn guide_segment(x: f32, top: f32, width: f32, height: f32, color: gpui::Hsla) -> Div {
    div()
        .absolute()
        .left(px(x))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .bg(color)
}

pub(crate) fn outline_tick(width: f32, thickness: f32, color: gpui::Hsla) -> Div {
    div().w(px(width)).h(px(thickness)).rounded_full().bg(color)
}
