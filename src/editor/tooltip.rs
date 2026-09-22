use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, AnyTooltip, App, AppContext, Bounds, DispatchPhase, Element, ElementId, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseMoveEvent, Pixels, Render, SharedString, Window, div, point, prelude::*, px, size,
};

use super::{
    EditorView,
    shortcut::{
        ShortcutRenderStyle, estimate_shortcut_width, estimate_text_width, render_shortcut,
    },
};

/// Explicit tooltip text line-height factor, shared between the rendered view
/// and the size estimate so above-anchor placement never overlaps the button.
const TOOLTIP_LINE_HEIGHT_FACTOR: f32 = 1.3;

pub(crate) fn with_tooltip(
    child: impl IntoElement,
    editor: Entity<EditorView>,
    title: impl Into<SharedString>,
    shortcut: Option<impl Into<SharedString>>,
) -> AnyElement {
    TooltipTrigger {
        child: child.into_any_element(),
        editor,
        title: title.into(),
        shortcut: shortcut.map(Into::into),
        prefer_above: false,
    }
    .into_any_element()
}

/// Requests the tooltip when the pointer is inside `bounds`.
///
/// Must be called during the prepaint phase (`Window::set_tooltip` asserts
/// prepaint). Pair it with [`register_tooltip_hover_refresh`] during paint so
/// hover changes trigger a redraw.
pub(crate) fn request_tooltip_for_hitbox(
    window: &mut Window,
    cx: &mut App,
    editor: Entity<EditorView>,
    bounds: Bounds<Pixels>,
    title: impl Into<SharedString>,
    shortcut: Option<impl Into<SharedString>>,
) {
    request_positioned_tooltip(
        window,
        cx,
        editor,
        bounds,
        title.into(),
        shortcut.map(Into::into),
        false,
    );
}

fn request_positioned_tooltip(
    window: &mut Window,
    cx: &mut App,
    editor: Entity<EditorView>,
    bounds: Bounds<Pixels>,
    title: SharedString,
    shortcut: Option<SharedString>,
    prefer_above: bool,
) {
    if editor.read(cx).global_tooltips_suppressed() {
        return;
    }

    if !bounds.contains(&window.mouse_position()) {
        return;
    }

    let (tooltip_width, _) = tooltip_size(
        editor.read(cx),
        &title,
        shortcut.as_ref().map(|shortcut| shortcut.as_ref()),
    );
    let tooltip_view = cx.new(|_| TooltipView {
        editor: editor.clone(),
        title: title.clone(),
        shortcut: shortcut.clone(),
        width: tooltip_width,
    });
    let deployment = tooltip_deployment_point(
        editor.read(cx),
        bounds,
        &title,
        shortcut.as_ref().map(|shortcut| shortcut.as_ref()),
        prefer_above,
    );
    let visible_bounds = bounds;
    let visible_editor = editor.clone();
    window.set_tooltip(AnyTooltip {
        view: tooltip_view.into(),
        mouse_position: deployment - point(px(1.0), px(1.0)),
        check_visible_and_update: Rc::new(move |_, window, cx| {
            !visible_editor.read(cx).global_tooltips_suppressed()
                && visible_bounds.contains(&window.mouse_position())
        }),
    });
}

/// Registers a mouse-move listener that redraws when hover state changes.
///
/// Must be called during the paint phase (`Window::on_mouse_event` asserts
/// paint).
pub(crate) fn register_tooltip_hover_refresh(
    window: &mut Window,
    hitbox: &Hitbox,
    bounds: Bounds<Pixels>,
) {
    let hover_state = Rc::new(Cell::new(bounds.contains(&window.mouse_position())));
    let hitbox = hitbox.clone();
    window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, _cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let next_hovered = hitbox.is_hovered(window);
        if hover_state.replace(next_hovered) != next_hovered {
            window.refresh();
        }
    });
}

struct TooltipTrigger {
    child: AnyElement,
    editor: Entity<EditorView>,
    title: SharedString,
    shortcut: Option<SharedString>,
    prefer_above: bool,
}

