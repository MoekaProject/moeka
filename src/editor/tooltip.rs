use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, AnyTooltip, App, AppContext, Bounds, DispatchPhase, Element, ElementId, Entity,
    FontWeight, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseMoveEvent, Pixels, Render, SharedString, TextRun, Window, div, point, prelude::*, px,
    size,
};

use super::{
    EditorView,
    shortcut::{ShortcutRenderStyle, estimate_shortcut_width, render_shortcut},
};

pub(crate) fn with_tooltip(
    child: impl IntoElement,
    editor: Entity<EditorView>,
    title: impl Into<SharedString>,
    shortcut: Option<impl Into<SharedString>>,
) -> AnyElement {
    TooltipTrigger {
        child: child.into_any_element(),
        editor,
        title: TooltipTitle {
            text: title.into(),
            bold_first_line: false,
        },
        shortcut: shortcut.map(Into::into),
        prefer_above: false,
    }
    .into_any_element()
}

pub(crate) fn with_tab_tooltip(
    child: impl IntoElement,
    editor: Entity<EditorView>,
    title: impl Into<SharedString>,
) -> AnyElement {
    TooltipTrigger {
        child: child.into_any_element(),
        editor,
        title: TooltipTitle {
            text: title.into(),
            bold_first_line: true,
        },
        shortcut: None,
        prefer_above: false,
    }
    .into_any_element()
}

#[derive(Clone)]
struct TooltipTitle {
    text: SharedString,
    bold_first_line: bool,
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
        TooltipTitle {
            text: title.into(),
            bold_first_line: false,
        },
        shortcut.map(Into::into),
        false,
    );
}

fn request_positioned_tooltip(
    window: &mut Window,
    cx: &mut App,
    editor: Entity<EditorView>,
    bounds: Bounds<Pixels>,
    title: TooltipTitle,
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
        window,
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
        window,
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
    title: TooltipTitle,
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
    title: TooltipTitle,
    shortcut: Option<SharedString>,
    width: f32,
}

impl Render for TooltipView {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
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

        crate::ui::surface::surface(&editor.styles.tooltip_surface(), editor.ui_scale)
            .w(px(self.width))
            .px(px(editor.scaled(styles.tooltip_padding_x_px)))
            .py(px(editor.scaled(styles.tooltip_padding_y_px)))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(editor.scaled(styles.tooltip_gap_px)))
            .font_family(SharedString::from(editor.styles.ui_font_family))
            .line_height(px(
                title_font_size * editor.styles.ui.spec.tooltip_line_height_factor
            ))
            .child(
                render_tooltip_text(
                    editor,
                    window,
                    &self.title.text,
                    self.title.bold_first_line,
                    self.width
                        - editor.scaled(styles.tooltip_padding_x_px) * 2.0
                        - editor.scaled(styles.tooltip_border_width_px) * 2.0
                        - self
                            .shortcut
                            .as_ref()
                            .map(|shortcut| {
                                (estimate_shortcut_width(shortcut, shortcut_style)
                                    + editor.scaled(styles.tooltip_shortcut_key_padding_x_px) * 2.0)
                                    .max(editor.scaled(styles.tooltip_shortcut_key_min_width_px))
                                    + editor.scaled(styles.tooltip_gap_px)
                            })
                            .unwrap_or(0.0),
                )
                .flex_1(),
            )
            .when_some(self.shortcut.as_ref(), |this, shortcut| {
                this.child(
                    crate::ui::typography::badge(editor.styles.tooltip_keycap(), editor.ui_scale)
                        .min_w(px(editor.scaled(styles.tooltip_shortcut_key_min_width_px)))
                        .flex_none()
                        .h(px(editor.scaled(styles.tooltip_shortcut_key_height_px)))
                        .child(render_shortcut(shortcut, shortcut_style)),
                )
            })
    }
}

