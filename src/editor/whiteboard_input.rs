//! Pointer interaction for the whiteboard canvas: tool dispatch, selection,
//! moving, resizing (incl. rotated boxes), rotating, linear-endpoint editing,
//! marquee selection, and pan/zoom. These methods are invoked from the editor's
//! shared mouse/scroll handlers when the active tab is a whiteboard.

use std::collections::HashMap;

use gpui::{
    AppContext, ClipboardItem, Context, CursorStyle, Entity, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Point, ScrollWheelEvent, Subscription, Window, px,
};

use super::whiteboard_geometry::{
    CanvasView, Pt, bbox_handle_anchor, bbox_handle_points, bounds_of, element_corners,
    element_edge_point, hit_test, intersects_rect, label_center, label_hit_test, linear_endpoints,
    resize_handle_points, route_cross_for_pointer, route_curve_offsets_for_pointer, route_handles,
    route_mid_for_pointer, selection_bbox, snap_to_grid,
};
use super::*;
use crate::text_input::{TextInput, TextInputEvent};

/// Active inline text editor over a whiteboard element. Holds the `TextInput`
/// entity and keeps its subscriptions alive.
pub(crate) struct WhiteboardTextEditor {
    pub element_id: String,
    pub input: Entity<TextInput>,
    /// Original text, restored when the edit is cancelled.
    pub original_text: String,
    _changed: Subscription,
    _blur: Subscription,
}

/// Pixel radius within which a pointer is considered to be on a handle.
const HANDLE_HIT_RADIUS: f32 = 9.0;
/// Width of the rotation ring just outside each corner handle. Wider than the
/// handle itself so the rotate affordance is easy to grab.
const ROTATE_RING_WIDTH: f32 = 20.0;
/// Minimum element dimension (canvas units) enforced during resize.
const MIN_SIZE: f32 = 4.0;

/// Z-order reordering operations for the selected elements.
#[derive(Clone, Copy)]
enum ZOrderOp {
    Forward,
    Backward,
    Front,
    Back,
}

fn dist(a: Pt, b: Pt) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn screen_pt(p: Point<gpui::Pixels>) -> Pt {
    (f32::from(p.x), f32::from(p.y))
}

impl EditorView {
    /// The current canvas transform, derived from the last painted bounds and
    /// the document's pan/zoom.
    pub(crate) fn whiteboard_canvas_view(&self) -> Option<CanvasView> {
        let bounds = self.whiteboard_canvas_bounds?;
        let state = self.active_whiteboard()?;
        let app = state.document.app_state;
        Some(CanvasView {
            origin_x: bounds.origin.x.into(),
            origin_y: bounds.origin.y.into(),
            scroll_x: app.scroll_x,
            scroll_y: app.scroll_y,
            zoom: app.zoom.clamp(0.05, 40.0),
        })
    }

