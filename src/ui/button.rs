use gpui::{Div, ElementId, Stateful, div, prelude::*, px};

use crate::style::{ControlState, ControlStyle, GroupPosition};

/// Presentation only: callers retain IDs, focus handles and action timing.
/// Disabled/loading controls occlude pointer input before caller listeners run.
pub(crate) fn button_frame(style: ControlStyle, state: ControlState, scale: f32) -> Div {
    let resolved = style.resolve(state);
    let hover = style.resolve(ControlState {
        hovered: true,
        ..state
    });
    div()
        .flex_none()
        .h(px(style.height_px * scale))
        .min_w(px(style.min_width_px * scale))
        .px(px(style.padding_x_px * scale))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(style.gap_px * scale))
        .rounded(px((if style.full_round && style.radius_px > 0.0 {
            style.height_px * 0.5
        } else {
            style.radius_px
        }) * if style.scale_radius { scale } else { 1.0 }))
        .border(px(
            style.border_width_px * if style.scale_border { scale } else { 1.0 }
        ))
        .border_color(resolved.border)
        .bg(resolved.background)
        .text_size(px(style.font_size_px * scale))
        .font_weight(style.font_weight)
        .text_color(resolved.foreground)
        .opacity(resolved.opacity)
        .cursor(style.cursor)
        .when(
            !state.disabled && !state.loading && !state.read_only,
            |this| {
                this.hover(move |s| {
                    let s = s
                        .bg(hover.background)
                        .text_color(hover.foreground)
                        .border_color(hover.border)
                        .opacity(hover.opacity);
                    if style.underline_on_hover {
                        s.underline()
                    } else {
                        s
                    }
                })
            },
        )
        .when(
            state.disabled || state.loading || state.read_only,
            super::interaction::inert_activation,
        )
}

pub(crate) fn button(
    id: impl Into<ElementId>,
    style: ControlStyle,
    state: ControlState,
    scale: f32,
) -> Stateful<Div> {
    let ring = style.ring.shadows(scale, state.invalid);
    let raised = raised_shadows(style, state, scale);
    let with_ring = [raised.clone(), ring.clone()].concat();
    button_frame(style, state, scale)
        .id(id)
        .when(!raised.is_empty(), |this| this.shadow(raised.clone()))
        .when(state.focus_visible, |this| this.shadow(with_ring.clone()))
        .when(
            !state.disabled && !state.loading && !state.read_only,
            |this| this.focus(move |s| s.shadow(with_ring)),
        )
}

/// Raised controls cast a shadow only while their resting state has a visible
/// surface, so ghost controls stay flat until selected.
pub(crate) fn raised_shadows(
    style: ControlStyle,
    state: ControlState,
    scale: f32,
) -> Vec<gpui::BoxShadow> {
    let Some(elevation) = style.elevation else {
        return Vec::new();
    };
    let resting = style.resolve(ControlState {
        hovered: false,
        pressed: false,
        ..state
    });
    let visible_border = style.border_width_px > 0.0 && resting.border.a > 0.01;
    if state.disabled || (resting.background.a <= 0.01 && !visible_border) {
        return Vec::new();
    }
    elevation.shadows(scale)
}

pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    style: ControlStyle,
    state: ControlState,
    scale: f32,
) -> Stateful<Div> {
    button(id, style, state, scale)
        .w(px(style.height_px * scale))
        .px(px(0.0))
}

/// A joined toggle's outer corners and seams are independent of selection.
pub(crate) fn toggle_segment(
    id: impl Into<ElementId>,
    style: ControlStyle,
    state: ControlState,
    position: GroupPosition,
    scale: f32,
) -> Stateful<Div> {
    let border = style.border_width_px * if style.scale_border { scale } else { 1.0 };
    let radius = (if style.full_round && style.radius_px > 0.0 {
        style.height_px * 0.5
    } else {
        style.radius_px
    }) * if style.scale_radius { scale } else { 1.0 };
    if style.elevation.is_some() && border == 0.0 {
        // Raised segments sit apart inside a tray instead of sharing seams.
        return button(id, style, state, scale);
    }
    let element = button(id, style, state, scale).rounded(px(0.0));
    match position.orientation {
        crate::style::Orientation::Horizontal => element
            .border_l(px(if position.first { border } else { 0.0 }))
            .border_r(px(if position.last { border } else { 0.0 }))
            .when(position.first, |this| this.rounded_l(px(radius)))
            .when(position.last, |this| this.rounded_r(px(radius))),
        crate::style::Orientation::Vertical => element
            .border_t(px(if position.first { border } else { 0.0 }))
            .border_b(px(if position.last { border } else { 0.0 }))
            .when(position.first, |this| this.rounded_t(px(radius)))
            .when(position.last, |this| this.rounded_b(px(radius))),
    }
}

/// The icon slot shares its size with the control's resolved size/role.
pub(crate) fn icon(
    style: ControlStyle,
    path: impl Into<gpui::SharedString>,
    color: gpui::Hsla,
    scale: f32,
) -> gpui::Svg {
    gpui::svg()
        .path(path)
        .size(px(style.icon_size_px * scale))
        .text_color(color)
}

pub(crate) fn toggle_group(style: crate::style::ToggleGroupStyle, scale: f32) -> Div {
    let mut group = div()
        .flex()
        .when_some(style.tray, |this, tray| {
            this.p(px(tray.padding_px * scale))
                .rounded(px(tray.radius_px * scale))
                .bg(tray.background)
        })
        .gap(px(style.gap_px * scale))
        .when(
            style.orientation == crate::style::Orientation::Horizontal,
            |this| this.flex_row(),
        )
        .when(
            style.orientation == crate::style::Orientation::Vertical,
            |this| this.flex_col(),
        );
    group.style().align_items = Some(match style.alignment {
        crate::style::GroupAlignment::Start => gpui::AlignItems::FlexStart,
        crate::style::GroupAlignment::Center => gpui::AlignItems::Center,
        crate::style::GroupAlignment::End => gpui::AlignItems::FlexEnd,
        crate::style::GroupAlignment::Stretch => gpui::AlignItems::Stretch,
    });
    group
}

pub(crate) fn compact_action(
    style: crate::style::CompactActionStyle,
    state: ControlState,
    scale: f32,
) -> Div {
    button_frame(style.control, state, scale)
        .h_auto()
        .py(px(style.padding_y_px * scale))
}
