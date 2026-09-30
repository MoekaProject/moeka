use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, ContentMask, Context, Corners, CursorStyle, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, Window, canvas, div, fill,
    point, prelude::*, px, size,
};

use super::{EditorView, GeneratedPreviewAction, ImageDragState, helpers};

const PREVIEW_OVERLAY_PADDING_PX: f32 = 32.0;
const CHART_INITIAL_VIEW_FRACTION: f32 = 0.8;
const PREVIEW_CLICK_SLOP_PX: f32 = 4.0;

#[derive(Clone, Copy)]
pub(crate) enum PreviewOverlayPress {
    Action,
    Pan {
        start_position: Point<Pixels>,
        moved: bool,
    },
}

fn moved_beyond_click_slop(start: Point<Pixels>, position: Point<Pixels>) -> bool {
    let dx = f32::from(position.x - start.x);
    let dy = f32::from(position.y - start.y);
    dx * dx + dy * dy > PREVIEW_CLICK_SLOP_PX * PREVIEW_CLICK_SLOP_PX
}

impl PreviewOverlayPress {
    fn on_move(self, position: Point<Pixels>) -> Self {
        match self {
            Self::Pan {
                start_position,
                moved,
            } => Self::Pan {
                start_position,
                moved: moved || moved_beyond_click_slop(start_position, position),
            },
            action => action,
        }
    }
}

fn should_close_on_release(
    press: PreviewOverlayPress,
    position: Point<Pixels>,
    inside: bool,
) -> bool {
    matches!(press, PreviewOverlayPress::Pan { start_position, moved }
        if inside && !moved && !moved_beyond_click_slop(start_position, position))
}

pub(super) fn inner_size(viewport_width: f32, viewport_height: f32) -> (f32, f32) {
    (
        (viewport_width - PREVIEW_OVERLAY_PADDING_PX * 2.0).max(1.0),
        (viewport_height - PREVIEW_OVERLAY_PADDING_PX * 2.0).max(1.0),
    )
}

pub(super) fn fit_scale(
    width: f32,
    height: f32,
    viewport_width: f32,
    viewport_height: f32,
    is_chart: bool,
) -> f32 {
    let (available_width, available_height) = inner_size(viewport_width, viewport_height);
    let fraction = if is_chart {
        CHART_INITIAL_VIEW_FRACTION
    } else {
        1.0
    };
    let fit = (available_width * fraction / width.max(1.0))
        .min(available_height * fraction / height.max(1.0));
    if is_chart { fit } else { fit.min(1.0) }
}

pub(super) fn clamp_pan(pan: f32, display_size: f32, inner_size: f32) -> f32 {
    let limit = (display_size - inner_size).abs() / 2.0;
    pan.clamp(-limit, limit)
}

