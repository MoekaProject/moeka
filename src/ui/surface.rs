use gpui::{Div, ElementId, Stateful, div, prelude::*, px};

use crate::style::{SeparatorStyle, SurfaceStyle};

/// Shared chrome for panels, popovers, dialogs and notifications. Position,
/// scrolling, dismissal and focus trapping are owned by the composition.
pub(crate) fn surface(style: &SurfaceStyle, scale: f32) -> Div {
    div()
        .bg(style.background)
        .text_color(style.foreground)
        .border(px(
            style.border_width_px * if style.scale_border { scale } else { 1.0 }
        ))
        .border_color(style.border_color)
        .rounded(px(style.radius_px * scale))
        .shadow(
            style
                .shadow
                .layers(if style.scale_shadow { scale } else { 1.0 }),
        )
        .opacity(style.opacity)
}

pub(crate) fn menu_scroll_area(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).w_full().min_h(px(0.0)).overflow_y_scroll()
}

pub(crate) fn separator(style: SeparatorStyle, scale: f32) -> Div {
    div()
        .w_full()
        .h(px(
            style.thickness_px * if style.scale_thickness { scale } else { 1.0 }
        ))
        .my(px(style.margin_px * scale))
        .bg(style.color)
        .flex_none()
}

pub(crate) fn drag_preview(
    style: &crate::style::DragPreviewStyle,
    offset: gpui::Point<gpui::Pixels>,
    text: impl Into<gpui::SharedString>,
) -> Div {
    div()
        .pl(offset.x + px(style.cursor_gap_px))
        .pt(offset.y + px(style.cursor_gap_px))
        .child(
            surface(&style.surface, 1.0)
                .px(px(style.padding_x_px))
                .py(px(style.padding_y_px))
                .text_size(px(style.font_size_px))
                .child(text.into()),
        )
}