pub(super) fn render_tooltip_text(
    editor: &EditorView,
    window: &Window,
    text: &str,
    bold_first_line: bool,
    available: f32,
) -> gpui::Div {
    let title = TooltipTitle {
        text: text.to_string().into(),
        bold_first_line,
    };
    let styles = &editor.styles.tooltip;
    div()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .text_size(px(editor.scaled(styles.tooltip_title_font_size_px)))
        .text_color(styles.tooltip_title_text_color)
        .children(text.split('\n').enumerate().map(|(index, line)| {
            let width =
                shaped_tooltip_line(editor, window, line, tooltip_font(editor, &title, index))
                    .width();
            div()
                .debug_selector(move || format!("tooltip-line-{index}"))
                .when(f32::from(width) <= available, |this| {
                    this.whitespace_nowrap()
                })
                .when(f32::from(width) > available, |this| {
                    this.whitespace_normal()
                })
                .when(bold_first_line && index == 0, |this| {
                    this.font_weight(FontWeight::BOLD)
                })
                .child(line.to_string())
        }))
}

fn tooltip_font(editor: &EditorView, title: &TooltipTitle, index: usize) -> gpui::Font {
    let mut font = gpui::font(editor.styles.ui_font_family);
    if title.bold_first_line && index == 0 {
        font.weight = FontWeight::BOLD;
    }
    font
}

fn shaped_tooltip_line(
    editor: &EditorView,
    window: &Window,
    line: &str,
    font: gpui::Font,
) -> gpui::WrappedLine {
    window
        .text_system()
        .shape_text(
            line.to_string().into(),
            px(editor.scaled(editor.styles.tooltip.tooltip_title_font_size_px)),
            &[TextRun {
                len: line.len(),
                font,
                color: editor.styles.tooltip.tooltip_title_text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
            None,
        )
        .expect("tooltip shaping uses one complete font run")
        .into_iter()
        .next()
        .unwrap_or_default()
}

fn tooltip_size(
    editor: &EditorView,
    window: &Window,
    title: &TooltipTitle,
    shortcut: Option<&str>,
) -> (f32, f32) {
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
        .map(|width| width.max(editor.scaled(styles.tooltip_shortcut_key_min_width_px)))
        .unwrap_or(0.0);
    let gap = if shortcut_width > 0.0 {
        editor.scaled(styles.tooltip_gap_px)
    } else {
        0.0
    };
    let lines = title
        .text
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            shaped_tooltip_line(editor, window, line, tooltip_font(editor, title, index))
        })
        .collect::<Vec<_>>();
    let text_width = lines
        .iter()
        .map(|line| f32::from(line.width()))
        .fold(0.0, f32::max)
        .ceil()
        + editor.scaled(1.0);
    let padding_x = editor.scaled(styles.tooltip_padding_x_px);
    let border_width = editor.scaled(styles.tooltip_border_width_px) * 2.0;
    let width = text_width + shortcut_width + gap + padding_x * 2.0 + border_width;
    // Long text (file paths) wraps instead of stretching the tooltip past the
    // viewport; the estimate must count the wrapped lines or an above-anchor
    // tooltip would overlap its button.
    let chrome_width = shortcut_width + gap + padding_x * 2.0 + border_width;
    let max_width = editor
        .scaled(styles.tooltip_max_width_px)
        .max(chrome_width + title_font_size * 6.0);
    let width = width.min(max_width);
    let available_text_width = (width - chrome_width).max(1.0);
    let title_lines = lines
        .into_iter()
        .map(|mut line| {
            line.rewrap(px(available_text_width));
            (line.wrap_boundaries.len() + 1) as f32
        })
        .sum::<f32>();
    // Match TooltipView's explicit line height so the height estimate is
    // exact; an underestimate makes above-anchor tooltips overlap the button.
    let content_height = (title_font_size
        * editor.styles.ui.spec.tooltip_line_height_factor
        * title_lines)
        .max(if shortcut_width > 0.0 {
            editor.scaled(styles.tooltip_shortcut_key_height_px)
        } else {
            0.0
        });
    let height = content_height + editor.scaled(styles.tooltip_padding_y_px) * 2.0 + border_width;
    (width, height)
}

