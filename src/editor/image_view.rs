use std::sync::Arc;

use gpui::{
    App, Bounds, Corners, CursorStyle, Element, ElementId, Entity, GlobalElementId, Hitbox,
    HitboxBehavior, LayoutId, Pixels, RenderImage, Style, Window, point, px, relative,
};

use super::*;

pub(crate) struct ImageViewElement {
    pub editor: Entity<EditorView>,
}

pub(crate) struct ImageViewPrepaintState {
    hitbox: Hitbox,
    image: Option<(Bounds<Pixels>, Arc<RenderImage>)>,
}

impl gpui::IntoElement for ImageViewElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
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
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let viewport_width: f32 = bounds.size.width.into();
        let viewport_height: f32 = bounds.size.height.into();

        let image = {
            let editor = self.editor.read(cx);
            let Some(tab) = editor
                .active_tab_index()
                .and_then(|index| editor.tabs.get(index))
            else {
                return ImageViewPrepaintState {
                    hitbox,
                    image: None,
                };
            };
            let Some(path) = tab.path.as_ref() else {
                return ImageViewPrepaintState {
                    hitbox,
                    image: None,
                };
            };
            let key = path.to_string_lossy();
            let Some(Some(image)) = editor.image_cache.get(key.as_ref()) else {
                return ImageViewPrepaintState {
                    hitbox,
                    image: None,
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
                };
            }

            let fit_scale = (viewport_width / image_width)
                .min(viewport_height / image_height)
                .min(1.0);
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

        let _ = self.editor.update(cx, |editor, _| {
            editor.last_viewport_width = viewport_width;
            editor.last_viewport_height = viewport_height;
        });

        ImageViewPrepaintState { hitbox, image }
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
        window.set_cursor_style(CursorStyle::Arrow, &prepaint.hitbox);
        if let Some((bounds, image)) = &prepaint.image {
            let _ = window.paint_image(*bounds, Corners::all(px(0.0)), image.clone(), 0, false);
        }
    }
}
