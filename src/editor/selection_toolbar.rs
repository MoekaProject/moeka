//! External line-formatting chrome driven by the line trigger's paint geometry.
//!
//! Only the measured bar and its trigger are interactive. Build the bar in
//! deferred prepaint, after EditorElement publishes the current line geometry,
//! and clip it explicitly because deferred draws lose ancestor masks.
//! The bar subtree must not itself contain a deferred element: GPUI disallows
//! enqueueing deferred draws while processing the deferred prepaint queue.
//! The retired floating selection toolbar must not be restored.

pub(crate) use crate::ui::self_drawn::LineFormatToolbarGeometry;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, Entity,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, MouseMoveEvent, Pixels, Position,
    Style, Window, px, relative, size,
};

use super::{EditorView, PaneId};

#[cfg(test)]
pub(super) const FORMAT_TRIGGER_HOVER_OPACITY: f32 = 0.9;

impl EditorView {
    pub(super) fn sync_line_control_style_metrics(&mut self) {
        self.format_trigger_animation_epoch = self.format_trigger_animation_epoch.wrapping_add(1);
        self.line_format_toolbar_animation_epoch =
            self.line_format_toolbar_animation_epoch.wrapping_add(1);
        self.format_trigger_animation_task = None;
        self.line_format_toolbar_animation_task = None;
        self.format_trigger_opacity = self.desired_format_trigger_opacity();
        self.format_trigger_opacity_target = self.format_trigger_opacity;
        self.line_format_toolbar_opacity = self.desired_line_format_toolbar_opacity();
        self.line_format_toolbar_opacity_target = self.line_format_toolbar_opacity;
    }

    fn desired_format_trigger_opacity(&self) -> f32 {
        if self.hovering_format_trigger_line.is_some()
            || self.line_format_toolbar_hovered
            || self.format_toolbar_owns_hover_menu()
        {
            self.styles.self_drawn.format_trigger_hover_opacity
        } else if self.hovering_format_line.is_some() {
            self.styles.self_drawn.format_trigger_line_opacity
        } else {
            0.0
        }
    }