impl EditorView {
    pub(super) fn on_preview_overlay_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.stop_propagation();
        // The canvas and editor can both see the same press; only handle it once.
        if event.button != MouseButton::Left
            || self.preview_overlay.is_none()
            || self.preview_overlay_press.is_some()
        {
            return;
        }
        window.focus(&self.focus_handle);
        let action = self
            .preview_overlay_action_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, action)| *action);
        if let Some(action) = action {
            self.preview_overlay_press = Some(PreviewOverlayPress::Action);
            match action {
                GeneratedPreviewAction::Fullscreen => self.close_preview_overlay(),
                GeneratedPreviewAction::Restore => {
                    self.reset_preview_overlay_view();
                    self.clamp_preview_overlay_view();
                }
                GeneratedPreviewAction::ZoomIn => self.zoom_preview_overlay_view(1.25),
                GeneratedPreviewAction::ZoomOut => self.zoom_preview_overlay_view(0.8),
                GeneratedPreviewAction::Copy | GeneratedPreviewAction::Code => {}
            }
        } else if let Some(overlay) = self.preview_overlay_state() {
            self.image_drag_state = Some(ImageDragState {
                start_position: event.position,
                start_pan_x: overlay.view.pan_x,
                start_pan_y: overlay.view.pan_y,
            });
            self.preview_overlay_press = Some(PreviewOverlayPress::Pan {
                start_position: event.position,
                moved: false,
            });
        }
        cx.notify();
    }

    pub(super) fn on_preview_overlay_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(press @ PreviewOverlayPress::Pan { .. }) = self.preview_overlay_press else {
            return;
        };
        if !event.dragging() {
            self.preview_overlay_press = None;
            self.image_drag_state = None;
            cx.notify();
            return;
        }
        let press = press.on_move(event.position);
        self.preview_overlay_press = Some(press);
        if !matches!(press, PreviewOverlayPress::Pan { moved: true, .. }) {
            return;
        }
        if let Some(drag) = self.image_drag_state {
            if let Some(overlay) = self.preview_overlay_state_mut() {
                overlay.view.pan_x =
                    drag.start_pan_x + f32::from(event.position.x - drag.start_position.x);
                overlay.view.pan_y =
                    drag.start_pan_y + f32::from(event.position.y - drag.start_position.y);
            }
            self.clamp_preview_overlay_view();
            cx.notify();
        }
    }

    pub(super) fn on_preview_overlay_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        inside: bool,
        cx: &mut Context<Self>,
    ) {
        // The editor's mouse-up-out may run before the canvas mouse-up. Taking the
        // press here keeps either dispatch path from finishing the gesture twice.
        let press = self.preview_overlay_press.take();
        let drag = self.image_drag_state.take();
        if let Some(press) = press {
            let inside = inside
                && !self
                    .preview_overlay_action_areas
                    .iter()
                    .any(|(bounds, _)| bounds.contains(&event.position));
            if should_close_on_release(press, event.position, inside) {
                self.close_preview_overlay();
            }
        }
        if press.is_some() || drag.is_some() {
            cx.notify();
        }
    }

    pub(super) fn render_preview_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let editor = cx.entity().clone();
        div()
            .id("window-preview-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(self.styles.editor.editor_bg)
            .cursor(if self.image_drag_state.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(Self::on_preview_overlay_mouse_down),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    cx.stop_propagation();
                    this.on_preview_overlay_mouse_up(event, true, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.on_preview_overlay_mouse_up(event, false, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let hovered = this
                    .preview_overlay_action_areas
                    .iter()
                    .find(|(bounds, _)| bounds.contains(&event.position))
                    .map(|(_, action)| *action);
                if hovered != this.hovering_preview_overlay_action {
                    this.hovering_preview_overlay_action = hovered;
                    cx.notify();
                }
                this.on_preview_overlay_mouse_move(event, cx);
                cx.stop_propagation();
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                this.on_scroll(event, window, cx);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        editor.update(cx, |editor, _| {
                            let overlay = editor.preview_overlay_state()?.clone();
                            let base = editor
                                .image_cache
                                .get(&overlay.cache_key)?
                                .as_ref()?
                                .clone();
                            let dimensions = base.size(0);
                            let kind = editor
                                .decorations
                                .get(overlay.line_idx)
                                .and_then(|decoration| decoration.block_preview.as_ref())
                                .map(|preview| preview.kind);
                            let (width, height) = kind
                                .map(|kind| {
                                    helpers::intrinsic_generated_preview_size_for_kind(
                                        dimensions.width.0 as f32,
                                        dimensions.height.0 as f32,
                                        kind,
                                    )
                                })
                                .unwrap_or((dimensions.width.0 as f32, dimensions.height.0 as f32));
                            if width <= 0.0 || height <= 0.0 {
                                return None;
                            }
                            let scale = fit_scale(
                                width,
                                height,
                                bounds.size.width.into(),
                                bounds.size.height.into(),
                                matches!(kind, Some(crate::markdown::BlockPreviewKind::Mermaid)),
                            );
                            let display_width = width * scale * overlay.view.zoom_factor;
                            let display_height = height * scale * overlay.view.zoom_factor;
                            let image_bounds = Bounds::new(
                                point(
                                    bounds.left()
                                        + (bounds.size.width - px(display_width)) / 2.0
                                        + px(overlay.view.pan_x),
                                    bounds.top()
                                        + (bounds.size.height - px(display_height)) / 2.0
                                        + px(overlay.view.pan_y),
                                ),
                                size(px(display_width), px(display_height)),
                            );
                            let image = if let Some(kind) = kind {
                                let minimum = helpers::generated_preview_raster_scale(kind);
                                let raster_scale =
                                    (scale * overlay.view.zoom_factor * window.scale_factor())
                                        .max(minimum)
                                        .min(16.0);
                                let scale_key = (raster_scale * 100.0).round() as u16;
                                if let Some((_, cached)) = editor
                                    .generated_preview_overlay_cache
                                    .get(&overlay.cache_key)
                                    .filter(|(cached_scale, _)| *cached_scale == scale_key)
                                {
                                    cached.clone()
                                } else if let Some(svg) =
                                    editor.generated_preview_svg_cache.get(&overlay.cache_key)
                                {
                                    match helpers::load_generated_preview_overlay_image_from_svg(
                                        svg,
                                        kind,
                                        scale_key as f32 / 100.0,
                                    )
                                    .map(Arc::new)
                                    {
                                        Ok(image) => {
                                            if let Some((_, previous)) =
                                                editor.generated_preview_overlay_cache.insert(
                                                    overlay.cache_key.clone(),
                                                    (scale_key, image.clone()),
                                                )
                                            {
                                                editor.retired_images.push(previous);
                                            }
                                            image
                                        }
                                        Err(_) => base,
                                    }
                                } else {
                                    base
                                }
                            } else {
                                base
                            };
                            editor.preview_overlay_bounds = Some(bounds);
                            editor.preview_overlay_image_bounds = Some(image_bounds);
                            Some((
                                bounds,
                                image_bounds,
                                image,
                                editor.styles.editor.editor_bg,
                                matches!(kind, Some(crate::markdown::BlockPreviewKind::Mermaid)),
                            ))
                        })
                    },
                    |_, paint, window, _| {
                        if let Some((bounds, image_bounds, image, background, is_chart)) = paint {
                            window.paint_quad(fill(bounds, background));
                            let content_bounds = if is_chart {
                                let viewport_width = f32::from(bounds.size.width);
                                let viewport_height = f32::from(bounds.size.height);
                                let (inner_width, inner_height) =
                                    inner_size(viewport_width, viewport_height);
                                let inset_x = (viewport_width - inner_width) / 2.0;
                                let inset_y = (viewport_height - inner_height) / 2.0;
                                Bounds::new(
                                    point(bounds.left() + px(inset_x), bounds.top() + px(inset_y)),
                                    size(px(inner_width), px(inner_height)),
                                )
                            } else {
                                bounds
                            };
                            window.with_content_mask(
                                Some(ContentMask {
                                    bounds: content_bounds,
                                }),
                                |window| {
                                    window
                                        .paint_image(
                                            image_bounds,
                                            Corners::all(px(0.0)),
                                            image,
                                            0,
                                            false,
                                        )
                                        .ok();
                                },
                            );
                        }
                    },
                )
                .size_full(),
            )
            .child(self.render_preview_overlay_actions(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PreviewOverlayPress, clamp_pan, fit_scale, inner_size, moved_beyond_click_slop,
        should_close_on_release,
    };
    use gpui::{point, px};

    #[test]
    fn preview_press_release_distinguishes_clicks_from_drags() {
        let start = point(px(100.0), px(100.0));
        let pan = |moved| PreviewOverlayPress::Pan {
            start_position: start,
            moved,
        };
        assert!(should_close_on_release(pan(false), start, true));
        let small_move = point(px(102.0), px(103.0));
        assert!(should_close_on_release(
            pan(false).on_move(small_move),
            small_move,
            true
        ));
        assert!(!should_close_on_release(
            pan(false),
            point(px(105.0), px(100.0)),
            true
        ));
        assert!(moved_beyond_click_slop(start, point(px(105.0), px(100.0))));
        let dragged_back = pan(false)
            .on_move(point(px(105.0), px(100.0)))
            .on_move(start);
        assert!(!should_close_on_release(dragged_back, start, true));
        assert!(!should_close_on_release(pan(false), start, false));
        assert!(!should_close_on_release(
            PreviewOverlayPress::Action,
            start,
            true
        ));
    }

    #[test]
    fn fullscreen_preview_keeps_padding_on_every_edge() {
        for (width, height, viewport_width, viewport_height) in [
            (200.0, 100.0, 1000.0, 800.0),
            (100.0, 800.0, 1000.0, 600.0),
            (10.0, 10.0, 400.0, 300.0),
            (4000.0, 1000.0, 800.0, 600.0),
        ] {
            let scale = fit_scale(width, height, viewport_width, viewport_height, true);
            let (inner_width, inner_height) = inner_size(viewport_width, viewport_height);
            assert!(scale > 0.0);
            assert!(width * scale <= inner_width * super::CHART_INITIAL_VIEW_FRACTION + 0.01);
            assert!(height * scale <= inner_height * super::CHART_INITIAL_VIEW_FRACTION + 0.01);
            let other_scale = fit_scale(width, height, viewport_width, viewport_height, false);
            assert!(other_scale <= 1.0);
            assert!(width * other_scale <= inner_width + 0.01);
            assert!(height * other_scale <= inner_height + 0.01);
        }
    }

    #[test]
    fn fullscreen_charts_fill_eighty_percent_of_the_inner_view() {
        let (width, height) = inner_size(1000.0, 800.0);
        let scale = fit_scale(120.0, 40.0, 1000.0, 800.0, true);
        assert!((120.0 * scale - width * 0.8).abs() < 0.01);
        assert!(40.0 * scale <= height * 0.8);
        assert_eq!(fit_scale(120.0, 40.0, 1000.0, 800.0, false), 1.0);
    }

    #[test]
    fn panning_stays_out_of_padding_at_both_zoom_levels() {
        let (inner_width, inner_height) = inner_size(1000.0, 800.0);
        for (display_width, display_height) in [(400.0, 200.0), (1600.0, 1200.0)] {
            let pan_x = clamp_pan(10_000.0, display_width, inner_width);
            let pan_y = clamp_pan(-10_000.0, display_height, inner_height);
            if display_width < inner_width {
                assert!((inner_width - display_width) / 2.0 >= pan_x);
            } else {
                assert!(display_width / 2.0 - pan_x >= inner_width / 2.0);
            }
            if display_height < inner_height {
                assert!((inner_height - display_height) / 2.0 >= -pan_y);
            } else {
                assert!(display_height / 2.0 + pan_y >= inner_height / 2.0);
            }
            assert_eq!(clamp_pan(-10_000.0, display_width, inner_width), -pan_x);
            assert_eq!(clamp_pan(10_000.0, display_height, inner_height), -pan_y);
        }
    }
}
