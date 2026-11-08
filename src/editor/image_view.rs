use std::sync::Arc;

use gpui::{
    App, Bounds, Corners, CursorStyle, Element, ElementId, Entity, GlobalElementId, Hitbox,
    HitboxBehavior, LayoutId, Pixels, RenderImage, Style, Window, point, px, relative,
};

use super::*;

pub(crate) struct ImageViewElement {
    pub editor: Entity<EditorView>,
    pub pane_id: super::PaneId,
}

pub(crate) struct ImageViewPrepaintState {
    hitbox: Hitbox,
    image: Option<(Bounds<Pixels>, Arc<RenderImage>)>,
    dragging: bool,
}

impl gpui::IntoElement for ImageViewElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl ImageViewElement {
    fn prepaint_impl(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> ImageViewPrepaintState {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let viewport_width: f32 = bounds.size.width.into();
        let viewport_height: f32 = bounds.size.height.into();
        self.editor.update(cx, |editor, _| {
            editor.active_image_bounds = None;
        });

        let image = {
            let editor = self.editor.read(cx);
            let Some(tab) = editor
                .active_tab_index()
                .and_then(|index| editor.tabs.get(index))
            else {
                return ImageViewPrepaintState {
                    hitbox,
                    image: None,
                    dragging: false,
                };
            };
            let Some(path) = tab.path.as_ref() else {
                return ImageViewPrepaintState {
                    hitbox,
                    image: None,
                    dragging: false,
                };
            };
            let key = path.to_string_lossy();
            let Some(Some(image)) = editor.image_cache.get(key.as_ref()) else {
                return ImageViewPrepaintState {
                    hitbox,
                    image: None,
                    dragging: false,
                };
            };

            let state = tab.image_view;
            let image_size = image.size(0);
            let image_width = image_size.width.0 as f32;
            let image_height = image_size.height.0 as f32;
            if image_width <= 0.0 || image_height <= 0.0 {
                return ImageViewPrepaintState {
                    hitbox,
                    image: None,
                    dragging: false,
                };
            }

            let fit_scale = viewport_width / image_width;
            let display_width = image_width * fit_scale * state.zoom_factor;
            let display_height = image_height * fit_scale * state.zoom_factor;
            let origin_x =
                f32::from(bounds.origin.x) + (viewport_width - display_width) * 0.5 + state.pan_x;
            let origin_y =
                f32::from(bounds.origin.y) + (viewport_height - display_height) * 0.5 + state.pan_y;

            Some((
                Bounds::new(
                    point(px(origin_x), px(origin_y)),
                    gpui::size(px(display_width), px(display_height)),
                ),
                image.clone(),
            ))
        };

        let dragging = self.editor.read(cx).image_drag_state.is_some();

        let _ = self.editor.update(cx, |editor, _| {
            editor.content_origin = bounds.origin;
            editor.last_viewport_width = viewport_width;
            editor.last_viewport_height = viewport_height;
            editor.active_image_bounds = image
                .as_ref()
                .map(|(image_bounds, _)| image_bounds.intersect(&bounds));
        });

        ImageViewPrepaintState {
            hitbox,
            image,
            dragging,
        }
    }
}

impl Element for ImageViewElement {
    type RequestLayoutState = ();
    type PrepaintState = ImageViewPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let pane_swapped = self
            .editor
            .update(cx, |editor, _| editor.begin_pane_swap(self.pane_id));
        let state = self.prepaint_impl(bounds, window, cx);
        if pane_swapped {
            self.editor
                .update(cx, |editor, _| editor.end_pane_swap(self.pane_id));
        }
        state
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let hitbox = prepaint.hitbox.clone();
        let editor = self.editor.clone();
        let pane_id = self.pane_id;
        window.on_mouse_event(move |event: &gpui::MagnifyEvent, phase, window, cx| {
            if phase != gpui::DispatchPhase::Bubble
                || !hitbox.is_hovered(window)
                || !event.delta.is_finite()
                || event.delta.abs() < 0.0001
            {
                return;
            }
            editor.update(cx, |editor, cx| {
                editor.with_pane(pane_id, |editor| {
                    if editor.active_tab_is_image() {
                        editor.zoom_active_image_view(
                            (1.0 + event.delta).clamp(0.5, 2.0),
                            Some(event.position),
                        );
                        cx.stop_propagation();
                        cx.notify();
                    }
                });
            });
        });
        window.set_cursor_style(
            if prepaint.dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            },
            &prepaint.hitbox,
        );
        if let Some((bounds, image)) = &prepaint.image {
            let _ = window.paint_image(*bounds, Corners::all(px(0.0)), image.clone(), 0, false);
        }
    }
}