impl IntoElement for TooltipTrigger {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TooltipTrigger {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        request_positioned_tooltip(
            window,
            cx,
            self.editor.clone(),
            bounds,
            self.title.clone(),
            self.shortcut.clone(),
            self.prefer_above,
        );
        hitbox
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        register_tooltip_hover_refresh(window, hitbox, bounds);
    }
}

struct TooltipView {
    editor: Entity<EditorView>,
    title: SharedString,
    shortcut: Option<SharedString>,
    width: f32,
}

impl Render for TooltipView {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let styles = &editor.styles.tooltip;
        let title_font_size = editor.scaled(styles.tooltip_title_font_size_px);
        let shortcut_font_size = editor.scaled(styles.tooltip_subtitle_font_size_px);
        let shortcut_style = ShortcutRenderStyle {
            font_size_px: shortcut_font_size,
            font_family: editor.styles.ui_font_family,
            text_color: styles.tooltip_subtitle_text_color,
            gap_px: editor.scaled(styles.tooltip_shortcut_gap_px),
        };

        div()
            .w(px(self.width))
            .px(px(editor.scaled(styles.tooltip_padding_x_px)))
            .py(px(editor.scaled(styles.tooltip_padding_y_px)))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(editor.scaled(styles.tooltip_gap_px)))
            .font_family(SharedString::from(editor.styles.ui_font_family))
            .line_height(px(title_font_size * TOOLTIP_LINE_HEIGHT_FACTOR))
            .bg(styles.tooltip_bg)
            .border(px(editor.scaled(styles.tooltip_border_width_px)))
            .border_color(styles.tooltip_border_color)
            .rounded(px(editor.scaled(styles.tooltip_radius_px)))
            .child(
                div()
                    .min_w(px(0.0))
                    .whitespace_normal()
                    .text_size(px(title_font_size))
                    .text_color(styles.tooltip_title_text_color)
                    .child(self.title.clone()),
            )
            .when_some(self.shortcut.as_ref(), |this, shortcut| {
                this.child(
                    div()
                        .min_w(px(editor.scaled(styles.tooltip_shortcut_key_min_width_px)))
                        .h(px(editor.scaled(styles.tooltip_shortcut_key_height_px)))
                        .px(px(editor.scaled(styles.tooltip_shortcut_key_padding_x_px)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(editor.scaled(styles.tooltip_shortcut_key_radius_px)))
                        .bg(styles.tooltip_shortcut_key_bg)
                        .child(render_shortcut(shortcut, shortcut_style)),
                )
            })
    }
}

fn tooltip_size(editor: &EditorView, title: &str, shortcut: Option<&str>) -> (f32, f32) {
    let styles = &editor.styles.tooltip;
    let title_font_size = editor.scaled(styles.tooltip_title_font_size_px);
    let shortcut_font_size = editor.scaled(styles.tooltip_subtitle_font_size_px);
    let shortcut_style = ShortcutRenderStyle {
        font_size_px: shortcut_font_size,
        font_family: editor.styles.ui_font_family,
        text_color: styles.tooltip_subtitle_text_color,
        gap_px: editor.scaled(styles.tooltip_shortcut_gap_px),
    };
    let shortcut_width = shortcut
        .map(|shortcut| {
            estimate_shortcut_width(shortcut, shortcut_style)
                + editor.scaled(styles.tooltip_shortcut_key_padding_x_px) * 2.0
        })
        .unwrap_or(0.0);
    let gap = if shortcut_width > 0.0 {
        editor.scaled(styles.tooltip_gap_px)
    } else {
        0.0
    };
    let width = estimate_text_width(title, title_font_size)
        + shortcut_width
        + gap
        + title_font_size
        + editor.scaled(styles.tooltip_padding_x_px) * 2.0;
    // Long text (file paths) wraps instead of stretching the tooltip past the
    // viewport; the estimate must count the wrapped lines or an above-anchor
    // tooltip would overlap its button.
    let chrome_width = width - estimate_text_width(title, title_font_size);
    let max_width = editor
        .scaled(styles.tooltip_max_width_px)
        .max(chrome_width + title_font_size * 6.0);
    let width = width.min(max_width);
    let available_text_width = (width - chrome_width).max(1.0);
    let title_lines = (estimate_text_width(title, title_font_size) / available_text_width)
        .ceil()
        .max(1.0);
    // Match TooltipView's explicit line height so the height estimate is
    // exact; an underestimate makes above-anchor tooltips overlap the button.
    let content_height =
        (title_font_size * TOOLTIP_LINE_HEIGHT_FACTOR * title_lines).max(if shortcut_width > 0.0 {
            editor.scaled(styles.tooltip_shortcut_key_height_px)
        } else {
            0.0
        });
    let height = content_height + editor.scaled(styles.tooltip_padding_y_px) * 2.0;
    (width, height)
}