    fn sync_format_trigger_opacity(&mut self, cx: &mut gpui::Context<Self>) {
        let target = self.desired_format_trigger_opacity();
        if (self.format_trigger_opacity_target - target).abs() <= 0.001 {
            return;
        }
        self.format_trigger_opacity_target = target;
        self.format_trigger_animation_epoch = self.format_trigger_animation_epoch.wrapping_add(1);
        let epoch = self.format_trigger_animation_epoch;
        let pane_id = self.current_pane_id();
        let frame_ms = self.styles.self_drawn.format_trigger_animation_frame_ms;
        self.format_trigger_animation_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.format_trigger_animation_epoch != epoch {
                                return true;
                            }
                            let difference =
                                this.format_trigger_opacity_target - this.format_trigger_opacity;
                            if difference.abs() <= 0.01 {
                                this.format_trigger_opacity = this.format_trigger_opacity_target;
                                this.format_trigger_animation_task = None;
                                if this.format_trigger_opacity <= 0.0 {
                                    this.format_trigger_line = None;
                                }
                                cx.notify();
                                return true;
                            }
                            this.format_trigger_opacity +=
                                difference * this.styles.self_drawn.format_trigger_animation_step;
                            cx.notify();
                            false
                        })
                        .unwrap_or(true)
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(frame_ms))
                    .await;
            }
        }));
    }

    fn desired_line_format_toolbar_opacity(&self) -> f32 {
        if self.line_format_toolbar_line.is_some()
            && (self.hovering_format_trigger_line.is_some()
                || self.line_format_toolbar_hovered
                || self.format_toolbar_owns_hover_menu())
        {
            1.0
        } else {
            0.0
        }
    }

    fn sync_line_format_toolbar_opacity(&mut self, cx: &mut gpui::Context<Self>) {
        let target = self.desired_line_format_toolbar_opacity();
        if (self.line_format_toolbar_opacity_target - target).abs() <= 0.001 {
            return;
        }
        self.line_format_toolbar_opacity_target = target;
        self.line_format_toolbar_animation_epoch =
            self.line_format_toolbar_animation_epoch.wrapping_add(1);
        let epoch = self.line_format_toolbar_animation_epoch;
        let pane_id = self.current_pane_id();
        let frame_ms = self.styles.self_drawn.format_trigger_animation_frame_ms;
        self.line_format_toolbar_animation_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.line_format_toolbar_animation_epoch != epoch {
                                return true;
                            }
                            let difference = this.line_format_toolbar_opacity_target
                                - this.line_format_toolbar_opacity;
                            if difference.abs() <= 0.01 {
                                this.line_format_toolbar_opacity =
                                    this.line_format_toolbar_opacity_target;
                                this.line_format_toolbar_animation_task = None;
                                if this.line_format_toolbar_opacity <= 0.0 {
                                    this.line_format_toolbar_line = None;
                                    this.line_format_toolbar_bounds = None;
                                }
                                cx.notify();
                                return true;
                            }
                            this.line_format_toolbar_opacity +=
                                difference * this.styles.self_drawn.format_trigger_animation_step;
                            cx.notify();
                            false
                        })
                        .unwrap_or(true)
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(frame_ms))
                    .await;
            }
        }));
    }

    pub(crate) fn update_line_format_hover(
        &mut self,
        line: Option<usize>,
        trigger_line: Option<usize>,
        cx: &mut gpui::Context<Self>,
    ) {
        if let (Some(active_line), Some(next_line)) = (self.line_format_toolbar_line, trigger_line)
            && active_line != next_line
        {
            return;
        }
        let changed =
            self.hovering_format_line != line || self.hovering_format_trigger_line != trigger_line;
        self.hovering_format_line = line;
        self.hovering_format_trigger_line = trigger_line;
        if let Some(line) = line {
            self.format_trigger_line = Some(line);
        }
        self.sync_format_trigger_opacity(cx);
        self.sync_line_format_toolbar_opacity(cx);
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn toggle_line_format_toolbar(&mut self, line: usize, cx: &mut gpui::Context<Self>) {
        if self.line_format_toolbar_line == Some(line) {
            self.dismiss_line_format_toolbar(cx);
            return;
        }
        self.hovering_format_line = Some(line);
        self.hovering_format_trigger_line = Some(line);
        self.format_trigger_line = Some(line);
        self.line_format_toolbar_line = Some(line);
        self.line_format_toolbar_hovered = false;
        self.sync_format_trigger_opacity(cx);
        self.sync_line_format_toolbar_opacity(cx);
        cx.notify();
    }

    pub(crate) fn set_line_format_toolbar_hovered(
        &mut self,
        hovered: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        self.line_format_toolbar_hovered = hovered;
        if hovered {
            self.hovering_format_trigger_line = None;
        }
        self.sync_format_trigger_opacity(cx);
        self.sync_line_format_toolbar_opacity(cx);
    }

    pub(crate) fn clear_line_format_hover(&mut self, cx: &mut gpui::Context<Self>) {
        self.update_line_format_hover(None, None, cx);
    }

    pub(crate) fn dismiss_line_format_toolbar(&mut self, cx: &mut gpui::Context<Self>) {
        self.hovering_format_line = None;
        self.hovering_format_trigger_line = None;
        self.format_trigger_line = None;
        self.line_format_toolbar_line = None;
        self.line_format_toolbar_bounds = None;
        self.line_format_toolbar_hovered = false;
        self.line_format_toolbar_opacity = 0.0;
        self.line_format_toolbar_opacity_target = 0.0;
        self.line_format_toolbar_animation_epoch =
            self.line_format_toolbar_animation_epoch.wrapping_add(1);
        self.line_format_toolbar_animation_task = None;
        self.sync_format_trigger_opacity(cx);
        cx.notify();
    }

    pub(crate) fn hide_line_format_toolbar_for_scroll(&mut self, cx: &mut gpui::Context<Self>) {
        if self.embedded_editor || !self.active_tab_is_markdown() {
            return;
        }
        self.dismiss_line_format_toolbar(cx);
    }
}

