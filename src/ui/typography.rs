use crate::style::{BadgeStyle, TextOverflow, TextStyle};
use gpui::{Div, SharedString, div, prelude::*, px};

pub(crate) fn label(style: TextStyle, scale: f32) -> Div {
    div()
        .text_size(px(style.size_px * scale))
        .font_weight(style.weight)
        .when_some(style.font_fallbacks, |mut this, families| {
            this.text_style()
                .get_or_insert_with(Default::default)
                .font_fallbacks = Some(gpui::FontFallbacks::from_fonts(
                families.iter().map(|family| (*family).to_owned()).collect(),
            ));
            this
        })
        .when(style.italic, |this| this.italic())
        .when(
            style.decoration == crate::style::TextDecoration::Underline,
            |this| this.underline(),
        )
        .when(
            style.decoration == crate::style::TextDecoration::Strikethrough,
            |this| this.line_through(),
        )
        .text_color(style.color)
        .when_some(style.line_height_px, |this, height| {
            this.line_height(px(height * scale))
        })
        .when_some(style.font_family, |this, family| {
            this.font_family(SharedString::from(family))
        })
        .when(style.overflow == TextOverflow::Ellipsis, |this| {
            this.min_w(px(0.0)).truncate().whitespace_nowrap()
        })
        .when(style.overflow == TextOverflow::Wrap, |this| {
            this.whitespace_normal()
        })
}

pub(crate) fn badge(style: BadgeStyle, scale: f32) -> Div {
    label(style.text, scale)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .px(px(style.padding_x_px * scale))
        .py(px(style.padding_y_px * scale))
        .rounded(px(style.radius_px * scale))
        .bg(style.background)
        .border(px(style.border_width_px * scale))
        .border_color(style.border_color)
}

/// `shortcut` retains tokenization and width measurement; this owns the key slot.
pub(crate) fn keycap(style: TextStyle, width_px: f32) -> Div {
    let height = style.size_px;
    label(style, 1.0)
        .w(px(width_px))
        .h(px(height))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .line_height(px(height))
}

pub(crate) fn fragment(color: gpui::Hsla) -> Div {
    div().text_color(color)
}
