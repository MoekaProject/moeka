//! Split-pane rendering: the recursive pane layout tree, per-pane leaf chrome
//! (tab bar + content), divider resize handles, and the per-pane editor pane
//! body extracted from `render.rs`.

use crate::ui::self_drawn::{DropIndicatorControl, ResizeHandleControl, ScrollbarControl};

use gpui::{
    AnyElement, Bounds, CursorStyle, ExternalPaths, MouseButton, MouseDownEvent, MouseMoveEvent,
    Point, ScrollWheelEvent, SharedString, Window, canvas, deferred, div, prelude::*, px, relative,
};

use super::tooltip::toolbar_anchor_bounds;
use super::*;

/// Minimum pane size (px, unscaled) enforced while dragging a divider.
const MIN_PANE_MAIN_AXIS_PX: f32 = 140.0;

/// Normalized half-size of the central "merge" zone. Outside it, the pane is
/// partitioned diagonally into four half-pane split zones, so e.g. anywhere in
/// the right half (outside the center box) splits right.
const SPLIT_DROP_CENTER_FRACTION: f32 = 0.2;

/// Computes the split direction for a tab dragged over a pane's content area.
/// The pane is split into a small central merge zone and four half-pane edge
/// zones (nearest edge in normalized coordinates, i.e. quadrants along the
/// diagonals).
fn split_drop_direction(
    bounds: Bounds<gpui::Pixels>,
    position: Point<gpui::Pixels>,
) -> Option<SplitDirection> {
    let width = f32::from(bounds.size.width).max(1.0);
    let height = f32::from(bounds.size.height).max(1.0);
    let fx = ((f32::from(position.x) - f32::from(bounds.origin.x)) / width).clamp(0.0, 1.0);
    let fy = ((f32::from(position.y) - f32::from(bounds.origin.y)) / height).clamp(0.0, 1.0);
    let center = SPLIT_DROP_CENTER_FRACTION;
    if (fx - 0.5).abs() <= center && (fy - 0.5).abs() <= center {
        return None;
    }
    let candidates = [
        (SplitDirection::Up, fy),
        (SplitDirection::Right, 1.0 - fx),
        (SplitDirection::Down, 1.0 - fy),
        (SplitDirection::Left, fx),
    ];
    candidates
        .into_iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(direction, _)| direction)
}

/// How far (px) outside every pane a dragged tab may be while still snapping
/// to the nearest pane's edge. Dropping right at a window/pane border often
/// lands the cursor a few pixels outside the recorded bounds.
const SPLIT_DROP_SNAP_TOLERANCE_PX: f32 = 48.0;

impl EditorView {
    fn ensure_editor_pane_views(
        &mut self,
        pane_id: PaneId,
        cx: &mut Context<Self>,
    ) -> (gpui::AnyView, gpui::Entity<super::caret::CaretOverlayView>) {
        if !self.pane_editor_content_views.contains_key(&pane_id) {
            let geometry = std::rc::Rc::new(std::cell::RefCell::new(
                super::caret::CaretGeometrySnapshot::default(),
            ));
            let editor = cx.entity().clone();
            let content_geometry = geometry.clone();
            let overlay_editor = editor.clone();
            let content = cx.new(move |cx| {
                super::caret::EditorContentView::new(editor, pane_id, content_geometry, cx)
            });
            let config = self.caret_animation_config();
            let overlay = cx.new(move |_| {
                super::caret::CaretOverlayView::new(overlay_editor, geometry, config)
            });
            self.pane_editor_content_views.insert(pane_id, content);
            self.pane_caret_overlay_views.insert(pane_id, overlay);
        }

        let content = self
            .pane_editor_content_views
            .get(&pane_id)
            .expect("editor content view inserted")
            .clone();
        let overlay = self
            .pane_caret_overlay_views
            .get(&pane_id)
            .expect("caret overlay view inserted")
            .clone();
        let content = gpui::AnyView::from(content);
        // Keep ordinary caret-only frames cached, but prepaint focused Markdown
        // selection and line-format-trigger frames.
        let content = if self.table_handles.needs_render()
            || self.hovering_handle.is_some()
            || !self.table_border_segment_areas.is_empty()
            || (pane_id == self.focused_pane_id
                && self.active_tab_is_markdown()
                && (self.cursor.has_selection()
                    || self.hovering_format_line.is_some()
                    || self.line_drag.is_some()
                    || self.line_drop_flash.is_some()
                    || self.line_format_toolbar_line.is_some()))
        {
            content
        } else {
            content.cached(gpui::StyleRefinement::default().size_full())
        };
        (content, overlay)
    }