fn tooltip_deployment_point(
    editor: &EditorView,
    anchor: Bounds<Pixels>,
    title: &str,
    shortcut: Option<&str>,
    prefer_above: bool,
) -> gpui::Point<Pixels> {
    let styles = &editor.styles.tooltip;
    let (width, height) = tooltip_size(editor, title, shortcut);
    let viewport_padding = editor.scaled(styles.tooltip_viewport_padding_px);
    let offset = editor.scaled(styles.tooltip_offset_px);
    let viewport_width = editor.last_window_width.max(width + viewport_padding * 2.0);
    let viewport_height = editor
        .last_window_height
        .max(height + viewport_padding * 2.0);
    let mut position = position_tooltip(
        anchor,
        width,
        height,
        viewport_width,
        viewport_height,
        viewport_padding,
        offset,
    );
    let above = f32::from(anchor.top()) - offset - height;
    if prefer_above && above >= viewport_padding {
        position.y = px(above);
    }
    position
}

fn position_tooltip(
    anchor: Bounds<Pixels>,
    width: f32,
    height: f32,
    viewport_width: f32,
    viewport_height: f32,
    viewport_padding: f32,
    offset: f32,
) -> gpui::Point<Pixels> {
    let anchor_left = f32::from(anchor.origin.x);
    let anchor_top = f32::from(anchor.origin.y);
    let anchor_width = f32::from(anchor.size.width);
    let anchor_height = f32::from(anchor.size.height);
    let left = (anchor_left + anchor_width / 2.0 - width / 2.0).clamp(
        viewport_padding,
        (viewport_width - width - viewport_padding).max(viewport_padding),
    );
    let below = anchor_top + anchor_height + offset;
    let above = anchor_top - offset - height;
    let top = if below + height <= viewport_height - viewport_padding {
        below
    } else {
        above.max(viewport_padding)
    };
    point(px(left), px(top))
}

pub(crate) fn toolbar_anchor_bounds(x: f32, y: f32, button_size: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(button_size), px(button_size)))
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::position_tooltip;

    #[test]
    fn positions_below_anchor_center_by_default() {
        let anchor = Bounds::new(point(px(100.0), px(50.0)), size(px(20.0), px(20.0)));
        let position = position_tooltip(anchor, 80.0, 30.0, 300.0, 200.0, 8.0, 5.0);

        assert_eq!(f32::from(position.x), 70.0);
        assert_eq!(f32::from(position.y), 75.0);
    }

    #[test]
    fn clamps_to_viewport_edges() {
        let left_anchor = Bounds::new(point(px(0.0), px(20.0)), size(px(16.0), px(16.0)));
        let right_anchor = Bounds::new(point(px(292.0), px(20.0)), size(px(8.0), px(16.0)));

        let left = position_tooltip(left_anchor, 100.0, 30.0, 300.0, 200.0, 8.0, 5.0);
        let right = position_tooltip(right_anchor, 100.0, 30.0, 300.0, 200.0, 8.0, 5.0);

        assert_eq!(f32::from(left.x), 8.0);
        assert_eq!(f32::from(right.x), 192.0);
    }

    #[test]
    fn flips_above_when_bottom_space_is_insufficient() {
        let anchor = Bounds::new(point(px(100.0), px(170.0)), size(px(20.0), px(20.0)));
        let position = position_tooltip(anchor, 80.0, 30.0, 300.0, 200.0, 8.0, 5.0);

        assert_eq!(f32::from(position.y), 135.0);
    }
}
