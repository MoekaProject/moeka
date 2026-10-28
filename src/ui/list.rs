use crate::style::{ControlState, ListItemStyle};
use gpui::{Div, div, prelude::*, px};

/// Used by context menus, command results and searchable pickers. Returning a
/// Div lets each owner keep its existing ID and mouse-down/hover semantics.
pub(crate) fn list_item(style: ListItemStyle, state: ControlState, scale: f32) -> Div {
    let colors = style.control.resolve(state);
    let hover = style.control.resolve(ControlState {
        hovered: true,
        ..state
    });
    div()
        .w_full()
        .when(style.inset_x_px > 0.0, |this| {
            this.w_auto().mx(px(style.inset_x_px * scale))
        })
        .when(
            style.presentation.orientation == crate::style::Orientation::Vertical,
            |this| this.flex_col(),
        )
        .when(
            style.presentation.variant == crate::style::ContainerVariant::Outline,
            |this| {
                this.border(px(style.control.border_width_px * scale))
                    .border_color(style.control.ring.color)
            },
        )
        .when(
            style.presentation.orientation == crate::style::Orientation::Horizontal,
            |this| this.h(px(style.control.height_px * scale)),
        )
        .when(
            style.presentation.orientation == crate::style::Orientation::Vertical,
            |this| this.min_h(px(style.control.height_px * scale)),
        )
        .px(px(style.control.padding_x_px * scale))
        .flex()
        .items_center()
        .gap(px(style.control.gap_px * scale))
        .text_size(px(style.control.font_size_px * scale))
        .text_color(colors.foreground)
        .rounded(px(style.control.radius_px * scale))
        .bg(
            if style.presentation.variant == crate::style::ContainerVariant::Muted
                && !state.selected
                && !state.checked
            {
                style.control.selected.background
            } else {
                colors.background
            },
        )
        .opacity(colors.opacity)
        .cursor(style.control.cursor)
        .when(
            !state.disabled && !state.loading && !state.read_only,
            |this| this.hover(move |s| s.bg(hover.background).text_color(hover.foreground)),
        )
        .when(
            state.disabled || state.loading || state.read_only,
            super::interaction::inert_activation,
        )
}

pub(crate) fn empty(style: crate::style::EmptyStyle, scale: f32) -> Div {
    div()
        .w_full()
        .when(
            style.variant == crate::style::ContainerVariant::Outline,
            |this| {
                this.border(px(style.border_width_px * scale))
                    .border_color(style.border)
                    .rounded(px(style.radius_px * scale))
            },
        )
        .when(
            style.variant == crate::style::ContainerVariant::Muted,
            |this| {
                this.bg(style.background)
                    .rounded(px(style.radius_px * scale))
            },
        )
        .h(px(style.height_px * scale))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(style.text.size_px * scale))
        .text_color(style.text.color)
}

pub(crate) fn tree_row(
    style: crate::style::TreeRowStyle,
    state: crate::style::TreeRowState,
    scale: f32,
) -> Div {
    const GROUP: &str = "tree-row";
    let inset = style.selected_inset_px > 0.0;
    let inset_surface = || {
        div()
            .absolute()
            .left(px(style.selected_inset_px * scale))
            .right(px(style.selected_inset_px * scale))
            .top(px(1.0 * scale))
            .bottom(px(1.0 * scale))
            .rounded(px(style.selected_radius_px * scale))
    };
    div()
        .w_full()
        .h(px(style.height_px * scale))
        .flex_none()
        .relative()
        .flex()
        .items_center()
        .opacity(if state.cut { style.cut_opacity } else { 1.0 })
        .bg(
            if (state.selected && style.selected_inset_px == 0.0) || state.drag_over {
                style.selected_background
            } else {
                gpui::transparent_black()
            },
        )
        .border(px(style.border_width_px))
        .border_color(if state.selected && style.selected_inset_px == 0.0 {
            style.active_border
        } else if state.focused && !state.selected {
            style.focus_border
        } else {
            gpui::transparent_black()
        })
        .when(state.selected && inset, |this| {
            this.child(
                inset_surface()
                    .bg(style.selected_background)
                    .border(px(style.border_width_px))
                    .border_color(style.active_border)
                    .when_some(style.selected_elevation, |this, elevation| {
                        this.shadow(elevation.shadows(scale))
                    }),
            )
        })
        // Inset rows hover with the same rounded, raised surface as selection.
        .when(inset && !state.selected && !state.resizing, |this| {
            let shadows = style
                .selected_elevation
                .map(|elevation| elevation.shadows(scale));
            this.group(GROUP)
                .child(inset_surface().group_hover(GROUP, move |s| {
                    let s = s.bg(style.hover_background);
                    match shadows {
                        Some(shadows) => s.shadow(shadows),
                        None => s,
                    }
                }))
        })
        .when(!inset && !state.resizing, |this| {
            this.hover(move |s| {
                if state.selected {
                    s
                } else {
                    s.bg(style.hover_background)
                }
            })
        })
}

pub(crate) fn file_row(
    style: crate::style::FileRowStyle,
    height: f32,
    selected: bool,
    resizing: bool,
    scale: f32,
) -> Div {
    div()
        .w_full()
        .h(px(height))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(style.gap_px * scale))
        .px(px(style.padding_x_px * scale))
        .py(px(style.padding_y_px * scale))
        .border_b(px(style.separator_width_px))
        .border_dashed()
        .border_color(style.separator_color)
        .bg(if selected {
            style.selected_background
        } else {
            style.background
        })
        .cursor(gpui::CursorStyle::Arrow)
        .when(!resizing, |this| {
            this.hover(move |s| s.bg(style.hover_background))
        })
}

/// Cache-owned row height and indentation are already scaled: paint and hit
/// testing continue to use the same measured outline geometry.
pub(crate) fn outline_row(
    style: crate::style::OutlineRowStyle,
    height: f32,
    indent: f32,
    active: bool,
    scale: f32,
) -> Div {
    super::typography::label(style.text, scale)
        .w_full()
        .max_w_full()
        .overflow_hidden()
        .min_h(px(height))
        .flex_shrink_0()
        .relative()
        .pl(px(indent))
        .pr(px(style.padding_right_px * scale))
        .py(px(style.padding_y_px * scale))
        .flex()
        .items_center()
        .gap(px(style.gap_px * scale))
        .cursor(gpui::CursorStyle::Arrow)
        .when(active, |this| this.bg(style.active_background))
        .hover(move |s| s.bg(style.active_background))
}

pub(crate) fn selection_grid_cell(
    style: crate::style::SelectionGridStyle,
    current: bool,
    hovered: bool,
    scale: f32,
) -> Div {
    div()
        .size(px(style.cell_size_px * scale))
        .border(px(style.border_width_px * scale))
        .border_color(if current || hovered {
            style.active_border
        } else {
            style.border
        })
        .bg(match (current, hovered) {
            (true, true) => style.overlap,
            (true, false) => style.current,
            (false, true) => style.hover,
            (false, false) => gpui::transparent_black(),
        })
        .cursor(gpui::CursorStyle::Arrow)
}