    fn caret_animation_config(&self) -> super::caret::CaretAnimationConfig {
        super::caret::CaretAnimationConfig {
            pause: self.styles.cursor.blink_pause_delay(),
            visible: self.styles.cursor.blink_visible_duration(),
            transparent: self.styles.cursor.blink_transparent_duration(),
            min_opacity: self.styles.cursor.cursor_opacity_min,
            max_opacity: self.styles.cursor.cursor_opacity_max,
        }
    }

    /// Resolves which pane (and which split edge, None = merge) a dragged tab
    /// at `position` is targeting. Called from the drag source's
    /// `on_drag_move` handler, which is the only listener guaranteed to see
    /// every move (capture-phase propagation is stopped there), so it
    /// hit-tests the recorded pane bounds instead of relying on per-pane drag
    /// listeners. Positions over a pane's tab bar yield no target (tab-bar
    /// drops reorder/merge instead).
    pub(super) fn split_drop_target_at(
        &self,
        position: Point<gpui::Pixels>,
    ) -> Option<(PaneId, Option<SplitDirection>)> {
        let tab_bar_height = self.tab_bar_height();
        let content_bounds_for = |pane_id: PaneId| -> Option<Bounds<gpui::Pixels>> {
            let bounds = self.pane_bounds.get(&pane_id)?;
            Some(Bounds::new(
                gpui::point(bounds.origin.x, bounds.origin.y + px(tab_bar_height)),
                gpui::size(
                    bounds.size.width,
                    px((f32::from(bounds.size.height) - tab_bar_height).max(0.0)),
                ),
            ))
        };

        // Exact hit first.
        for pane_id in self.pane_layout.leaf_ids() {
            let Some(bounds) = self.pane_bounds.get(&pane_id) else {
                continue;
            };
            if !bounds.contains(&position) {
                continue;
            }
            // The leaf bounds include the tab bar strip at the top; drops
            // there are handled by the tab bar itself.
            let content_bounds = content_bounds_for(pane_id)?;
            if !content_bounds.contains(&position) {
                return None;
            }
            return Some((pane_id, split_drop_direction(content_bounds, position)));
        }

        // No exact hit: dragging flush against a pane or window border often
        // puts the cursor a few pixels outside the recorded bounds. Snap to
        // the nearest pane's content area within a tolerance and clamp the
        // position into it, so edge drops still resolve to that edge.
        let mut nearest: Option<(PaneId, Bounds<gpui::Pixels>, f32)> = None;
        for pane_id in self.pane_layout.leaf_ids() {
            let Some(content_bounds) = content_bounds_for(pane_id) else {
                continue;
            };
            let left = f32::from(content_bounds.origin.x);
            let top = f32::from(content_bounds.origin.y);
            let right = left + f32::from(content_bounds.size.width);
            let bottom = top + f32::from(content_bounds.size.height);
            let x = f32::from(position.x);
            let y = f32::from(position.y);
            let dx = (left - x).max(0.0).max(x - right);
            let dy = (top - y).max(0.0).max(y - bottom);
            let distance = dx.hypot(dy);
            if distance <= SPLIT_DROP_SNAP_TOLERANCE_PX
                && nearest.as_ref().is_none_or(|(_, _, best)| distance < *best)
            {
                nearest = Some((pane_id, content_bounds, distance));
            }
        }
        let (pane_id, content_bounds, _) = nearest?;
        let left = f32::from(content_bounds.origin.x);
        let top = f32::from(content_bounds.origin.y);
        let right = left + f32::from(content_bounds.size.width);
        let bottom = top + f32::from(content_bounds.size.height);
        let clamped = gpui::point(
            px(f32::from(position.x).clamp(left, right - 1.0)),
            px(f32::from(position.y).clamp(top, bottom - 1.0)),
        );
        Some((pane_id, split_drop_direction(content_bounds, clamped)))
    }
}

