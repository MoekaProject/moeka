//! Split-pane rendering: the recursive pane layout tree, per-pane leaf chrome
//! (tab bar + content), divider resize handles, and the per-pane editor pane
//! body extracted from `render.rs`.

use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, ContentMask, Corners, CursorStyle, ExternalPaths, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Point, ScrollWheelEvent, SharedString, Window,
    canvas, deferred, div, fill, prelude::*, px, relative,
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
        let scale = self.last_window_scale_factor.max(1.0);
        // At least one device pixel, rounded to a whole number of them.
        ((self.scaled(1.0) * scale).round().max(1.0)) / scale
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
        let ss = &self.styles.sidebar;
        let hover_indicator_color = ss.sidebar_resize_active_indicator_color;
        let handle_width = self.scaled(ss.sidebar_resize_handle_width_px).max(6.0);
        let is_active = self.pane_resize_drag.as_ref().is_some_and(|drag| {
            drag.axis == axis && drag.path == path && drag.divider_index == divider_index
        });
        let line_color = if is_active {
            hover_indicator_color
        } else {
            self.styles.tab_bar.tab_bar_border_color
        };

        let anchor = div().relative().flex_none().bg(line_color);
        let anchor = match axis {
            SplitAxis::Horizontal => anchor.w(px(thickness)).h_full(),
            SplitAxis::Vertical => anchor.h(px(thickness)).w_full(),
        };

        let hit = div()
            .id(SharedString::from(format!(
                "pane-divider-{path:?}-{divider_index}"
            )))
            .absolute();
        let hit = match axis {
            SplitAxis::Horizontal => hit
                .top_0()
                .left(px(-(handle_width - thickness) / 2.0))
                .w(px(handle_width))
                .h_full()
                .cursor(CursorStyle::ResizeLeftRight),
            SplitAxis::Vertical => hit
                .left_0()
                .top(px(-(handle_width - thickness) / 2.0))
                .h(px(handle_width))
                .w_full()
                .cursor(CursorStyle::ResizeUpDown),
        };
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
            .when(self.active_tab_is_diff(), |this| {
                this.child(self.render_active_tab_path_bar(cx))
            })
            .when(
                is_focused_pane
                    && self.app_settings.format_toolbar_visible
                    && self.active_tab_is_markdown(),
                |this| this.child(self.render_format_toolbar(editor_pane_width, cx)),
            )
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
                                let had_hover = this.hovered_editor_position.take().is_some();
                                if this.editor_scrollbar_drag.is_none() {
                                    this.set_editor_scrollbar_hovered(false, cx);
                                }
                                let toolbar_changed = if this.toolbar_corner_hovered {
                                    this.toolbar_corner_hovered = false;
                                    this.sync_toolbar_opacity(cx);
                                    true
                                } else {
                                    false
                                };
                                this.clear_gutter_fold_hover(cx);
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
                        let file_drop_bg = self.styles.editor.editor_file_drop_bg;
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
                        this.focus_pane(pane_id, cx);
                        this.import_external_paths_as_attachments(paths.paths(), cx);
                    }))
                    .child(if let Some(terminal_tab_id) = self.active_terminal_tab_id() {
                        self.render_terminal_tab_content(terminal_tab_id, window, cx)
                    } else if self.active_tab_is_whiteboard() {
                        whiteboard_element::WhiteboardElement {
                            editor: cx.entity().clone(),
                            pane_id,
                        }
                        .into_any_element()
                    } else if self.active_tab_is_image() {
                        image_view::ImageViewElement {
                            editor: cx.entity().clone(),
                            pane_id,
                        }
                        .into_any_element()
                    } else if self.active_tab_is_binary() {
                        self.render_binary_placeholder(cx)
                    } else {
                        element::EditorElement {
                            editor: cx.entity().clone(),
                            pane_id,
                        }
                        .into_any_element()
                    })
                    .when(self.active_tab_is_whiteboard(), |this| {
                        this.child(self.render_whiteboard_toolbar(cx)).when_some(
                            self.whiteboard_text_overlay_rect(),
                            |this, rect| {
                                let input = self
                                    .whiteboard_text_editor
                                    .as_ref()
                                    .map(|editor| editor.input.clone());
                                this.when_some(input, |this, input| {
                                    this.child(
                                        div()
                                            .id("wb-text-overlay")
                                            .occlude()
                                            .absolute()
                                            .left(px(rect.0))
                                            .top(px(rect.1))
                                            .w(px(rect.2))
                                            .h(px(rect.3))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|_, _: &MouseDownEvent, _, cx| {
                                                    cx.stop_propagation();
                                                }),
                                            )
                                            .child(input),
                                    )
                                })
                            },
                        )
                    })
                    .when_some(
                        self.editor_scrollbar_thumb_geometry(),
                        |this, (thumb_top, thumb_height)| {
                            let thumb_width = self.scaled(self.editor_scrollbar_width);
                            let hit_width = self.editor_scrollbar_hit_width();
                            let right_margin =
                                self.scaled(self.styles.editor.editor_scrollbar_right_margin_px);
                            let thumb_radius =
                                self.scaled(self.styles.editor.editor_scrollbar_radius_px);
                            let mut thumb_color = self.styles.editor.editor_scrollbar_thumb_color;
                            thumb_color.a *= self.editor_scrollbar_opacity.clamp(0.0, 1.0);
                            this.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right(px(0.0))
                                    .h_full()
                                    .w(px(hit_width + right_margin))
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(thumb_top))
                                            .right(px(right_margin))
                                            .w(px(thumb_width))
                                            .h(px(thumb_height))
                                            .cursor(CursorStyle::Arrow)
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
                                            )
                                            .rounded(px(thumb_radius))
                                            .bg(thumb_color),
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
                            let align_tooltip = if self.active_tab_is_non_markdown_text() {
                                self.t(crate::i18n::TextKey::ToolbarWrapTooltip)
                            } else {
                                self.t(crate::i18n::TextKey::ToolbarAlignTooltip)
                            };
                            let align_shortcut = self
                                .active_tab_is_non_markdown_text()
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
                                        if self.editor_left_aligned { 1.0 } else { 0.55 },
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
                                .when(self.active_tab_is_markdown(), |this| {
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
                        is_focused_pane && self.preview_overlay_state().is_some(),
                        |this| {
                            let editor_handle = cx.entity().clone();
                            this.child(deferred(
                                canvas(
                                    move |bounds, window, cx| {
                                        editor_handle.update(cx, |editor, _| {
                                            let Some(overlay) =
                                                editor.preview_overlay_state().cloned()
                                            else {
                                                editor.preview_overlay_bounds = None;
                                                editor.preview_overlay_image_bounds = None;
                                                return None;
                                            };
                                            let Some(base_image) = editor
                                                .image_cache
                                                .get(&overlay.cache_key)
                                                .and_then(|image| image.as_ref())
                                                .cloned()
                                            else {
                                                editor.preview_overlay_bounds = None;
                                                editor.preview_overlay_image_bounds = None;
                                                return None;
                                            };

                                            let base_size = base_image.size(0);
                                            let base_width = base_size.width.0 as f32;
                                            let base_height = base_size.height.0 as f32;
                                            let preview_kind = editor
                                                .decorations
                                                .get(overlay.line_idx)
                                                .and_then(|decoration| {
                                                    decoration.block_preview.as_ref()
                                                })
                                                .map(|preview| preview.kind);
                                            let (intrinsic_width, intrinsic_height) =
                                                if let Some(preview_kind) = preview_kind {
                                                    helpers::intrinsic_generated_preview_size_for_kind(
                                                        base_width,
                                                        base_height,
                                                        preview_kind,
                                                    )
                                                } else {
                                                    (base_width, base_height)
                                                };
                                            if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
                                                editor.preview_overlay_bounds = None;
                                                editor.preview_overlay_image_bounds = None;
                                                return None;
                                            }

                                            let overlay_bounds = bounds;
                                            let overlay_width =
                                                f32::from(bounds.size.width).max(1.0);
                                            let overlay_height =
                                                f32::from(bounds.size.height).max(1.0);
                                            let fit_scale = if preview_kind.is_some() {
                                                1.0
                                            } else {
                                                (overlay_width / intrinsic_width.max(1.0))
                                                    .min(overlay_height / intrinsic_height.max(1.0))
                                                    .min(1.0)
                                            };
                                            let base_display_width = intrinsic_width * fit_scale;
                                            let base_display_height = intrinsic_height * fit_scale;
                                            let display_width =
                                                base_display_width * overlay.view.zoom_factor;
                                            let display_height =
                                                base_display_height * overlay.view.zoom_factor;
                                            let image_left = f32::from(bounds.origin.x)
                                                + (overlay_width - display_width) * 0.5
                                                + overlay.view.pan_x;
                                            let image_top = f32::from(bounds.origin.y)
                                                + (overlay_height - display_height) * 0.5
                                                + overlay.view.pan_y;
                                            let overlay_image_bounds = Bounds::new(
                                                gpui::point(px(image_left), px(image_top)),
                                                gpui::size(px(display_width), px(display_height)),
                                            );

                                            let image = if let Some(preview_kind) = preview_kind {
                                                let minimum_raster_scale =
                                                    helpers::generated_preview_raster_scale(
                                                        preview_kind,
                                                    );
                                                // Keep overlay zoom responsive by scaling an already-rasterized
                                                // image instead of regenerating Mermaid/Math SVG on every wheel tick.
                                                let overlay_raster_scale = window
                                                    .scale_factor()
                                                    .max(minimum_raster_scale);
                                                let scale_key = ((overlay_raster_scale * 100.0)
                                                    .round()
                                                    .clamp(minimum_raster_scale * 100.0, 1600.0))
                                                    as u16;

                                                let cached_overlay_image = editor
                                                    .generated_preview_overlay_cache
                                                    .get(&overlay.cache_key)
                                                    .and_then(|(cached_scale_key, cached_image)| {
                                                        (*cached_scale_key == scale_key)
                                                            .then(|| cached_image.clone())
                                                    });
                                                let svg = editor
                                                    .generated_preview_svg_cache
                                                    .get(&overlay.cache_key)
                                                    .cloned();
                                                if let Some(cached_overlay_image) =
                                                    cached_overlay_image
                                                {
                                                    cached_overlay_image
                                                } else if let Some(svg) = svg {
                                                    match helpers::load_render_image_from_svg_with_scale(
                                                        svg.as_ref(),
                                                        scale_key as f32 / 100.0,
                                                    )
                                                    .map(Arc::new)
                                                    {
                                                        Ok(image) => {
                                                            editor
                                                                .generated_preview_overlay_cache
                                                                .insert(
                                                                    overlay.cache_key.clone(),
                                                                    (scale_key, image.clone()),
                                                                );
                                                            image
                                                        }
                                                        Err(_) => base_image.clone(),
                                                    }
                                                } else {
                                                    base_image.clone()
                                                }
                                            } else {
                                                base_image.clone()
                                            };

                                            editor.preview_overlay_bounds = Some(overlay_bounds);
                                            editor.preview_overlay_image_bounds =
                                                Some(overlay_image_bounds);

                                            Some((
                                                overlay_bounds,
                                                overlay_image_bounds,
                                                image,
                                                editor.styles.editor.editor_bg,
                                                // Generated math/Mermaid previews are cropped tightly
                                                // to glyph ink, so a rounded-corner clip cuts into real
                                                // glyphs (e.g. E=mc²). Keep square corners for them.
                                                px(0.0),
                                            ))
                                        })
                                    },
                                    move |_bounds, overlay_paint, window, _cx| {
                                        if let Some((
                                            overlay_bounds,
                                            overlay_image_bounds,
                                            img_data,
                                            background,
                                            corner_radius,
                                        )) = overlay_paint
                                        {
                                            window.paint_quad(fill(overlay_bounds, background));
                                            window.with_content_mask(
                                                Some(ContentMask {
                                                    bounds: overlay_bounds,
                                                }),
                                                |window| {
                                                    window
                                                        .paint_image(
                                                            overlay_image_bounds,
                                                            Corners::all(corner_radius),
                                                            img_data,
                                                            0,
                                                            false,
                                                        )
                                                        .ok();
                                                },
                                            );
                                        }
                                    },
                                )
                                .absolute()
                                .left(px(0.0))
                                .top(px(0.0))
                                .right(px(0.0))
                                .bottom(px(0.0)),
                            ))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(0.0))
                                    .top(px(0.0))
                                    .right(px(0.0))
                                    .bottom(px(0.0))
                                    .bg(gpui::transparent_black())
                                    .cursor(CursorStyle::OpenHand)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                            let should_capture = this
                                                .preview_overlay_bounds
                                                .is_some_and(|bounds| {
                                                    bounds.contains(&event.position)
                                                });
                                            if should_capture {
                                                cx.stop_propagation();
                                                this.on_mouse_down(event, window, cx);
                                            }
                                        }),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, event: &MouseUpEvent, window, cx| {
                                            let should_capture = this.image_drag_state.is_some()
                                                || this.preview_overlay_bounds.is_some_and(
                                                    |bounds| bounds.contains(&event.position),
                                                );
                                            if should_capture {
                                                cx.stop_propagation();
                                                this.on_mouse_up(event, window, cx);
                                            }
                                        }),
                                    )
                                    .on_mouse_up_out(
                                        MouseButton::Left,
                                        cx.listener(|this, event: &MouseUpEvent, window, cx| {
                                            if this.image_drag_state.is_some() {
                                                cx.stop_propagation();
                                                this.on_mouse_up(event, window, cx);
                                            }
                                        }),
                                    )
                                    .on_mouse_move(cx.listener(
                                        |this, event: &MouseMoveEvent, window, cx| {
                                            let should_capture = this.image_drag_state.is_some()
                                                || this.preview_overlay_bounds.is_some_and(
                                                    |bounds| bounds.contains(&event.position),
                                                );
                                            if should_capture {
                                                this.on_mouse_move(event, window, cx);
                                            }
                                        },
                                    )),
                            )
                            .child(self.render_preview_overlay_actions(cx))
                        },
                    )
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
                            let overlay_bg = self.styles.editor.editor_file_drop_bg;
                            let overlay = div().absolute().bg(overlay_bg);
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
