use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, ContentMask, Context, Corners, CursorStyle, MouseButton, MouseMoveEvent,
    ScrollWheelEvent, canvas, div, fill, point, prelude::*, px, size,
};

use super::{EditorView, GeneratedPreviewAction, ImageDragState, helpers};

pub(super) fn fit_scale(
    width: f32,
    height: f32,
    viewport_width: f32,
    viewport_height: f32,
    generated: bool,
) -> f32 {
    let fit =
        (viewport_width.max(1.0) / width.max(1.0)).min(viewport_height.max(1.0) / height.max(1.0));
    if generated { fit * 0.8 } else { fit.min(1.0) }
}

impl EditorView {
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
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    window.focus(&this.focus_handle);
                    let action = this
                        .preview_overlay_action_areas
                        .iter()
                        .find(|(bounds, _)| bounds.contains(&event.position))
                        .map(|(_, action)| *action);
                    match action {
                        Some(GeneratedPreviewAction::Fullscreen) => this.close_preview_overlay(),
                        Some(GeneratedPreviewAction::Restore) => this.reset_preview_overlay_view(),
                        Some(GeneratedPreviewAction::ZoomIn) => {
                            this.zoom_preview_overlay_view(1.25)
                        }
                        Some(GeneratedPreviewAction::ZoomOut) => {
                            this.zoom_preview_overlay_view(0.8)
                        }
                        Some(GeneratedPreviewAction::Copy | GeneratedPreviewAction::Code) => {}
                        None => {
                            if let Some(overlay) = this.preview_overlay_state() {
                                this.image_drag_state = Some(ImageDragState {
                                    start_position: event.position,
                                    start_pan_x: overlay.view.pan_x,
                                    start_pan_y: overlay.view.pan_y,
                                });
                            }
                        }
                    }
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                    cx.stop_propagation();
                    if this.image_drag_state.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                    if this.image_drag_state.take().is_some() {
                        cx.notify();
                    }
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
                if let Some(drag) = this.image_drag_state {
                    if event.dragging() {
                        if let Some(overlay) = this.preview_overlay_state_mut() {
                            overlay.view.pan_x = drag.start_pan_x
                                + f32::from(event.position.x - drag.start_position.x);
                            overlay.view.pan_y = drag.start_pan_y
                                + f32::from(event.position.y - drag.start_position.y);
                        }
                        this.clamp_preview_overlay_view();
                        cx.notify();
                    } else {
                        this.image_drag_state = None;
                        cx.notify();
                    }
                }
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
                                kind.is_some(),
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
                                    (scale * window.scale_factor()).max(minimum).min(16.0);
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
                                    match helpers::load_render_image_from_svg_with_scale(
                                        svg,
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
                            Some((bounds, image_bounds, image, editor.styles.editor.editor_bg))
                        })
                    },
                    |_, paint, window, _| {
                        if let Some((bounds, image_bounds, image, background)) = paint {
                            window.paint_quad(fill(bounds, background));
                            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                window
                                    .paint_image(
                                        image_bounds,
                                        Corners::all(px(0.0)),
                                        image,
                                        0,
                                        false,
                                    )
                                    .ok();
                            });
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
    use super::fit_scale;

    #[test]
    fn generated_fullscreen_fits_eighty_percent_of_either_dimension() {
        for (width, height, viewport_width, viewport_height) in [
            (200.0, 100.0, 1000.0, 800.0),
            (100.0, 800.0, 1000.0, 600.0),
            (10.0, 10.0, 400.0, 300.0),
            (4000.0, 1000.0, 800.0, 600.0),
        ] {
            let scale = fit_scale(width, height, viewport_width, viewport_height, true);
            assert!(width * scale <= viewport_width * 0.8 + 0.01);
            assert!(height * scale <= viewport_height * 0.8 + 0.01);
            assert!(
                (width * scale / viewport_width - 0.8).abs() < 0.001
                    || (height * scale / viewport_height - 0.8).abs() < 0.001
            );
        }
    }
}