    pub(crate) fn whiteboard_on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle(cx));
        if event.button != MouseButton::Left {
            return;
        }
        let Some(view) = self.whiteboard_canvas_view() else {
            return;
        };
        let screen = screen_pt(event.position);
        self.whiteboard_pointer = Some(event.position);
        let canvas = view.to_canvas(screen);
        let shift = event.modifiers.shift;

        // A click that reaches the canvas while editing text is outside the
        // text box (the overlay occludes inside-clicks), so commit the edit and
        // continue handling the click for selection/drawing.
        if self.whiteboard_text_editor.is_some() {
            self.whiteboard_commit_text_edit(false, cx);
        }

        // Hold Space to pan the canvas by dragging.
        if self.whiteboard_space_down {
            let scroll = self
                .active_whiteboard()
                .map(|state| {
                    (
                        state.document.app_state.scroll_x,
                        state.document.app_state.scroll_y,
                    )
                })
                .unwrap_or((0.0, 0.0));
            self.whiteboard_drag = Some(WhiteboardDrag::Pan {
                start_pointer: screen,
                start_scroll: scroll,
            });
            cx.notify();
            return;
        }

        let tool = self
            .active_whiteboard()
            .map(|state| state.tool)
            .unwrap_or(WhiteboardTool::Select);

        if let Some(kind) = tool.element_kind() {
            if kind == ElementKind::Text {
                cx.stop_propagation();
                self.whiteboard_create_text(canvas, window, cx);
            } else {
                self.whiteboard_begin_create(kind, canvas, cx);
            }
            return;
        }

        // Double-click enters inline text editing on any element (shapes and
        // lines get a centered label; text elements edit in place).
        if event.click_count >= 2 {
            let tolerance = 6.0 / view.zoom;
            if let Some(id) = self.whiteboard_pick_element(canvas, tolerance) {
                if let Some(state) = self.active_whiteboard_mut() {
                    state.selected_ids = vec![id.clone()];
                }
                cx.stop_propagation();
                self.whiteboard_begin_text_edit(id, false, window, cx);
                return;
            }
        }

        // Select tool: handles take priority over picking.
        if let Some(drag) = self.whiteboard_hit_handle(&view, screen) {
            self.whiteboard_push_undo();
            self.whiteboard_drag = Some(drag);
            cx.notify();
            return;
        }

        let tolerance = 6.0 / view.zoom;
        let picked = self.whiteboard_pick_element(canvas, tolerance);
        let group_pick = if picked.is_none() {
            // Clicking anywhere inside a group's rectangle selects the group.
            self.whiteboard_pick_group_at(canvas)
        } else {
            None
        };
        if picked.is_some() || group_pick.is_some() {
            if let Some(id) = picked {
                let already = self
                    .active_whiteboard()
                    .map(|state| state.is_selected(&id))
                    .unwrap_or(false);
                if let Some(state) = self.active_whiteboard_mut() {
                    if shift {
                        if already {
                            state.selected_ids.retain(|other| other != &id);
                        } else {
                            state.selected_ids.push(id.clone());
                        }
                    } else if !already {
                        state.selected_ids = vec![id.clone()];
                    }
                }
            } else if let Some(members) = group_pick
                && let Some(state) = self.active_whiteboard_mut()
            {
                if shift {
                    for id in members {
                        if !state.selected_ids.contains(&id) {
                            state.selected_ids.push(id);
                        }
                    }
                } else {
                    let already = members.iter().all(|id| state.selected_ids.contains(id));
                    if !already {
                        state.selected_ids = members;
                    }
                }
            }
            // Clicking a grouped element selects the whole (outermost) group.
            self.whiteboard_expand_selection_to_groups();
            let origins = self.whiteboard_selection_origins();
            if !origins.is_empty() {
                self.whiteboard_drag = Some(WhiteboardDrag::Move {
                    start: canvas,
                    origins,
                    moved: false,
                });
            }
            cx.notify();
            return;
        }

        // Empty canvas: start a marquee (clearing selection unless extending).
        if let Some(state) = self.active_whiteboard_mut()
            && !shift
        {
            state.selected_ids.clear();
        }
        self.whiteboard_drag = Some(WhiteboardDrag::Marquee {
            start: canvas,
            current: canvas,
        });
        cx.notify();
    }

    pub(crate) fn whiteboard_on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.whiteboard_canvas_view() else {
            return;
        };
        let screen = screen_pt(event.position);
        let canvas = view.to_canvas(screen);
        self.whiteboard_pointer = Some(event.position);
        let Some(drag) = self.whiteboard_drag.clone() else {
            // Not dragging: refresh so the hover cursor (resize/rotate) updates.
            cx.notify();
            return;
        };
        let shift = event.modifiers.shift;

        match drag {
            WhiteboardDrag::Create { id, start } => {
                if let Some(state) = self.active_whiteboard_mut()
                    && let Some(index) = state.document.element_index(&id)
                {
                    let element = &mut state.document.elements[index];
                    element.width = canvas.0 - start.0;
                    element.height = canvas.1 - start.1;
                    if shift {
                        constrain_proportions(element);
                    }
                }
                // While drawing a linear element, highlight bindable elements
                // under both endpoints.
                let is_linear = self
                    .active_whiteboard()
                    .and_then(|state| state.document.element(&id))
                    .map(|element| element.kind.is_linear())
                    .unwrap_or(false);
                let mut targets = Vec::new();
                if is_linear {
                    if let Some(target) = self.whiteboard_element_at(start, Some(&id)) {
                        targets.push(target);
                    }
                    if let Some(target) = self.whiteboard_element_at(canvas, Some(&id))
                        && !targets.contains(&target)
                    {
                        targets.push(target);
                    }
                }
                self.whiteboard_bind_targets = targets;
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::Move {
                start,
                origins,
                moved,
            } => {
                if !moved {
                    self.whiteboard_push_undo();
                }
                // Snap the drag delta so the first element's origin lands on
                // the grid (Figma-style snapping that preserves relative
                // offsets within the selection).
                let mut dx = canvas.0 - start.0;
                let mut dy = canvas.1 - start.1;
                if let Some((_, ox, oy)) = origins.first() {
                    dx = snap_to_grid(ox + dx) - ox;
                    dy = snap_to_grid(oy + dy) - oy;
                }
                if let Some(state) = self.active_whiteboard_mut() {
                    for (id, ox, oy) in &origins {
                        if let Some(index) = state.document.element_index(id) {
                            let element = &mut state.document.elements[index];
                            element.x = ox + dx;
                            element.y = oy + dy;
                        }
                    }
                }
                self.whiteboard_drag = Some(WhiteboardDrag::Move {
                    start,
                    origins,
                    moved: true,
                });
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::Resize {
                id,
                handle,
                anchor,
                rotation,
                aspect,
            } => {
                if let Some(state) = self.active_whiteboard_mut()
                    && let Some(index) = state.document.element_index(&id)
                {
                    apply_resize(
                        &mut state.document.elements[index],
                        handle,
                        anchor,
                        rotation,
                        aspect,
                        canvas,
                        shift,
                    );
                }
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::MoveEndpoint { id, end } => {
                if let Some(state) = self.active_whiteboard_mut()
                    && let Some(index) = state.document.element_index(&id)
                {
                    let element = &mut state.document.elements[index];
                    if end {
                        element.width = canvas.0 - element.x;
                        element.height = canvas.1 - element.y;
                    } else {
                        let end_x = element.x + element.width;
                        let end_y = element.y + element.height;
                        element.x = canvas.0;
                        element.y = canvas.1;
                        element.width = end_x - canvas.0;
                        element.height = end_y - canvas.1;
                    }
                }
                // Highlight a magnetic bind target under the pointer.
                let targets: Vec<String> = self
                    .whiteboard_element_at(canvas, Some(&id))
                    .into_iter()
                    .collect();
                if self.whiteboard_bind_targets != targets {
                    self.whiteboard_bind_targets = targets;
                }
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::RouteWaypoint { id, handle } => {
                if let Some(state) = self.active_whiteboard_mut()
                    && let Some(index) = state.document.element_index(&id)
                {
                    let element = &mut state.document.elements[index];
                    match element.route {
                        RouteStyle::Curved => {
                            let (offset_n, offset_t) =
                                route_curve_offsets_for_pointer(element, canvas);
                            element.route_curve = offset_n;
                            element.route_curve_t = offset_t;
                        }
                        RouteStyle::Orthogonal => match handle {
                            RouteHandle::SideStart => {
                                element.route_mid = route_mid_for_pointer(element, canvas);
                            }
                            RouteHandle::SideEnd => {
                                element.route_mid2 = Some(route_mid_for_pointer(element, canvas));
                            }
                            RouteHandle::Cross => {
                                element.route_cross = route_cross_for_pointer(element, canvas);
                            }
                        },
                        RouteStyle::Straight => {}
                    }
                }
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::GroupResize {
                handle,
                anchor,
                handle_origin,
                originals,
            } => {
                let affects_x = !matches!(handle, ResizeHandle::Top | ResizeHandle::Bottom);
                let affects_y = !matches!(handle, ResizeHandle::Left | ResizeHandle::Right);
                let scale = |pointer: f32, origin: f32, anchor_v: f32| -> f32 {
                    let denom = origin - anchor_v;
                    if denom.abs() < 1e-3 {
                        1.0
                    } else {
                        (pointer - anchor_v) / denom
                    }
                };
                let mut sx = if affects_x {
                    scale(canvas.0, handle_origin.0, anchor.0)
                } else {
                    1.0
                };
                let mut sy = if affects_y {
                    scale(canvas.1, handle_origin.1, anchor.1)
                } else {
                    1.0
                };
                if shift && handle.is_corner() {
                    let magnitude = sx.abs().max(sy.abs());
                    sx = magnitude.copysign(sx);
                    sy = magnitude.copysign(sy);
                }
                let clamp_scale = |s: f32| -> f32 {
                    if s.abs() < 0.02 {
                        0.02_f32.copysign(if s == 0.0 { 1.0 } else { s })
                    } else {
                        s
                    }
                };
                sx = clamp_scale(sx);
                sy = clamp_scale(sy);
                if let Some(state) = self.active_whiteboard_mut() {
                    for (id, ox, oy, ow, oh) in &originals {
                        if let Some(index) = state.document.element_index(id) {
                            let element = &mut state.document.elements[index];
                            element.x = anchor.0 + (ox - anchor.0) * sx;
                            element.y = anchor.1 + (oy - anchor.1) * sy;
                            element.width = ow * sx;
                            element.height = oh * sy;
                        }
                    }
                }
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::GroupRotate {
                center,
                start_pointer,
                originals,
            } => {
                let center_screen = view.to_screen(center);
                let pointer = (screen.1 - center_screen.1).atan2(screen.0 - center_screen.0);
                let mut delta = pointer - start_pointer;
                if shift {
                    let step = std::f32::consts::FRAC_PI_4;
                    delta = (delta / step).round() * step;
                }
                let (sin, cos) = delta.sin_cos();
                if let Some(state) = self.active_whiteboard_mut() {
                    for (id, ox, oy, ow, oh, orot) in &originals {
                        if let Some(index) = state.document.element_index(id) {
                            let element = &mut state.document.elements[index];
                            let ecx = ox + ow * 0.5;
                            let ecy = oy + oh * 0.5;
                            let dx = ecx - center.0;
                            let dy = ecy - center.1;
                            let ncx = center.0 + dx * cos - dy * sin;
                            let ncy = center.1 + dx * sin + dy * cos;
                            element.x = ncx - ow * 0.5;
                            element.y = ncy - oh * 0.5;
                            element.rotation = orot + delta;
                        }
                    }
                }
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::Rotate {
                id,
                center,
                start_pointer,
                start_rotation,
            } => {
                let center_screen = view.to_screen(center);
                let pointer = (screen.1 - center_screen.1).atan2(screen.0 - center_screen.0);
                let mut rotation = start_rotation + (pointer - start_pointer);
                if shift {
                    // Snap to 45° increments while Shift is held.
                    let step = std::f32::consts::FRAC_PI_4;
                    rotation = (rotation / step).round() * step;
                }
                if let Some(state) = self.active_whiteboard_mut()
                    && let Some(index) = state.document.element_index(&id)
                {
                    state.document.elements[index].rotation = rotation;
                }
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::Marquee { start, .. } => {
                let min = (start.0.min(canvas.0), start.1.min(canvas.1));
                let max = (start.0.max(canvas.0), start.1.max(canvas.1));
                if let Some(state) = self.active_whiteboard_mut() {
                    state.selected_ids = state
                        .document
                        .elements
                        .iter()
                        .filter(|element| intersects_rect(element, min, max))
                        .map(|element| element.id.clone())
                        .collect();
                }
                self.whiteboard_expand_selection_to_groups();
                self.whiteboard_drag = Some(WhiteboardDrag::Marquee {
                    start,
                    current: canvas,
                });
                cx.notify();
            }
            WhiteboardDrag::Pan {
                start_pointer,
                start_scroll,
            } => {
                let dx = (screen.0 - start_pointer.0) / view.zoom;
                let dy = (screen.1 - start_pointer.1) / view.zoom;
                if let Some(state) = self.active_whiteboard_mut() {
                    let app = &mut state.document.app_state;
                    app.scroll_x = start_scroll.0 - dx;
                    app.scroll_y = start_scroll.1 - dy;
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn whiteboard_on_mouse_up(&mut self, _event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.whiteboard_drag.take() else {
            return;
        };
        match drag {
            WhiteboardDrag::Create { id, start } => {
                if let Some(state) = self.active_whiteboard_mut() {
                    if let Some(index) = state.document.element_index(&id) {
                        let element = &mut state.document.elements[index];
                        if element.width.abs() < 3.0 && element.height.abs() < 3.0 {
                            let (default_w, default_h) = if element.kind.is_linear() {
                                (140.0, 0.0)
                            } else {
                                (120.0, 80.0)
                            };
                            element.x = start.0;
                            element.y = start.1;
                            element.width = default_w;
                            element.height = default_h;
                        }
                        normalize_box(element);
                    }
                    state.tool = WhiteboardTool::Select;
                }
                // A newly drawn arrow/line magnetically binds endpoints that
                // land on elements.
                let is_linear = self
                    .active_whiteboard()
                    .and_then(|state| state.document.element(&id))
                    .map(|element| element.kind.is_linear())
                    .unwrap_or(false);
                if is_linear {
                    let (start_pt, end_pt) = self
                        .active_whiteboard()
                        .and_then(|state| state.document.element(&id))
                        .map(|element| {
                            (
                                (element.x, element.y),
                                (element.x + element.width, element.y + element.height),
                            )
                        })
                        .unwrap_or(((0.0, 0.0), (0.0, 0.0)));
                    let start_target = self.whiteboard_element_at(start_pt, Some(&id));
                    let end_target = self.whiteboard_element_at(end_pt, Some(&id));
                    if let Some(state) = self.active_whiteboard_mut()
                        && let Some(index) = state.document.element_index(&id)
                    {
                        state.document.elements[index].start_binding = start_target;
                        state.document.elements[index].end_binding = end_target;
                    }
                    self.whiteboard_resolve_bindings();
                }
                self.whiteboard_bind_targets.clear();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::Resize { id, .. } => {
                if let Some(state) = self.active_whiteboard_mut()
                    && let Some(index) = state.document.element_index(&id)
                {
                    normalize_box(&mut state.document.elements[index]);
                }
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::MoveEndpoint { id, end } => {
                // Finalize a magnetic binding to the highlighted target.
                let target = self.whiteboard_bind_targets.first().cloned();
                self.whiteboard_bind_targets.clear();
                if let Some(state) = self.active_whiteboard_mut()
                    && let Some(index) = state.document.element_index(&id)
                {
                    if end {
                        state.document.elements[index].end_binding = target;
                    } else {
                        state.document.elements[index].start_binding = target;
                    }
                }
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::GroupResize { originals, .. } => {
                if let Some(state) = self.active_whiteboard_mut() {
                    for (id, ..) in &originals {
                        if let Some(index) = state.document.element_index(id) {
                            let element = &mut state.document.elements[index];
                            if !element.kind.is_linear() {
                                normalize_box(element);
                            }
                        }
                    }
                }
                self.whiteboard_resolve_bindings();
                self.mark_whiteboard_dirty();
                cx.notify();
            }
            WhiteboardDrag::Move { .. } => {
                self.whiteboard_resolve_bindings();
                cx.notify();
            }
            WhiteboardDrag::Rotate { .. }
            | WhiteboardDrag::GroupRotate { .. }
            | WhiteboardDrag::RouteWaypoint { .. }
            | WhiteboardDrag::Marquee { .. }
            | WhiteboardDrag::Pan { .. } => {
                cx.notify();
            }
        }
        self.whiteboard_pointer = None;
    }

    pub(crate) fn whiteboard_on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.whiteboard_canvas_view() else {
            return;
        };
        let delta = event.delta.pixel_delta(px(40.0));
        let dx = f32::from(delta.x);
        let dy = f32::from(delta.y);
        let zoom_gesture = event.modifiers.platform || event.modifiers.control;

        if zoom_gesture {
            if dy.abs() < 0.01 {
                return;
            }
            let ratio = 1.0 + (dy.abs() / 300.0).clamp(0.02, 0.25);
            let factor = if dy < 0.0 { ratio } else { 1.0 / ratio };
            let screen = screen_pt(event.position);
            let canvas_before = view.to_canvas(screen);
            if let Some(state) = self.active_whiteboard_mut() {
                let app = &mut state.document.app_state;
                let new_zoom = (app.zoom * factor).clamp(0.05, 40.0);
                app.zoom = new_zoom;
                app.scroll_x = canvas_before.0 - (screen.0 - view.origin_x) / new_zoom;
                app.scroll_y = canvas_before.1 - (screen.1 - view.origin_y) / new_zoom;
            }
            cx.notify();
        } else {
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            if let Some(state) = self.active_whiteboard_mut() {
                let app = &mut state.document.app_state;
                app.scroll_x -= dx / view.zoom;
                app.scroll_y -= dy / view.zoom;
            }
            cx.notify();
        }
    }

    /// Mark the active whiteboard tab dirty after a mutation.
    pub(crate) fn mark_whiteboard_dirty(&mut self) {
        if let Some(index) = self.active_tab_index()
            && let Some(tab) = self.tabs.get_mut(index)
        {
            tab.is_dirty = true;
        }
    }

    pub(crate) fn whiteboard_has_selection(&self) -> bool {
        self.active_whiteboard()
            .map(|state| !state.selected_ids.is_empty())
            .unwrap_or(false)
    }

    // ── Undo / redo ──────────────────────────────────────────────────────────

    /// Snapshot the current document onto the undo stack before a mutation.
    pub(crate) fn whiteboard_push_undo(&mut self) {
        if let Some(state) = self.active_whiteboard_mut() {
            let snapshot = state.document.clone();
            state.undo_stack.push(snapshot);
            if state.undo_stack.len() > 200 {
                state.undo_stack.remove(0);
            }
            state.redo_stack.clear();
        }
    }

    pub(crate) fn whiteboard_undo(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut()
            && let Some(previous) = state.undo_stack.pop()
        {
            let current = std::mem::replace(&mut state.document, previous);
            state.redo_stack.push(current);
            state.selected_ids.clear();
            changed = true;
        }
        if changed {
            self.whiteboard_resolve_bindings();
            self.mark_whiteboard_dirty();
            cx.notify();
        }
    }

    pub(crate) fn whiteboard_redo(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(state) = self.active_whiteboard_mut()
            && let Some(next) = state.redo_stack.pop()
        {
            let current = std::mem::replace(&mut state.document, next);
            state.undo_stack.push(current);
            state.selected_ids.clear();
            changed = true;
        }
        if changed {
            self.whiteboard_resolve_bindings();
            self.mark_whiteboard_dirty();
            cx.notify();
        }
    }

    // ── Clipboard / delete / selection ───────────────────────────────────────

    pub(crate) fn whiteboard_copy(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.active_whiteboard() else {
            return;
        };
        let selected: Vec<WhiteboardElement> = state.selected_elements().cloned().collect();
        if selected.is_empty() {
            return;
        }
        let text = serialize_elements_for_clipboard(&selected);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    pub(crate) fn whiteboard_cut(&mut self, cx: &mut Context<Self>) {
        let has_selection = self
            .active_whiteboard()
            .map(|state| !state.selected_ids.is_empty())
            .unwrap_or(false);
        if !has_selection {
            return;
        }
        self.whiteboard_copy(cx);
        self.whiteboard_delete_selected(cx);
    }

    pub(crate) fn whiteboard_paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(text) = item.text() else {
            return;
        };
        let Some(mut elements) = parse_clipboard_elements(&text) else {
            return;
        };
        if elements.is_empty() {
            return;
        }
        self.whiteboard_push_undo();
        // Fresh element ids, remapped group ids (so a pasted group stays its
        // own group), and bindings remapped within the pasted set.
        let mut id_map: HashMap<String, String> = HashMap::new();
        for element in elements.iter_mut() {
            let new_id = new_element_id();
            id_map.insert(element.id.clone(), new_id.clone());
            element.id = new_id;
        }
        let mut group_map: HashMap<String, String> = HashMap::new();
        for element in elements.iter_mut() {
            for group in element.group_ids.iter_mut() {
                let mapped = group_map
                    .entry(group.clone())
                    .or_insert_with(new_element_id);
                *group = mapped.clone();
            }
            element.start_binding = element
                .start_binding
                .take()
                .and_then(|target| id_map.get(&target).cloned());
            element.end_binding = element
                .end_binding
                .take()
                .and_then(|target| id_map.get(&target).cloned());
        }
        let mut new_ids = Vec::new();
        if let Some(state) = self.active_whiteboard_mut() {
            for element in elements.iter_mut() {
                element.x += 16.0;
                element.y += 16.0;
                new_ids.push(element.id.clone());
                state.document.elements.push(element.clone());
            }
            state.selected_ids = new_ids;
        }
        self.whiteboard_resolve_bindings();
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    pub(crate) fn whiteboard_delete_selected(&mut self, cx: &mut Context<Self>) {
        let has_selection = self
            .active_whiteboard()
            .map(|state| !state.selected_ids.is_empty())
            .unwrap_or(false);
        if !has_selection {
            return;
        }
        self.whiteboard_push_undo();
        if let Some(state) = self.active_whiteboard_mut() {
            let ids = state.selected_ids.clone();
            state
                .document
                .elements
                .retain(|element| !ids.iter().any(|id| id == &element.id));
            state.selected_ids.clear();
        }
        self.whiteboard_resolve_bindings();
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    pub(crate) fn whiteboard_select_all(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.active_whiteboard_mut() {
            state.selected_ids = state
                .document
                .elements
                .iter()
                .map(|element| element.id.clone())
                .collect();
        }
        cx.notify();
    }

    // ── Z-order ──────────────────────────────────────────────────────────────

    /// Move the selected elements one step toward the front (drawn later).
    pub(crate) fn whiteboard_bring_forward(&mut self, cx: &mut Context<Self>) {
        self.whiteboard_reorder(ZOrderOp::Forward, cx);
    }

    /// Move the selected elements one step toward the back (drawn earlier).
    pub(crate) fn whiteboard_send_backward(&mut self, cx: &mut Context<Self>) {
        self.whiteboard_reorder(ZOrderOp::Backward, cx);
    }

    /// Move the selected elements to the very front.
    pub(crate) fn whiteboard_bring_to_front(&mut self, cx: &mut Context<Self>) {
        self.whiteboard_reorder(ZOrderOp::Front, cx);
    }

    /// Move the selected elements to the very back.
    pub(crate) fn whiteboard_send_to_back(&mut self, cx: &mut Context<Self>) {
        self.whiteboard_reorder(ZOrderOp::Back, cx);
    }

    /// Original positions of all selected elements, captured at drag start.
    fn whiteboard_selection_origins(&self) -> Vec<(String, f32, f32)> {
        let Some(state) = self.active_whiteboard() else {
            return Vec::new();
        };
        state
            .selected_elements()
            .map(|element| (element.id.clone(), element.x, element.y))
            .collect()
    }

    /// Expand the selection so every member of a selected element's outermost
    /// group is also selected (groups select as a unit).
    pub(crate) fn whiteboard_expand_selection_to_groups(&mut self) {
        let Some(state) = self.active_whiteboard_mut() else {
            return;
        };
        let mut groups: Vec<String> = Vec::new();
        for element in state.document.elements.iter() {
            if state.selected_ids.iter().any(|id| id == &element.id)
                && let Some(group) = element.outermost_group()
                && !groups.iter().any(|g| g == group)
            {
                groups.push(group.to_string());
            }
        }
        if groups.is_empty() {
            return;
        }
        for element in state.document.elements.iter() {
            if let Some(group) = element.outermost_group()
                && groups.iter().any(|g| g == group)
                && !state.selected_ids.iter().any(|id| id == &element.id)
            {
                state.selected_ids.push(element.id.clone());
            }
        }
    }

    /// Whether the current selection can be grouped (two or more distinct
    /// top-level units).
    pub(crate) fn whiteboard_can_group(&self) -> bool {
        let Some(state) = self.active_whiteboard() else {
            return false;
        };
        if state.selected_ids.len() < 2 {
            return false;
        }
        // Grouping is meaningful unless everything already shares one
        // outermost group.
        self.whiteboard_common_outer_group().is_none()
    }

    /// The outermost group shared by ALL selected elements, if any.
    fn whiteboard_common_outer_group(&self) -> Option<String> {
        let state = self.active_whiteboard()?;
        let mut iter = state.selected_elements();
        let first = iter.next()?.outermost_group()?.to_string();
        for element in state.selected_elements() {
            if element.outermost_group() != Some(first.as_str()) {
                return None;
            }
        }
        Some(first)
    }

    /// Whether the current selection can be ungrouped (all share a group).
    pub(crate) fn whiteboard_can_ungroup(&self) -> bool {
        self.whiteboard_common_outer_group().is_some()
    }

    /// Group the selected elements together (groups nest by stacking ids).
    pub(crate) fn whiteboard_group_selection(&mut self, cx: &mut Context<Self>) {
        if !self.whiteboard_can_group() {
            return;
        }
        self.whiteboard_push_undo();
        let group_id = new_element_id();
        if let Some(state) = self.active_whiteboard_mut() {
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.group_ids.push(group_id.clone());
                }
            }
        }
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    /// Dissolve the outermost group shared by the selection.
    pub(crate) fn whiteboard_ungroup_selection(&mut self, cx: &mut Context<Self>) {
        let Some(group) = self.whiteboard_common_outer_group() else {
            return;
        };
        self.whiteboard_push_undo();
        if let Some(state) = self.active_whiteboard_mut() {
            for element in state.document.elements.iter_mut() {
                if element.outermost_group() == Some(group.as_str()) {
                    element.group_ids.pop();
                }
            }
            // Drop the selection so the old group frame disappears immediately.
            state.selected_ids.clear();
        }
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    /// Move the selection by one grid step in the given direction.
    pub(crate) fn whiteboard_nudge(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        if !self.whiteboard_has_selection() {
            return;
        }
        self.whiteboard_push_undo();
        if let Some(state) = self.active_whiteboard_mut() {
            let ids = state.selected_ids.clone();
            for element in state.document.elements.iter_mut() {
                if ids.iter().any(|id| id == &element.id) {
                    element.x = snap_to_grid(element.x + dx);
                    element.y = snap_to_grid(element.y + dy);
                }
            }
        }
        self.whiteboard_resolve_bindings();
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    pub(crate) fn whiteboard_flip_horizontal(&mut self, cx: &mut Context<Self>) {
        self.whiteboard_flip(true, cx);
    }

    pub(crate) fn whiteboard_flip_vertical(&mut self, cx: &mut Context<Self>) {
        self.whiteboard_flip(false, cx);
    }

    /// Mirror the selection across the vertical (horizontal flip) or
    /// horizontal (vertical flip) axis through the selection bbox center.
    fn whiteboard_flip(&mut self, horizontal: bool, cx: &mut Context<Self>) {
        if !self.whiteboard_has_selection() {
            return;
        }
        self.whiteboard_push_undo();
        if let Some(state) = self.active_whiteboard_mut() {
            let ids = state.selected_ids.clone();
            let Some((min, max)) = selection_bbox(
                state
                    .document
                    .elements
                    .iter()
                    .filter(|e| ids.iter().any(|id| id == &e.id)),
            ) else {
                return;
            };
            let center = ((min.0 + max.0) * 0.5, (min.1 + max.1) * 0.5);
            for element in state.document.elements.iter_mut() {
                if !ids.iter().any(|id| id == &element.id) {
                    continue;
                }
                if element.kind.is_linear() {
                    // Mirror both endpoints; width/height may go negative,
                    // which linear geometry supports.
                    if horizontal {
                        element.x = 2.0 * center.0 - element.x;
                        element.width = -element.width;
                    } else {
                        element.y = 2.0 * center.1 - element.y;
                        element.height = -element.height;
                    }
                    element.route_curve = -element.route_curve;
                } else if horizontal {
                    element.x = 2.0 * center.0 - element.x - element.width;
                } else {
                    element.y = 2.0 * center.1 - element.y - element.height;
                }
                element.rotation = -element.rotation;
            }
        }
        self.whiteboard_resolve_bindings();
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    fn whiteboard_reorder(&mut self, op: ZOrderOp, cx: &mut Context<Self>) {
        if !self.whiteboard_has_selection() {
            return;
        }
        self.whiteboard_push_undo();
        if let Some(state) = self.active_whiteboard_mut() {
            let is_sel =
                |element: &WhiteboardElement| state.selected_ids.iter().any(|id| id == &element.id);
            let elements = &mut state.document.elements;
            match op {
                ZOrderOp::Front => {
                    let (mut keep, sel): (Vec<_>, Vec<_>) = std::mem::take(elements)
                        .into_iter()
                        .partition(|e| !is_sel(e));
                    keep.extend(sel);
                    *elements = keep;
                }
                ZOrderOp::Back => {
                    let (sel, keep): (Vec<_>, Vec<_>) = std::mem::take(elements)
                        .into_iter()
                        .partition(|e| is_sel(e));
                    let mut result = sel;
                    result.extend(keep);
                    *elements = result;
                }
                ZOrderOp::Forward => {
                    let n = elements.len();
                    for i in (0..n.saturating_sub(1)).rev() {
                        if is_sel(&elements[i]) && !is_sel(&elements[i + 1]) {
                            elements.swap(i, i + 1);
                        }
                    }
                }
                ZOrderOp::Backward => {
                    let n = elements.len();
                    for i in 1..n {
                        if is_sel(&elements[i]) && !is_sel(&elements[i - 1]) {
                            elements.swap(i, i - 1);
                        }
                    }
                }
            }
        }
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    /// Handle Escape on the canvas: reset to the Select tool, then clear
    /// selection. Returns whether anything was handled.
    pub(crate) fn whiteboard_escape(&mut self) -> bool {
        if let Some(state) = self.active_whiteboard_mut() {
            if state.tool != WhiteboardTool::Select {
                state.tool = WhiteboardTool::Select;
                return true;
            }
            if !state.selected_ids.is_empty() {
                state.selected_ids.clear();
                return true;
            }
        }
        false
    }

    /// Select the element under the pointer (used when opening the right-click
    /// menu over an unselected element). Returns whether there is a selection.
    pub(crate) fn whiteboard_select_at(&mut self, position: Point<gpui::Pixels>) -> bool {
        let Some(view) = self.whiteboard_canvas_view() else {
            return false;
        };
        let canvas = view.to_canvas(screen_pt(position));
        let tolerance = 6.0 / view.zoom;
        if let Some(id) = self.whiteboard_pick_element(canvas, tolerance) {
            let already = self
                .active_whiteboard()
                .map(|state| state.is_selected(&id))
                .unwrap_or(false);
            if !already && let Some(state) = self.active_whiteboard_mut() {
                state.selected_ids = vec![id];
            }
        }
        self.active_whiteboard()
            .map(|state| !state.selected_ids.is_empty())
            .unwrap_or(false)
    }

    fn whiteboard_begin_create(&mut self, kind: ElementKind, canvas: Pt, cx: &mut Context<Self>) {
        let style = self
            .active_whiteboard()
            .map(|state| state.default_style.clone())
            .unwrap_or_default();
        let element = WhiteboardElement::new(kind, canvas.0, canvas.1, 0.0, 0.0, style);
        let id = element.id.clone();
        self.whiteboard_push_undo();
        if let Some(state) = self.active_whiteboard_mut() {
            state.document.elements.push(element);
            state.selected_ids = vec![id.clone()];
        }
        self.whiteboard_drag = Some(WhiteboardDrag::Create { id, start: canvas });
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    fn whiteboard_create_text(&mut self, canvas: Pt, window: &mut Window, cx: &mut Context<Self>) {
        let style = self
            .active_whiteboard()
            .map(|state| state.default_style.clone())
            .unwrap_or_default();
        let line_height = style.font.size.px() * 1.4;
        let mut element = WhiteboardElement::new(
            ElementKind::Text,
            canvas.0,
            canvas.1,
            200.0,
            line_height,
            style,
        );
        element.text = String::new();
        let id = element.id.clone();
        self.whiteboard_push_undo();
        if let Some(state) = self.active_whiteboard_mut() {
            state.document.elements.push(element);
            state.selected_ids = vec![id.clone()];
            state.tool = WhiteboardTool::Select;
        }
        self.mark_whiteboard_dirty();
        self.whiteboard_begin_text_edit(id, true, window, cx);
    }

    // ── Inline text editing ──────────────────────────────────────────────────

    pub(crate) fn whiteboard_begin_text_edit(
        &mut self,
        element_id: String,
        created: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.active_whiteboard() else {
            return;
        };
        let Some(element) = state.document.element(&element_id) else {
            return;
        };
        let original_text = element.text.clone();
        // The edit overlay mirrors the rendered alignment of the label/text.
        let align = element.font.align;
        let zoom = state.document.app_state.zoom.clamp(0.05, 40.0);
        let font_px = (element.font.size.px() * zoom).max(8.0);
        let color = element
            .font
            .effective_color(element.stroke.color)
            .resolve_stroke(&self.theme);

        if !created {
            self.whiteboard_push_undo();
        }

        let styles = self.whiteboard_text_input_styles(font_px, color, align);
        let seed = original_text.clone();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(styles, "", cx);
            input.multiline = true;
            input.set_text(&seed, cx);
            input.select_all(cx);
            input
        });
        let changed = cx.subscribe(
            &input,
            |this, _input, event: &TextInputEvent, cx| match event {
                TextInputEvent::Changed => this.whiteboard_sync_text_edit(cx),
                TextInputEvent::Submit | TextInputEvent::SubmitBackward => {
                    this.whiteboard_commit_text_edit(false, cx)
                }
                TextInputEvent::Escape => this.whiteboard_commit_text_edit(true, cx),
                _ => {}
            },
        );
        let focus_handle = input.read(cx).focus_handle.clone();
        let blur = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.whiteboard_commit_text_edit(false, cx);
        });
        window.focus(&focus_handle);

        self.whiteboard_text_editor = Some(WhiteboardTextEditor {
            element_id,
            input,
            original_text,
            _changed: changed,
            _blur: blur,
        });
        cx.notify();
    }

    fn whiteboard_sync_text_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.whiteboard_text_editor.as_ref() else {
            return;
        };
        let id = editor.element_id.clone();
        let text = editor.input.read(cx).text().to_string();
        if let Some(state) = self.active_whiteboard_mut()
            && let Some(index) = state.document.element_index(&id)
        {
            state.document.elements[index].text = text;
        }
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    pub(crate) fn whiteboard_commit_text_edit(&mut self, cancel: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.whiteboard_text_editor.take() else {
            return;
        };
        let id = editor.element_id;
        let final_text = if cancel {
            editor.original_text
        } else {
            editor.input.read(cx).text().to_string()
        };
        if let Some(state) = self.active_whiteboard_mut()
            && let Some(index) = state.document.element_index(&id)
        {
            let element = &mut state.document.elements[index];
            element.text = final_text;
            if element.kind == ElementKind::Text && element.text.trim().is_empty() {
                state.document.elements.remove(index);
                state.selected_ids.retain(|selected| selected != &id);
            }
        }
        self.mark_whiteboard_dirty();
        cx.notify();
    }

    fn whiteboard_text_input_styles(
        &self,
        font_px: f32,
        color: Hsla,
        align: CanvasTextAlign,
    ) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        styles.text_input_height_px = font_px * 1.5;
        styles.text_input_padding_left_px = 2.0;
        styles.text_input_padding_right_px = 2.0;
        styles.text_input_font_size_px = font_px;
        styles.text_input_bg = gpui::transparent_black();
        styles.text_input_focus_bg = gpui::transparent_black();
        styles.text_input_text_color = color;
        styles.text_input_border_width_px = 0.0;
        styles.text_input_border_color = gpui::transparent_black();
        styles.text_input_focus_border_color = gpui::transparent_black();
        styles.text_input_radius_px = 0.0;
        styles.text_input_cursor_color = self.theme.text_accent;
        styles.text_input_center = align == CanvasTextAlign::Center;
        styles.text_input_right = align == CanvasTextAlign::Right;
        styles
    }

    /// Pane-local rectangle for the inline text editor overlay, if active.
    /// Labels (shapes and linear elements) are centered on the label anchor;
    /// standalone text anchors at the element's top-left. Coordinates are
    /// relative to the canvas element (its parent positions the overlay).
    pub(crate) fn whiteboard_text_overlay_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let editor = self.whiteboard_text_editor.as_ref()?;
        let view = self.whiteboard_canvas_view()?;
        let canvas_bounds = self.whiteboard_canvas_bounds?;
        let pane_x: f32 = canvas_bounds.origin.x.into();
        let pane_y: f32 = canvas_bounds.origin.y.into();
        let state = self.active_whiteboard()?;
        let element = state.document.element(&editor.element_id)?;
        let screen: Vec<Pt> = element_corners(element)
            .iter()
            .map(|corner| view.to_screen(*corner))
            .collect();
        let (min, max) = bounds_of(&screen)?;
        let line_height = element.font.size.px() * view.zoom * 1.5;
        let line_count = element.text.split('\n').count().max(1) as f32;
        let text_height = line_height * line_count;
        if element.kind.supports_label() {
            // Size the overlay to the estimated text block (same heuristic as
            // `label_hit_test`), centered on the label anchor, so the input's
            // alignment matches the rendered block and the text stays put.
            let font_px = element.font.size.px() * view.zoom;
            let mut block_w = 0.0_f32;
            for line in element.text.split('\n') {
                let w: f32 = line
                    .chars()
                    .map(|ch| {
                        if ch.is_ascii() {
                            font_px * 0.55
                        } else {
                            font_px
                        }
                    })
                    .sum();
                block_w = block_w.max(w);
            }
            let width = (block_w + 8.0).max(40.0);
            let (center_x, center_y) = view.to_screen(label_center(element));
            let top = center_y - text_height * 0.5;
            Some((
                center_x - width * 0.5 - pane_x,
                top - pane_y,
                width,
                text_height,
            ))
        } else {
            let width = (max.0 - min.0).max(120.0);
            Some((min.0 - pane_x, min.1 - pane_y, width, text_height))
        }
    }

    pub(crate) fn whiteboard_editing_element_id(&self) -> Option<String> {
        self.whiteboard_text_editor
            .as_ref()
            .map(|editor| editor.element_id.clone())
    }

    /// Element ids to outline on hover (expanded to the hovered element's
    /// outermost group). Empty while dragging/editing, when nothing is under
    /// the pointer, or when the hovered unit is already fully selected.
    pub(crate) fn whiteboard_hover_highlight_ids(&self, view: &CanvasView) -> Vec<String> {
        if self.whiteboard_drag.is_some() || self.whiteboard_text_editor.is_some() {
            return Vec::new();
        }
        let Some(pointer) = self.whiteboard_pointer else {
            return Vec::new();
        };
        let Some(state) = self.active_whiteboard() else {
            return Vec::new();
        };
        if state.tool != WhiteboardTool::Select {
            return Vec::new();
        }
        let canvas = view.to_canvas(screen_pt(pointer));
        let tolerance = 6.0 / view.zoom;
        let mut ids: Vec<String> = Vec::new();
        if let Some(id) = self.whiteboard_pick_element(canvas, tolerance) {
            ids.push(id.clone());
            if let Some(element) = state.document.element(&id)
                && let Some(group) = element.outermost_group()
            {
                for other in &state.document.elements {
                    if other.id != id && other.outermost_group() == Some(group) {
                        ids.push(other.id.clone());
                    }
                }
            }
        } else if let Some(members) = self.whiteboard_pick_group_at(canvas) {
            ids = members;
        }
        if ids.is_empty() {
            return Vec::new();
        }
        // Already selected as a whole: the selection visuals cover it.
        if ids
            .iter()
            .all(|id| state.selected_ids.iter().any(|sel| sel == id))
        {
            return Vec::new();
        }
        ids
    }

    fn whiteboard_pick_element(&self, canvas: Pt, tolerance: f32) -> Option<String> {
        let state = self.active_whiteboard()?;
        state
            .document
            .elements
            .iter()
            .rev()
            .find(|element| hit_test(element, canvas, tolerance) || label_hit_test(element, canvas))
            .map(|element| element.id.clone())
    }

    /// All member ids of the topmost group whose combined bounding box contains
    /// `canvas` (clicking anywhere inside a group's rectangle selects it).
    fn whiteboard_pick_group_at(&self, canvas: Pt) -> Option<Vec<String>> {
        let state = self.active_whiteboard()?;
        let mut seen: Vec<&str> = Vec::new();
        for element in state.document.elements.iter().rev() {
            let Some(group) = element.outermost_group() else {
                continue;
            };
            if seen.contains(&group) {
                continue;
            }
            seen.push(group);
            let members = state
                .document
                .elements
                .iter()
                .filter(|other| other.outermost_group() == Some(group));
            if let Some((min, max)) = selection_bbox(members)
                && canvas.0 >= min.0
                && canvas.0 <= max.0
                && canvas.1 >= min.1
                && canvas.1 <= max.1
            {
                return Some(
                    state
                        .document
                        .elements
                        .iter()
                        .filter(|other| other.outermost_group() == Some(group))
                        .map(|other| other.id.clone())
                        .collect(),
                );
            }
        }
        None
    }

    /// Topmost shape (rectangle/ellipse/text) under `canvas`, excluding linear
    /// elements and `exclude` — used as a magnetic bind target.
    fn whiteboard_element_at(&self, canvas: Pt, exclude: Option<&str>) -> Option<String> {
        let state = self.active_whiteboard()?;
        state
            .document
            .elements
            .iter()
            .rev()
            .find(|element| {
                !element.kind.is_linear()
                    && exclude != Some(element.id.as_str())
                    && hit_test(element, canvas, 0.0)
            })
            .map(|element| element.id.clone())
    }

    /// Recompute the endpoints of every magnetically bound linear element so
    /// they stay attached to their target's edge as elements move/resize.
    pub(crate) fn whiteboard_resolve_bindings(&mut self) {
        let Some(state) = self.active_whiteboard_mut() else {
            return;
        };
        let doc = &mut state.document;
        // Snapshot positions so we can read targets while mutating linears.
        let snapshot = doc.elements.clone();
        let find = |id: &str| snapshot.iter().find(|e| e.id == id);
        for element in doc.elements.iter_mut() {
            if !element.kind.is_linear() {
                continue;
            }
            // Drop bindings whose target was deleted.
            if let Some(id) = element.start_binding.as_deref()
                && find(id).is_none()
            {
                element.start_binding = None;
            }
            if let Some(id) = element.end_binding.as_deref()
                && find(id).is_none()
            {
                element.end_binding = None;
            }
            if element.start_binding.is_none() && element.end_binding.is_none() {
                continue;
            }
            // Endpoints currently (ignore rotation for bound linears).
            let mut start = (element.x, element.y);
            let mut end = (element.x + element.width, element.y + element.height);
            if let Some(target_id) = element.start_binding.as_deref()
                && let Some(target) = find(target_id)
            {
                start = element_edge_point(target, end);
            }
            if let Some(target_id) = element.end_binding.as_deref()
                && let Some(target) = find(target_id)
            {
                end = element_edge_point(target, start);
            }
            // Re-resolve start against the updated end for a tighter fit.
            if let Some(target_id) = element.start_binding.as_deref()
                && let Some(target) = find(target_id)
            {
                start = element_edge_point(target, end);
            }
            element.rotation = 0.0;
            element.x = start.0;
            element.y = start.1;
            element.width = end.0 - start.0;
            element.height = end.1 - start.1;
        }
    }

    /// If the pointer is on a transform handle of the current selection,
    /// return the drag to begin.
    fn whiteboard_hit_handle(&self, view: &CanvasView, screen: Pt) -> Option<WhiteboardDrag> {
        let state = self.active_whiteboard()?;

        // Multi-selection: handles on the combined bounding box.
        if state.selected_ids.len() > 1 {
            let (min, max) = selection_bbox(state.selected_elements())?;
            let smin = view.to_screen(min);
            let smax = view.to_screen(max);
            let points = bbox_handle_points(smin, smax);
            let originals: Vec<(String, f32, f32, f32, f32)> = state
                .selected_elements()
                .map(|e| (e.id.clone(), e.x, e.y, e.width, e.height))
                .collect();
            let canvas_points = bbox_handle_points(min, max);
            for (i, handle) in ResizeHandle::ALL.iter().enumerate() {
                if dist(screen, points[i]) <= HANDLE_HIT_RADIUS {
                    return Some(WhiteboardDrag::GroupResize {
                        handle: *handle,
                        anchor: bbox_handle_anchor(min, max, i),
                        handle_origin: canvas_points[i],
                        originals,
                    });
                }
            }
            // Rotation ring outside the bbox corners.
            for corner in points.iter().take(4) {
                let d = dist(screen, *corner);
                if d > HANDLE_HIT_RADIUS && d <= HANDLE_HIT_RADIUS + ROTATE_RING_WIDTH {
                    let center = ((min.0 + max.0) * 0.5, (min.1 + max.1) * 0.5);
                    let center_screen = view.to_screen(center);
                    let start_pointer =
                        (screen.1 - center_screen.1).atan2(screen.0 - center_screen.0);
                    let originals = state
                        .selected_elements()
                        .map(|e| (e.id.clone(), e.x, e.y, e.width, e.height, e.rotation))
                        .collect();
                    return Some(WhiteboardDrag::GroupRotate {
                        center,
                        start_pointer,
                        originals,
                    });
                }
            }
            return None;
        }

        if state.selected_ids.len() != 1 {
            return None;
        }
        let id = &state.selected_ids[0];
        let element = state.document.element(id)?;

        if element.kind.is_linear() {
            let (a, b) = linear_endpoints(element);
            if dist(screen, view.to_screen(a)) <= HANDLE_HIT_RADIUS {
                return Some(WhiteboardDrag::MoveEndpoint {
                    id: id.clone(),
                    end: false,
                });
            }
            if dist(screen, view.to_screen(b)) <= HANDLE_HIT_RADIUS {
                return Some(WhiteboardDrag::MoveEndpoint {
                    id: id.clone(),
                    end: true,
                });
            }
            for (handle, position) in route_handles(element, view) {
                if dist(screen, position) <= HANDLE_HIT_RADIUS {
                    return Some(WhiteboardDrag::RouteWaypoint {
                        id: id.clone(),
                        handle,
                    });
                }
            }
            return None;
        }

        let points = resize_handle_points(element, view);
        for (i, handle) in ResizeHandle::ALL.iter().enumerate() {
            if dist(screen, points[i]) <= HANDLE_HIT_RADIUS {
                let anchor = handle_anchor(element, *handle);
                let aspect = if element.height.abs() > 1e-3 {
                    element.width.abs() / element.height.abs()
                } else {
                    1.0
                };
                return Some(WhiteboardDrag::Resize {
                    id: id.clone(),
                    handle: *handle,
                    anchor,
                    rotation: element.rotation,
                    aspect,
                });
            }
        }

        // Rotation ring just outside the corners.
        for corner in points.iter().take(4) {
            let d = dist(screen, *corner);
            if d > HANDLE_HIT_RADIUS && d <= HANDLE_HIT_RADIUS + ROTATE_RING_WIDTH {
                let center = element.center();
                let center_screen = view.to_screen(center);
                let start_pointer = (screen.1 - center_screen.1).atan2(screen.0 - center_screen.0);
                return Some(WhiteboardDrag::Rotate {
                    id: id.clone(),
                    center,
                    start_pointer,
                    start_rotation: element.rotation,
                });
            }
        }
        None
    }

    /// The cursor to show given the current drag/hover state over the canvas.
    pub(crate) fn whiteboard_cursor_at(&self, view: &CanvasView) -> CursorStyle {
        // An in-progress resize dictates the cursor; everything else uses the
        // default arrow (rotation shows the custom glyph instead).
        if let Some(drag) = self.whiteboard_drag.as_ref() {
            return match drag {
                WhiteboardDrag::Resize {
                    handle, rotation, ..
                } => resize_cursor(*handle, *rotation),
                WhiteboardDrag::GroupResize { handle, .. } => resize_cursor(*handle, 0.0),
                _ => CursorStyle::Arrow,
            };
        }

        let Some(pointer) = self.whiteboard_pointer else {
            return CursorStyle::Arrow;
        };
        let screen = screen_pt(pointer);

        // Hovering a resize handle of the single selection.
        if let Some(state) = self.active_whiteboard()
            && state.selected_ids.len() == 1
            && let Some(element) = state.document.element(&state.selected_ids[0])
            && !element.kind.is_linear()
        {
            let points = resize_handle_points(element, view);
            for (i, handle) in ResizeHandle::ALL.iter().enumerate() {
                if dist(screen, points[i]) <= HANDLE_HIT_RADIUS {
                    return resize_cursor(*handle, element.rotation);
                }
            }
        }

        // Hovering a handle on the multi-selection bounding box.
        if let Some(state) = self.active_whiteboard()
            && state.selected_ids.len() > 1
            && let Some((min, max)) = selection_bbox(state.selected_elements())
        {
            let smin = view.to_screen(min);
            let smax = view.to_screen(max);
            let points = bbox_handle_points(smin, smax);
            for (i, handle) in ResizeHandle::ALL.iter().enumerate() {
                if dist(screen, points[i]) <= HANDLE_HIT_RADIUS {
                    return resize_cursor(*handle, 0.0);
                }
            }
        }

        CursorStyle::Arrow
    }

    /// When hovering/dragging a rotation affordance, the screen-space center and
    /// orientation (radians) of the rotate glyph to draw. gpui has no rotate
    /// cursor, so we render one ourselves.
    pub(crate) fn whiteboard_rotate_glyph(&self, view: &CanvasView) -> Option<(Pt, f32)> {
        let pointer = screen_pt(self.whiteboard_pointer?);

        // Active rotate drag: orient toward the current pointer.
        match self.whiteboard_drag.as_ref() {
            Some(WhiteboardDrag::Rotate { center, .. })
            | Some(WhiteboardDrag::GroupRotate { center, .. }) => {
                let c = view.to_screen(*center);
                let orient = (pointer.1 - c.1).atan2(pointer.0 - c.0);
                return Some((pointer, orient));
            }
            Some(_) => return None,
            None => {}
        }

        // Hovering a rotation ring.
        let state = self.active_whiteboard()?;
        if state.tool != WhiteboardTool::Select {
            return None;
        }

        // Multi-selection: ring outside the combined bbox corners.
        if state.selected_ids.len() > 1 {
            let (min, max) = selection_bbox(state.selected_elements())?;
            let smin = view.to_screen(min);
            let smax = view.to_screen(max);
            let points = bbox_handle_points(smin, smax);
            for point in &points {
                if dist(pointer, *point) <= HANDLE_HIT_RADIUS {
                    return None;
                }
            }
            let center = ((smin.0 + smax.0) * 0.5, (smin.1 + smax.1) * 0.5);
            for corner in points.iter().take(4) {
                let d = dist(pointer, *corner);
                if d > HANDLE_HIT_RADIUS && d <= HANDLE_HIT_RADIUS + ROTATE_RING_WIDTH {
                    let orient = (corner.1 - center.1).atan2(corner.0 - center.0);
                    return Some((pointer, orient));
                }
            }
            return None;
        }

        if state.selected_ids.len() != 1 {
            return None;
        }
        let element = state.document.element(&state.selected_ids[0])?;
        if element.kind.is_linear() {
            return None;
        }
        let points = resize_handle_points(element, view);
        for point in &points {
            if dist(pointer, *point) <= HANDLE_HIT_RADIUS {
                return None;
            }
        }
        let center = view.to_screen(element.center());
        for corner in points.iter().take(4) {
            let d = dist(pointer, *corner);
            if d > HANDLE_HIT_RADIUS && d <= HANDLE_HIT_RADIUS + ROTATE_RING_WIDTH {
                let orient = (corner.1 - center.1).atan2(corner.0 - center.0);
                return Some((pointer, orient));
            }
        }
        None
    }
}

/// Map a resize handle (with the element's rotation applied) to the nearest
/// directional resize cursor.
fn resize_cursor(handle: ResizeHandle, rotation: f32) -> CursorStyle {
    use std::f32::consts::PI;
    // Base outward normal angle of each handle (screen space, y-down, 0 = right).
    let base = match handle {
        ResizeHandle::Right => 0.0,
        ResizeHandle::BottomRight => PI * 0.25,
        ResizeHandle::Bottom => PI * 0.5,
        ResizeHandle::BottomLeft => PI * 0.75,
        ResizeHandle::Left => PI,
        ResizeHandle::TopLeft => PI * 1.25,
        ResizeHandle::Top => PI * 1.5,
        ResizeHandle::TopRight => PI * 1.75,
    };
    let mut angle = (base + rotation) % PI;
    if angle < 0.0 {
        angle += PI;
    }
    // Fold [0, PI) into four 45°-wide sectors centered on the axes/diagonals.
    let sector = (angle / (PI / 4.0)).round() as i32 % 4;
    match sector {
        0 => CursorStyle::ResizeLeftRight,       // ─ (E/W)
        1 => CursorStyle::ResizeUpLeftDownRight, // ╲ (NW/SE)
        2 => CursorStyle::ResizeUpDown,          // │ (N/S)
        _ => CursorStyle::ResizeUpRightDownLeft, // ╱ (NE/SW)
    }
}

/// The fixed (anchor) canvas point opposite a resize handle.
fn handle_anchor(element: &WhiteboardElement, handle: ResizeHandle) -> Pt {
    let corners = element_corners(element);
    let mid = |a: Pt, b: Pt| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    match handle {
        ResizeHandle::TopLeft => corners[2],
        ResizeHandle::TopRight => corners[3],
        ResizeHandle::BottomRight => corners[0],
        ResizeHandle::BottomLeft => corners[1],
        ResizeHandle::Top => mid(corners[2], corners[3]),
        ResizeHandle::Bottom => mid(corners[0], corners[1]),
        ResizeHandle::Left => mid(corners[1], corners[2]),
        ResizeHandle::Right => mid(corners[3], corners[0]),
    }
}

/// Resize `element` so the dragged `handle` follows `pointer` (canvas space)
/// while the opposite `anchor` point stays fixed, honoring `rotation`. When
/// `keep_aspect` is set, corner drags preserve the original `aspect` ratio.
fn apply_resize(
    element: &mut WhiteboardElement,
    handle: ResizeHandle,
    anchor: Pt,
    rotation: f32,
    aspect: f32,
    pointer: Pt,
    keep_aspect: bool,
) {
    let (sin, cos) = rotation.sin_cos();
    // Express the pointer offset from the anchor in the element's local frame.
    let dxw = pointer.0 - anchor.0;
    let dyw = pointer.1 - anchor.1;
    let local_x = dxw * cos + dyw * sin;
    let local_y = -dxw * sin + dyw * cos;

    let (new_w, new_h, offset) = match handle {
        ResizeHandle::TopLeft
        | ResizeHandle::TopRight
        | ResizeHandle::BottomRight
        | ResizeHandle::BottomLeft => {
            let mut w = local_x.abs().max(MIN_SIZE);
            let mut h = local_y.abs().max(MIN_SIZE);
            if keep_aspect && aspect > 1e-3 {
                // Lock to the original ratio using the larger drag axis.
                if w / aspect >= h {
                    h = w / aspect;
                } else {
                    w = h * aspect;
                }
            }
            let sx = if local_x >= 0.0 { 1.0 } else { -1.0 };
            let sy = if local_y >= 0.0 { 1.0 } else { -1.0 };
            (w, h, (sx * w * 0.5, sy * h * 0.5))
        }
        ResizeHandle::Left | ResizeHandle::Right => {
            let w = local_x.abs().max(MIN_SIZE);
            let h = element.height.abs().max(MIN_SIZE);
            let sx = if local_x >= 0.0 { 1.0 } else { -1.0 };
            (w, h, (sx * w * 0.5, 0.0))
        }
        ResizeHandle::Top | ResizeHandle::Bottom => {
            let w = element.width.abs().max(MIN_SIZE);
            let h = local_y.abs().max(MIN_SIZE);
            let sy = if local_y >= 0.0 { 1.0 } else { -1.0 };
            (w, h, (0.0, sy * h * 0.5))
        }
    };

    // New center = anchor + R(rotation) * offset.
    let center_x = anchor.0 + offset.0 * cos - offset.1 * sin;
    let center_y = anchor.1 + offset.0 * sin + offset.1 * cos;
    element.width = new_w;
    element.height = new_h;
    element.x = center_x - new_w * 0.5;
    element.y = center_y - new_h * 0.5;
}

/// Flip negative width/height into a positive box (non-linear shapes only;
/// linear elements keep their start→end direction so the arrowhead stays put).
fn normalize_box(element: &mut WhiteboardElement) {
    if element.kind.is_linear() {
        return;
    }
    if element.width < 0.0 {
        element.x += element.width;
        element.width = -element.width;
    }
    if element.height < 0.0 {
        element.y += element.height;
        element.height = -element.height;
    }
}

/// Constrain a shape to a square (or a 45°-snapped line) while Shift is held.
fn constrain_proportions(element: &mut WhiteboardElement) {
    if element.kind.is_linear() {
        let len = element.width.hypot(element.height);
        let angle = element.height.atan2(element.width);
        let step = std::f32::consts::FRAC_PI_4;
        let snapped = (angle / step).round() * step;
        element.width = len * snapped.cos();
        element.height = len * snapped.sin();
    } else {
        let size = element.width.abs().max(element.height.abs());
        element.width = size.copysign(element.width);
        element.height = size.copysign(element.height);
    }
}