pub(crate) struct LineFormatToolbarElement {
    pub(crate) editor: Entity<EditorView>,
    pub(crate) pane_id: PaneId,
}

pub(crate) struct LineFormatToolbarPrepaintState {
    toolbar: AnyElement,
    viewport: Bounds<Pixels>,
    toolbar_hitbox: gpui::Hitbox,
    trigger_hitbox: gpui::Hitbox,
    interaction_bounds: Bounds<Pixels>,
}

impl IntoElement for LineFormatToolbarElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for LineFormatToolbarElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<LineFormatToolbarPrepaintState>;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name(
            format!("line-format-toolbar-{}", self.pane_id).into(),
        ))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.inset.left = px(0.0).into();
        style.inset.top = px(0.0).into();
        style.size = size(relative(1.0).into(), relative(1.0).into());
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (mut toolbar, geometry, margin, gap) = self.editor.update(cx, |editor, cx| {
            editor.line_format_toolbar_bounds = None;
            if editor.focused_pane_id != self.pane_id
                || (editor.cursor.has_selection() && editor.line_format_toolbar_line.is_none())
                || !editor.active_tab_is_markdown()
                || editor.active_tab_is_read_only()
            {
                return None;
            }
            let line = editor.line_format_toolbar_line?;
            let geometry = editor
                .line_format_toolbar_geometries
                .iter()
                .find(|geometry| geometry.line == line)
                .cloned()?;
            let margin =
                px(editor.scaled(editor.styles.self_drawn.line_toolbar_viewport_margin_px));
            let gap = px(editor.scaled(editor.styles.self_drawn.line_toolbar_trigger_gap_px));
            let available_width = geometry.viewport.size.width - margin * 2.0;
            if available_width <= px(0.0) {
                return None;
            }
            let opacity = editor.line_format_toolbar_opacity.clamp(0.0, 1.0);
            let toolbar = editor.render_format_toolbar(
                f32::from(available_width),
                opacity,
                true,
                true,
                Some(line),
                cx,
            );
            Some((toolbar, geometry, margin, gap))
        })?;
        let measured = toolbar.layout_as_root(
            size(
                AvailableSpace::Definite(geometry.viewport.size.width - margin * 2.0),
                AvailableSpace::MinContent,
            ),
            window,
            cx,
        );
        let origin = geometry.toolbar_origin(measured, margin, gap)?;
        let toolbar_bounds = Bounds::new(origin, measured);
        let interaction_bounds = toolbar_bounds.union(&geometry.trigger_bounds);
        let toolbar_hitbox =
            window.insert_hitbox(toolbar_bounds, gpui::HitboxBehavior::BlockMouseExceptScroll);
        let trigger_hitbox = window.insert_hitbox(
            geometry.trigger_bounds,
            gpui::HitboxBehavior::BlockMouseExceptScroll,
        );
        window.with_content_mask(
            Some(ContentMask {
                bounds: geometry.viewport,
            }),
            |window| {
                toolbar.prepaint_at(origin, window, cx);
            },
        );
        self.editor.update(cx, |editor, _| {
            editor.line_format_toolbar_bounds = Some(toolbar_bounds);
        });
        Some(LineFormatToolbarPrepaintState {
            toolbar,
            viewport: geometry.viewport,
            toolbar_hitbox,
            trigger_hitbox,
            interaction_bounds,
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(state) = prepaint {
            window.set_cursor_style(gpui::CursorStyle::Arrow, &state.toolbar_hitbox);
            window.set_cursor_style(gpui::CursorStyle::OpenHand, &state.trigger_hitbox);
            let trigger_hitbox = state.trigger_hitbox.clone();
            let editor = self.editor.clone();
            let pane_id = self.pane_id;
            window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
                if !phase.capture()
                    || event.button != gpui::MouseButton::Left
                    || !trigger_hitbox.is_hovered(window)
                {
                    return;
                }
                editor.update(cx, |this, cx| {
                    this.focus_pane(pane_id, cx);
                    if let Some(line) = this.line_format_toolbar_line {
                        window.focus(&this.focus_handle);
                        this.begin_line_drag(line, event.position, cx);
                    }
                });
                window.prevent_default();
                cx.stop_propagation();
            });
            let interaction_bounds = state.interaction_bounds;
            let editor = self.editor.clone();
            let pane_id = self.pane_id;
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Capture {
                    return;
                }
                let hovered = interaction_bounds.contains(&window.mouse_position());
                editor.update(cx, |this, cx| {
                    this.with_pane(pane_id, |this| {
                        this.set_line_format_toolbar_hovered(hovered, cx);
                    });
                });
            });
            window.with_content_mask(
                Some(ContentMask {
                    bounds: state.viewport,
                }),
                |window| {
                    state.toolbar.paint(window, cx);
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn line_toolbar_aligns_with_content_and_prefers_above() {
        let geometry = LineFormatToolbarGeometry {
            line: 4,
            viewport: rect(100.0, 50.0, 500.0, 300.0),
            content_left: px(180.0),
            trigger_bounds: rect(180.0, 180.0, 18.0, 22.0),
        };
        assert_eq!(
            geometry.toolbar_origin(size(px(280.0), px(52.0)), px(6.0), px(4.0),),
            Some(point(px(180.0), px(124.0)))
        );

        let near_top = LineFormatToolbarGeometry {
            trigger_bounds: rect(180.0, 60.0, 18.0, 22.0),
            ..geometry
        };
        assert_eq!(
            near_top.toolbar_origin(size(px(280.0), px(52.0)), px(6.0), px(4.0),),
            Some(point(px(180.0), px(86.0)))
        );
    }

    #[test]
    fn line_toolbar_below_stays_attached_to_trigger_on_tall_lines() {
        let geometry = LineFormatToolbarGeometry {
            line: 4,
            viewport: rect(100.0, 50.0, 500.0, 300.0),
            content_left: px(180.0),
            trigger_bounds: rect(180.0, 60.0, 18.0, 22.0),
        };
        assert_eq!(
            geometry.toolbar_origin(size(px(280.0), px(52.0)), px(6.0), px(4.0)),
            Some(point(px(180.0), px(86.0)))
        );
    }

    #[test]
    fn line_toolbar_clamps_to_clipped_viewport_and_accepts_exact_fit() {
        let mut geometry = LineFormatToolbarGeometry {
            line: 4,
            viewport: rect(100.0, 50.0, 180.0, 300.0),
            content_left: px(120.0),
            trigger_bounds: rect(120.0, 50.0, 18.0, 22.0),
        };
        assert_eq!(
            geometry.toolbar_origin(size(px(156.0), px(48.0)), px(12.0), px(0.0)),
            Some(point(px(112.0), px(72.0)))
        );
        assert!(
            geometry
                .toolbar_origin(size(px(157.0), px(48.0)), px(12.0), px(0.0))
                .is_none()
        );
        geometry.content_left = px(50.0);
        assert_eq!(
            geometry.toolbar_origin(size(px(100.0), px(48.0)), px(12.0), px(0.0)),
            Some(point(px(112.0), px(72.0)))
        );
        geometry.content_left = px(270.0);
        assert_eq!(
            geometry.toolbar_origin(size(px(100.0), px(48.0)), px(12.0), px(0.0)),
            Some(point(px(168.0), px(72.0)))
        );
    }

    #[test]
    fn line_toolbar_hides_when_neither_side_fits_or_size_is_invalid() {
        let geometry = LineFormatToolbarGeometry {
            line: 4,
            viewport: rect(100.0, 50.0, 180.0, 100.0),
            content_left: px(120.0),
            trigger_bounds: rect(120.0, 90.0, 18.0, 22.0),
        };
        for measured in [
            size(px(156.0), px(48.0)),
            size(px(156.0), px(101.0)),
            size(px(0.0), px(20.0)),
            size(px(100.0), px(0.0)),
            size(px(-1.0), px(20.0)),
            size(px(f32::NAN), px(20.0)),
            size(px(100.0), px(f32::INFINITY)),
        ] {
            assert!(
                geometry
                    .toolbar_origin(measured, px(6.0), px(0.0))
                    .is_none()
            );
        }
    }
}