impl EditorView {
    pub(super) fn render_workspace_content_column(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = self.pane_layout.clone();
        div()
            .id("workspace-content-column")
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .id("workspace-content-row")
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .child(self.render_pane_layout(&layout, &mut Vec::new(), window, cx)),
            )
            .when(!self.active_tab_is_settings(), |this| {
                this.child(self.render_status_bar(cx))
            })
            .into_any_element()
    }

    fn render_pane_layout(
        &mut self,
        layout: &PaneLayout,
        path: &mut Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match layout {
            PaneLayout::Leaf(pane_id) => self.render_pane_leaf(*pane_id, window, cx),
            PaneLayout::Split {
                axis,
                children,
                flexes,
            } => {
                let axis = *axis;
                let total: f32 = flexes.iter().sum::<f32>().max(f32::EPSILON);
                let container = div()
                    .id(SharedString::from(format!("pane-split-{path:?}")))
                    .flex_1()
                    .size_full()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .flex();
                let mut element = match axis {
                    SplitAxis::Horizontal => container.flex_row(),
                    SplitAxis::Vertical => container.flex_col(),
                };
                let child_count = children.len();
                let divider_thickness = self.pane_divider_thickness();
                for (index, child) in children.iter().enumerate() {
                    path.push(index);
                    let child_element = self.render_pane_layout(child, path, window, cx);
                    path.pop();
                    let fraction = flexes.get(index).copied().unwrap_or(1.0) / total;
                    let is_last = index + 1 == child_count;
                    let wrapper = div().overflow_hidden();
                    // The last child absorbs rounding leftovers (and the space
                    // taken by the solid dividers) so the row always fills
                    // exactly.
                    let wrapper = if is_last {
                        wrapper
                            .flex_1()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .h_full()
                            .w_full()
                    } else {
                        match axis {
                            SplitAxis::Horizontal => {
                                wrapper.flex_none().w(relative(fraction)).h_full()
                            }
                            SplitAxis::Vertical => {
                                wrapper.flex_none().h(relative(fraction)).w_full()
                            }
                        }
                    };
                    element = element.child(wrapper.child(child_element));
                    if !is_last {
                        element = element.child(self.render_pane_divider(
                            axis,
                            path.clone(),
                            index,
                            divider_thickness,
                            cx,
                        ));
                    }
                }
                element.into_any_element()
            }
        }
    }

    /// Divider thickness in logical pixels, snapped to whole device pixels so
    /// the hairline stays crisp on fractional display scales.
    fn pane_divider_thickness(&self) -> f32 {
        ResizeHandleControl::new(
            &self.styles.self_drawn,
            self.scaled(1.0),
            self.last_window_scale_factor,
            false,
        )
        .thickness
    }

    /// A solid divider strip between two split children. It participates in
    /// layout (so neighbouring panes can't paint over it) and carries a wider
    /// absolutely-positioned drag handle straddling the boundary.
    fn render_pane_divider(
        &self,
        axis: SplitAxis,
        path: Vec<usize>,
        divider_index: usize,
        thickness: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_active = self.pane_resize_drag.as_ref().is_some_and(|drag| {
            drag.axis == axis && drag.path == path && drag.divider_index == divider_index
        });
        let control = ResizeHandleControl::new(
            &self.styles.self_drawn,
            self.scaled(1.0),
            self.last_window_scale_factor,
            is_active,
        );
        debug_assert_eq!(control.thickness, thickness);
        let hover_indicator_color = control.active_color;
        let vertical = axis == SplitAxis::Horizontal;
        let anchor = control.anchor(vertical);
        let hit = control.hit(vertical).id(SharedString::from(format!(
            "pane-divider-{path:?}-{divider_index}"
        )));
        let hit = hit
            .when(is_active, |style| style.bg(hover_indicator_color))
            .hover(move |style| style.bg(hover_indicator_color))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.start_pane_resize_drag(path.clone(), divider_index, axis, event.position);
                    cx.notify();
                }),
            );

        anchor.child(hit).into_any_element()
    }

    fn render_pane_leaf(
        &mut self,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.with_pane(pane_id, |this| {
            this.sync_table_source_view();
            let has_tabs = !this.tabs.is_empty();
            let is_settings = has_tabs && this.active_tab_is_settings();
            let editor_entity = cx.entity().clone();
            div()
                .id(SharedString::from(format!("pane-{pane_id}")))
                .relative()
                .size_full()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .overflow_hidden()
                .flex()
                .flex_col()
                .bg(this.styles.editor.editor_bg)
                .child(
                    canvas(
                        move |bounds, _window, cx| {
                            editor_entity.update(cx, |this, _| {
                                this.pane_bounds.insert(pane_id, bounds);
                            });
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .when(has_tabs, |el| el.child(this.render_tab_bar(pane_id, cx)))
                .when(is_settings, |el| {
                    el.child(this.render_settings_panel(window, cx))
                })
                .when(has_tabs && !is_settings, |el| {
                    el.child(this.render_editor_pane(pane_id, window, cx))
                })
                .when(!has_tabs, |el| el.child(div().flex_1()))
                .into_any_element()
        })
    }

    // ── Divider resize drag ─────────────────────────────────────────────

    fn start_pane_resize_drag(
        &mut self,
        path: Vec<usize>,
        divider_index: usize,
        axis: SplitAxis,
        position: Point<Pixels>,
    ) {
        let Some(PaneLayout::Split { flexes, .. }) = self.pane_layout.node_at_path_mut(&path)
        else {
            return;
        };
        if divider_index + 1 >= flexes.len() {
            return;
        }
        let start_flexes = (flexes[divider_index], flexes[divider_index + 1]);

        // Estimate the split container's main-axis extent from the union of
        // its leaves' painted bounds.
        let container_px = {
            let node = self
                .pane_layout
                .node_at_path_mut(&path)
                .expect("checked above");
            let mut min_v = f32::MAX;
            let mut max_v = f32::MIN;
            for leaf in node.leaf_ids() {
                if let Some(bounds) = self.pane_bounds.get(&leaf) {
                    let (start, end) = match axis {
                        SplitAxis::Horizontal => (
                            f32::from(bounds.origin.x),
                            f32::from(bounds.origin.x) + f32::from(bounds.size.width),
                        ),
                        SplitAxis::Vertical => (
                            f32::from(bounds.origin.y),
                            f32::from(bounds.origin.y) + f32::from(bounds.size.height),
                        ),
                    };
                    min_v = min_v.min(start);
                    max_v = max_v.max(end);
                }
            }
            if max_v > min_v { max_v - min_v } else { 1.0 }
        };

        let start_mouse = match axis {
            SplitAxis::Horizontal => f32::from(position.x),
            SplitAxis::Vertical => f32::from(position.y),
        };

        self.pane_resize_drag = Some(PaneResizeDrag {
            path,
            divider_index,
            axis,
            start_mouse,
            start_flexes,
            container_px,
        });
    }

    pub(super) fn update_pane_resize_drag(&mut self, position: Point<Pixels>) {
        let Some(drag) = self.pane_resize_drag.clone() else {
            return;
        };
        let mouse = match drag.axis {
            SplitAxis::Horizontal => f32::from(position.x),
            SplitAxis::Vertical => f32::from(position.y),
        };
        let delta_fraction = (mouse - drag.start_mouse) / drag.container_px.max(1.0);
        let min_fraction = self.scaled(MIN_PANE_MAIN_AXIS_PX) / drag.container_px.max(1.0);
        if let Some(PaneLayout::Split { flexes, .. }) =
            self.pane_layout.node_at_path_mut(&drag.path)
            && drag.divider_index + 1 < flexes.len()
        {
            flexes[drag.divider_index] = drag.start_flexes.0;
            flexes[drag.divider_index + 1] = drag.start_flexes.1;
            PaneLayout::resize_children(flexes, drag.divider_index, delta_fraction, min_fraction);
        }
    }

    // ── Per-pane editor pane (extracted from render.rs) ─────────────────

    fn render_editor_pane(
        &mut self,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = &window;
        let is_focused_pane = pane_id == self.focused_pane_id;
        let renders_document = !self.active_tab_is_terminal()
            && !self.active_tab_is_image()
            && !self.active_tab_is_binary();
        let editor_pane_views = renders_document.then(|| {
            let (content, overlay) = self.ensure_editor_pane_views(pane_id, cx);
            let active = is_focused_pane
                && window.is_window_active()
                && self.focused_sidebar_path.is_none()
                && self.focus_handle.is_focused(window)
                && self.active_tab_supports_text_buffer();
            let blink_epoch = self.blink_epoch;
            let config = self.caret_animation_config();
            overlay.update(cx, |overlay, cx| {
                overlay.sync(active, blink_epoch, config, cx)
            });
            (content, overlay)
        });
        let editor_pane_width = self
            .pane_bounds
            .get(&pane_id)
            .map(|bounds| f32::from(bounds.size.width))
            .unwrap_or_else(|| {
                (self.last_window_width - self.sidebar_width() - self.right_panel_width()).max(1.0)
            });
        let toolbar_base_visible = !self.app_settings.simple_mode
            && !self.tabs.is_empty()
            && !self.active_tab_is_settings()
            && !self.active_tab_is_binary()
            && !self.active_tab_is_terminal();
        let show_editor_toolbar = self.should_show_editor_toolbar(toolbar_base_visible);
        let render_editor_toolbar = is_focused_pane
            && toolbar_base_visible
            && (show_editor_toolbar
                || self.toolbar_opacity_task.is_some()
                || self.toolbar_opacity > self.styles.toolbar.toolbar_hidden_opacity + 0.01);

        div()
            .id(SharedString::from(format!("editor-pane-{pane_id}")))
            .flex_1()
            .w_full()
            .min_h(px(0.0))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("editor-pane-content")
                    .relative()
                    .flex_1()
                    .overflow_hidden()
                    .cursor(
                        if self.active_tab_is_image()
                            || self.active_tab_is_binary()
                            || self.active_tab_is_terminal()
                            || self.hovering_checkbox
                            || self.hovering_fold_line.is_some()
                            || self.hovering_image_edit_line.is_some()
                        {
                            CursorStyle::Arrow
                        } else {
                            CursorStyle::IBeam
                        },
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event, window, cx| {
                            this.focused_sidebar_path = None;
                            this.on_mouse_down(event, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(Self::on_editor_context_menu),
                    )
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_hover(cx.listener(move |this, hovering: &bool, _window, cx| {
                        let hovering = *hovering;
                        this.with_pane(pane_id, |this| {
                            if !hovering {
                                this.clear_table_handle_hover(cx);
                                let had_hover = this.hovered_editor_position.take().is_some();
                                if this.editor_scrollbar_drag.is_none() {
                                    this.set_editor_scrollbar_hovered(false, cx);
                                }
                                this.clear_code_block_horizontal_scrollbar_hover(cx);
                                let toolbar_changed = if this.toolbar_corner_hovered {
                                    this.toolbar_corner_hovered = false;
                                    this.sync_toolbar_opacity(cx);
                                    true
                                } else {
                                    false
                                };
                                this.clear_gutter_fold_hover(cx);
                                this.clear_line_format_hover(cx);
                                if had_hover || toolbar_changed {
                                    cx.notify();
                                }
                            }
                        })
                    }))
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                        this.with_pane(pane_id, |this| this.on_mouse_move(event, window, cx))
                    }))
                    .on_scroll_wheel(cx.listener(
                        move |this, event: &ScrollWheelEvent, window, cx| {
                            this.with_pane(pane_id, |this| this.on_scroll(event, window, cx))
                        },
                    ))
                    .drag_over::<DraggedSidebarItems>(move |style, _, _, _| style)
                    .drag_over::<ExternalPaths>({
                        let file_drop_bg = self.styles.self_drawn.colors.drop_overlay;
                        move |style, _, _, _| style.bg(file_drop_bg)
                    })
                    // NOTE: tab drops on the content area (split/merge) are
                    // handled by a capture-phase window MouseUp handler in
                    // render.rs using `tab_drag_split_target`, which is
                    // resolved in the dragged tab's own on_drag_move handler
                    // (tabs.rs). Element-level drag listeners here are
                    // unreliable: the drag source stops capture propagation on
                    // every move.
                    .on_drop(cx.listener(
                        move |this, dragged: &DraggedSidebarItems, _, cx| {
                            this.clear_sidebar_drag_state(cx);
                            this.focused_sidebar_path = None;
                            this.focus_pane(pane_id, cx);
                            this.insert_sidebar_items_as_wiki_links(&dragged.paths, cx);
                        },
                    ))
                    .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                        this.clear_sidebar_drag_state(cx);
                        this.focus_pane(pane_id, cx);
                        this.import_external_paths_as_attachments(paths.paths(), cx);
                    }))
                    .child(if let Some(terminal_tab_id) = self.active_terminal_tab_id() {
                        self.render_terminal_tab_content(terminal_tab_id, window, cx)
                    } else if self.active_tab_is_image() {
                        image_view::ImageViewElement {
                            editor: cx.entity().clone(),
                            pane_id,
                        }
                        .into_any_element()
                    } else if self.active_tab_is_binary() {
                        self.render_binary_placeholder(cx)
                    } else {
                        editor_pane_views
                            .as_ref()
                            .expect("document pane views initialized")
                            .0
                            .clone()
                            .into_any_element()
                    })
                    .when_some(editor_pane_views.map(|(_, overlay)| overlay), |this, overlay| {
                        this.child(overlay)
                    })
                    .when(renders_document, |this| {
                        // The line toolbar clips to the editor viewport because
                        // deferred draws lose ancestor masks.
                        this.child(
                            deferred(selection_toolbar::LineFormatToolbarElement {
                                editor: cx.entity().clone(),
                                pane_id,
                            })
                            .with_priority(1),
                        )
                    })
                    .when_some(
                        self.code_block_horizontal_scrollbar_overlay_geometry(),
                        |this, geometry| {
                            let control = ScrollbarControl::new(self.styles.self_drawn.scrollbar, self.scaled(1.0));
                            let color = control.color(false, geometry.opacity);
                            this.child(
                                div()
                                    .absolute()
                                    .left(px(geometry.track_left))
                                    .top(px(geometry.track_top))
                                    .w(px(geometry.track_width))
                                    .h(px(geometry.track_height))
                                    .child(
                                        control.thumb(geometry.thumb_width, geometry.thumb_height, color)
                                            .left(px(geometry.thumb_left))
                                            .top(px(
                                                geometry.track_height - geometry.thumb_height,
                                            ))

                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this, event: &MouseDownEvent, _, cx| {
                                                        cx.stop_propagation();
                                                        this.start_code_block_horizontal_scrollbar_drag(
                                                            event.position,
                                                            cx,
                                                        );
                                                    },
                                                ),
                                            ),
                                    ),
                            )
                        },
                    )
                    .when_some(
                        self.editor_scrollbar_thumb_geometry(),
                        |this, (thumb_top, thumb_height)| {
                            let thumb_width = self.scaled(self.editor_scrollbar_width);
                            let hit_width = self.editor_scrollbar_hit_width();
                            let right_margin =
                                self.scaled(self.styles.self_drawn.scrollbar.right_margin_px);
                            let control = ScrollbarControl::new(self.styles.self_drawn.scrollbar, self.scaled(1.0));
                            let thumb_color = control.color(false, self.editor_scrollbar_opacity);
                            this.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right(px(0.0))
                                    .h_full()
                                    .w(px(hit_width + right_margin))
                                    .child(
                                        control.thumb(thumb_width, thumb_height, thumb_color)
                                            .top(px(thumb_top))
                                            .right(px(right_margin))

                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this, event: &MouseDownEvent, _, cx| {
                                                        cx.stop_propagation();
                                                        this.start_editor_scrollbar_drag(
                                                            event.position,
                                                            cx,
                                                        );
                                                    },
                                                ),
                                            ),
                                    ),
                            )
                        },
                    )
                    .when(render_editor_toolbar, |this| {
                        this.child({
                            let primary_hover =
                                self.styles.toolbar.toolbar_primary_hover_opacity;
                            let secondary_hover =
                                self.styles.toolbar.toolbar_secondary_hover_opacity;
                            let toolbar_min_top =
                                self.scaled(self.styles.toolbar.toolbar_offset_top_px);
                            let toolbar_min_left =
                                self.scaled(self.styles.toolbar.toolbar_offset_left_px);
                            let primary_btn_size =
                                self.scaled(self.styles.toolbar.toolbar_primary_btn_size_px);
                            let secondary_btn_size =
                                self.scaled(self.styles.toolbar.toolbar_secondary_btn_size_px);
                            let toolbar_gap = self.scaled(self.styles.toolbar.toolbar_gap_px);
                            let primary_icon_size =
                                self.scaled(self.styles.toolbar.toolbar_primary_icon_size_px);
                            let secondary_icon_size =
                                self.scaled(self.styles.toolbar.toolbar_secondary_icon_size_px);
                            let toolbar_left = toolbar_min_left.round();
                            let toolbar_top = toolbar_min_top.round();
                            let sidebar_anchor =
                                toolbar_anchor_bounds(toolbar_left, toolbar_top, primary_btn_size);
                            let align_anchor = toolbar_anchor_bounds(
                                toolbar_left + primary_btn_size + toolbar_gap,
                                toolbar_top,
                                secondary_btn_size,
                            );
                            let align_tooltip = if self.editor_uses_code_gutter() {
                                self.t(crate::i18n::TextKey::ToolbarWrapTooltip)
                            } else {
                                self.t(crate::i18n::TextKey::ToolbarAlignTooltip)
                            };
                            let align_shortcut = self
                                .editor_uses_code_gutter()
                                .then_some(crate::editor::shortcut::hint("alt-z"))
                                .or(Some(crate::editor::shortcut::hint("secondary-alt-l")));
                            let sidebar_tooltip = if self.workspaces.is_empty() {
                                self.t(crate::i18n::TextKey::OpenFolder)
                            } else {
                                crate::i18n::sidebar_visibility_action_label(
                                    self.ui_language,
                                    self.sidebar_visible,
                                )
                            };
                            let sidebar_shortcut = Some(crate::editor::shortcut::hint("secondary-shift-s"));
                            div()
                                .absolute()
                                .left(px(toolbar_left))
                                .top(px(toolbar_top))
                                .flex()
                                .items_center()
                                .gap(px(toolbar_gap))
                                .opacity(self.toolbar_opacity)
                                .child(self.render_toolbar_button(
                                    "toolbar-sidebar-toggle",
                                    primary_btn_size,
                                    primary_btn_size,
                                    primary_icon_size,
                                    1.0,
                                    primary_hover,
                                    SIDEBAR_TOGGLE_SVG_PATH,
                                    sidebar_anchor,
                                    sidebar_tooltip,
                                    sidebar_shortcut,
                                    cx,
                                    |this, cx| {
                                        if this.workspaces.is_empty() {
                                            this.pick_workspace_folder(cx);
                                        } else {
                                            this.toggle_sidebar_visibility(cx);
                                        }
                                    },
                                ))
                                .when(self.active_tab_supports_text_buffer(), |this| {
                                    this.child(self.render_toolbar_button(
                                        "toolbar-align-toggle",
                                        secondary_btn_size,
                                        secondary_btn_size,
                                        secondary_icon_size,
                                        if if self.source_mode { !self.source_word_wrap } else { self.editor_left_aligned } { 1.0 } else { 0.55 },
                                        secondary_hover,
                                        EDITOR_ALIGN_LEFT_SVG_PATH,
                                        align_anchor,
                                        align_tooltip,
                                        align_shortcut,
                                        cx,
                                        |this: &mut Self, cx| {
                                            this.toggle_editor_alignment(cx);
                                        },
                                    ))
                                })
                        })
                    })
                    .when(render_editor_toolbar, |this| {
                        this.child({
                            let secondary_hover =
                                self.styles.toolbar.toolbar_secondary_hover_opacity;
                            let toolbar_gap = self.scaled(self.styles.toolbar.toolbar_gap_px);
                            let toolbar_min_top =
                                self.scaled(self.styles.toolbar.toolbar_offset_top_px);
                            let toolbar_min_right =
                                self.scaled(self.styles.toolbar.toolbar_offset_left_px);
                            let secondary_btn_size =
                                self.scaled(self.styles.toolbar.toolbar_secondary_btn_size_px);
                            let secondary_icon_size =
                                self.scaled(self.styles.toolbar.toolbar_secondary_icon_size_px);
                            let secondary_btn_width = secondary_btn_size;
                            let toolbar_right = toolbar_min_right.round();
                            let toolbar_top = toolbar_min_top.round();
                            let outline_active =
                                self.utility_panel == Some(RightUtilityPanel::Outline);
                            let outline_tooltip = crate::i18n::outline_action_label(
                                self.ui_language,
                                outline_active,
                            );
                            let outline_x = editor_pane_width
                                - toolbar_right
                                - secondary_btn_width * 2.0
                                - toolbar_gap;
                            let outline_anchor =
                                toolbar_anchor_bounds(outline_x, toolbar_top, secondary_btn_width);
                            let theme_x = editor_pane_width - toolbar_right - secondary_btn_width;
                            let theme_anchor =
                                toolbar_anchor_bounds(theme_x, toolbar_top, secondary_btn_width);
                            let theme_tooltip = crate::i18n::theme_action_label(
                                self.ui_language,
                                self.theme_kind.is_dark(),
                            );
                            div()
                                .absolute()
                                .right(px(toolbar_right))
                                .top(px(toolbar_top))
                                .flex()
                                .items_center()
                                .gap(px(toolbar_gap))
                                .opacity(self.toolbar_opacity)
                                .when(self.active_tab_uses_markdown_editor(), |this| {
                                    this.child(self.render_toolbar_button(
                                        "toolbar-outline-toggle",
                                        secondary_btn_width,
                                        secondary_btn_size,
                                        secondary_icon_size,
                                        if outline_active { 1.0 } else { 0.55 },
                                        secondary_hover,
                                        OUTLINE_TOGGLE_SVG_PATH,
                                        outline_anchor,
                                        outline_tooltip,
                                        Some(crate::editor::shortcut::hint(crate::platform::keys::TOGGLE_OUTLINE)),
                                        cx,
                                        |this: &mut Self, cx| {
                                            this.toggle_outline_visibility(cx);
                                        },
                                    ))
                                })
                                .child(self.render_toolbar_button(
                                    "toolbar-theme-toggle",
                                    secondary_btn_width,
                                    secondary_btn_size,
                                    secondary_icon_size,
                                    1.0,
                                    secondary_hover,
                                    if self.theme_kind.is_dark() {
                                        THEME_DARK_SVG_PATH
                                    } else {
                                        THEME_LIGHT_SVG_PATH
                                    },
                                    theme_anchor,
                                    theme_tooltip,
                                    None,
                                    cx,
                                    |this: &mut Self, cx| {
                                        this.toggle_theme(cx);
                                    },
                                ))
                        })
                    })
                    .when(is_focused_pane, |this| {
                        this.when_some(self.render_outline_rail(cx), |this, rail| this.child(rail))
                    })
                    .when(
                        matches!(
                            self.tab_drag_split_target.or(self.context_menu_pane_target),
                            Some((id, _)) if id == pane_id
                        ),
                        |this| {
                            let direction = self
                                .tab_drag_split_target
                                .or(self.context_menu_pane_target)
                                .and_then(|(_, direction)| direction);
                            let overlay = DropIndicatorControl::overlay(self.styles.self_drawn.colors.drop_overlay);
                            let overlay = match direction {
                                None => overlay.top_0().left_0().right_0().bottom_0(),
                                Some(SplitDirection::Up) => {
                                    overlay.top_0().left_0().right_0().h(relative(0.5))
                                }
                                Some(SplitDirection::Down) => {
                                    overlay.bottom_0().left_0().right_0().h(relative(0.5))
                                }
                                Some(SplitDirection::Left) => {
                                    overlay.top_0().bottom_0().left_0().w(relative(0.5))
                                }
                                Some(SplitDirection::Right) => {
                                    overlay.top_0().bottom_0().right_0().w(relative(0.5))
                                }
                            };
                            this.child(overlay)
                        },
                    )
                    .when(
                        self.search_visible
                            && (self.active_tab_supports_text_search()
                                || self.active_tab_is_terminal()),
                        |this: gpui::Stateful<gpui::Div>| this.child(self.render_search_bar(cx)),
                    ),
            )
            .into_any_element()
    }
}