fn tooltip_deployment_point(
    editor: &EditorView,
    window: &Window,
    anchor: Bounds<Pixels>,
    title: &TooltipTitle,
    shortcut: Option<&str>,
    prefer_above: bool,
) -> gpui::Point<Pixels> {
    let styles = &editor.styles.tooltip;
    let (width, height) = tooltip_size(editor, window, title, shortcut);
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

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn tooltip_long_paths_wrap_without_wrapping_the_tab_title(cx: &mut gpui::TestAppContext) {
        use super::*;
        use gpui::AppContext;
        let cx = cx.add_empty_window();
        let editor = cx.update(|_, cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false))
        });
        let title = TooltipTitle {
            text: format!("Title\n/{}note.md", "long-directory/".repeat(20)).into(),
            bold_first_line: true,
        };
        cx.draw(
            point(px(0.0), px(0.0)),
            size(px(800.0), px(800.0)),
            |window, cx| {
                let (width, _) = tooltip_size(editor.read(cx), window, &title, None);
                let editor_state = editor.read(cx);
                assert!(
                    width <= editor_state.scaled(editor_state.styles.tooltip.tooltip_max_width_px)
                );
                cx.new(|_| TooltipView {
                    editor: editor.clone(),
                    title,
                    shortcut: None,
                    width,
                })
            },
        );
        let title = cx.debug_bounds("tooltip-line-0").unwrap();
        let path = cx.debug_bounds("tooltip-line-1").unwrap();
        assert!(path.size.height > title.size.height * 2.0);
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn tooltip_short_text_does_not_wrap_in_the_rendered_layout(cx: &mut gpui::TestAppContext) {
        use super::*;
        use gpui::AppContext;
        let cx = cx.add_empty_window();
        let editor = cx.update(|_, cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false))
        });
        for scale in [1.0, 1.5, 2.0] {
            cx.update(|_, cx| editor.update(cx, |editor, _| editor.ui_scale = scale));
            for (text, bold, shortcut) in [
                ("内容大纲", false, Some("cmd-shift-o")),
                ("WWW wide title", false, None),
                ("Title\n/path/to/note.md", true, None),
                ("中文标题\n/note.md", true, None),
            ] {
                let title = TooltipTitle {
                    text: text.into(),
                    bold_first_line: bold,
                };
                let line_height = cx.update(|window, cx| {
                    let editor = editor.read(cx);
                    if bold {
                        assert_eq!(tooltip_font(editor, &title, 0).weight, FontWeight::BOLD);
                        assert_eq!(tooltip_font(editor, &title, 1).weight, FontWeight::NORMAL);
                    }
                    let (_, height) = tooltip_size(editor, window, &title, shortcut);
                    assert!(height > 0.0);
                    editor.scaled(editor.styles.tooltip.tooltip_title_font_size_px)
                        * editor.styles.ui.spec.tooltip_line_height_factor
                });
                cx.draw(
                    point(px(0.0), px(0.0)),
                    size(px(800.0), px(400.0)),
                    |window, cx| {
                        let (width, _) = tooltip_size(editor.read(cx), window, &title, shortcut);
                        cx.new(|_| TooltipView {
                            editor: editor.clone(),
                            title,
                            shortcut: shortcut.map(Into::into),
                            width,
                        })
                    },
                );
                for index in 0..text.split('\n').count() {
                    let bounds = cx
                        .debug_bounds(if index == 0 {
                            "tooltip-line-0"
                        } else {
                            "tooltip-line-1"
                        })
                        .expect("rendered tooltip title line");
                    assert!(
                        (f32::from(bounds.size.height) - line_height).abs() < 1.0,
                        "{text:?} at scale {scale}: line height {:?}, expected {line_height}",
                        bounds.size.height
                    );
                }
            }
        }
    }

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
