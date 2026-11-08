use crate::style::{FieldStyle, SectionStyle};
use gpui::{AnyElement, Div, SharedString, div, prelude::*, px};

pub(crate) fn field(
    style: FieldStyle,
    scale: f32,
    label: impl Into<SharedString>,
    control: AnyElement,
) -> Div {
    let vertical = style.orientation == crate::style::Orientation::Vertical;
    super::surface::surface(&style.surface, scale)
        .flex()
        .when(vertical, |this| this.flex_col().items_start())
        .when(!vertical, |this| this.items_center())
        .gap(px(style.gap_px * scale))
        .child(
            super::typography::label(style.label, scale)
                .when(vertical, |this| this.w_full())
                .when(!vertical, |this| this.w(px(style.label_width_px * scale)))
                .child(label.into()),
        )
        .child(
            div()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .when(vertical, |this| this.w_full())
                .when(!vertical, |this| this.flex_1())
                .child(control),
        )
}

pub(crate) fn section_heading(
    style: SectionStyle,
    scale: f32,
    title: impl Into<SharedString>,
) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(style.gap_px * scale))
        .child(super::typography::label(style.title, scale).child(title.into()))
        .child(super::surface::separator(style.separator, scale))
}
