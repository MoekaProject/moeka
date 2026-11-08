mod snapshot;

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollWheelEvent, canvas, div, point, prelude::*, px, size,
};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::app_settings::{MinimapPosition, MinimapSide};
use super::{EditorView, PaneId};
pub(super) use snapshot::Snapshot;
const MAX_QUADS: usize = 32_768;
const WINDOW_MARK_BYTES: usize = 32 * 1024 * 1024;
const MAX_SNAPSHOTS: usize = 1;
const MAX_PREPARE_ROWS: usize = 1024;
const MAX_PREPARE_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) struct MinimapMotion {
    side: MinimapSide,
    fraction: f32,
}

impl MinimapMotion {
    pub(super) fn new(position: MinimapPosition, last_side: Option<MinimapSide>) -> Self {
        Self {
            side: position.side().or(last_side).unwrap_or_default(),
            fraction: if position.is_visible() { 1.0 } else { 0.0 },
        }
    }

    fn next_stage(&mut self, target: Option<MinimapSide>) -> Option<(f32, f32)> {
        if target == Some(self.side) {
            (self.fraction < 1.0).then_some((self.fraction, 1.0))
        } else if self.fraction > 0.0 {
            Some((self.fraction, 0.0))
        } else if let Some(side) = target {
            self.side = side;
            Some((0.0, 1.0))
        } else {
            None
        }
    }

    fn snap(&mut self, target: Option<MinimapSide>) {
        if let Some(side) = target {
            self.side = side;
            self.fraction = 1.0;
        } else {
            self.fraction = 0.0;
        }
    }
}

fn snapshot_item_limit(panes: usize) -> usize {
    // Four live generations (capture/state/paint frames), with 2x Vec growth headroom.
    MAX_QUADS.min(WINDOW_MARK_BYTES / 8 / panes.max(1) / Snapshot::item_size())
}

#[derive(Clone, Copy, Debug)]
struct Projection {
    height: f64,
    viewport: f64,
    track: f64,
    scale: f64,
    origin: f64,
    thumb_top: f64,
    thumb_height: f64,
}

impl Projection {
    fn new(height: f64, viewport: f64, track: f64, scroll: f64, scale: f64) -> Self {
        let height = height.max(viewport).max(1.0);
        let viewport = viewport.max(1.0);
        let track = track.max(1.0);
        let scale = scale.clamp(f64::EPSILON, 1.0).min(track / viewport);
        let q = (scroll / (height - viewport).max(1.0)).clamp(0.0, 1.0);
        let origin = q * (height * scale - track).max(0.0);
        let thumb_height = (viewport * scale).min(track);
        let thumb_top = (scroll.clamp(0.0, height - viewport) * scale - origin)
            .clamp(0.0, track - thumb_height);
        Self {
            height,
            viewport,
            track,
            scale,
            origin,
            thumb_top,
            thumb_height,
        }
    }

    fn click_scroll(self, y: f64) -> f64 {
        ((self.origin + y) / self.scale - self.viewport * 0.5)
            .clamp(0.0, self.height - self.viewport)
    }

    fn drag_scroll(self, thumb_top: f64) -> f64 {
        let travel = self.track - self.thumb_height;
        if self.height * self.scale <= self.track {
            (thumb_top / self.scale).clamp(0.0, self.height - self.viewport)
        } else if travel > 0.0 {
            (thumb_top / travel).clamp(0.0, 1.0) * (self.height - self.viewport)
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug)]
struct Drag {
    tab: Option<String>,
    start_y: f64,
    current_y: f64,
    start_thumb: f64,
}

#[derive(Default)]
pub(crate) struct MinimapState {
    bounds: Option<Bounds<Pixels>>,
    projection: Option<Projection>,
    tab: Option<String>,
    drag: Option<Drag>,
    handle_hovered: bool,
    snapshots: VecDeque<Rc<Snapshot>>,
}

impl MinimapState {
    pub(super) fn track_bounds(&self) -> Option<Bounds<Pixels>> {
        self.bounds
    }

    fn handle_contains(&self, position: Point<Pixels>, min_hit_height: f32) -> bool {
        let (Some(bounds), Some(projection)) = (self.bounds, self.projection) else {
            return false;
        };
        if !bounds.contains(&position) {
            return false;
        }
        let y = f32::from(position.y - bounds.top()) as f64;
        let center = projection.thumb_top + projection.thumb_height * 0.5;
        (y - center).abs() <= projection.thumb_height.max(min_hit_height as f64) * 0.5
    }

    pub(super) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub(super) fn cancel_drag(&mut self) {
        self.drag = None;
    }
    pub(super) fn clear_snapshot(&mut self) {
        self.snapshots.clear();
    }

    fn publish(&mut self, tab: Option<String>, snapshot: Snapshot) {
        if self.tab != tab {
            *self = Self::default();
        }
        self.tab = tab;
        if self.snapshots.back().is_some_and(|previous| {
            previous.viewport.size != snapshot.viewport.size
                || previous.background != snapshot.background
        }) {
            self.snapshots.clear();
        }
        self.snapshots
            .retain(|previous| previous.scroll != snapshot.scroll);
        while self.snapshots.len() >= MAX_SNAPSHOTS {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(Rc::new(snapshot));
    }
}

fn snapshot_scale(source_width: f32, minimap_width: f32, padding: f32) -> f64 {
    ((minimap_width - padding * 2.0).max(1.0) / source_width.max(1.0)) as f64
}

fn supports_minimap(kind: Option<&super::FileKind>, embedded: bool) -> bool {
    !embedded && kind.is_some_and(super::FileKind::supports_text_buffer)
}

fn bounded_render_rows(
    buffer: &crate::buffer::Buffer,
    decorations: &[crate::markdown::LineDecoration],
    requested: std::ops::Range<usize>,
    fences: Option<&[std::ops::Range<usize>]>,
) -> Option<std::ops::Range<usize>> {
    let mut range =
        requested.start.min(buffer.line_count())..requested.end.min(buffer.line_count());
    if range.is_empty() {
        return None;
    }
    let within_budget = |range: &std::ops::Range<usize>| {
        range.end - range.start <= MAX_PREPARE_ROWS
            && buffer
                .line_start(range.end)
                .saturating_sub(buffer.line_start(range.start))
                <= MAX_PREPARE_BYTES
    };
    if !within_budget(&range) {
        return None;
    }
    // Match the shared parser's block-boundary expansion, but reject unbounded requests.
    while fences.is_some() && range.start > 0 {
        let previous = buffer.line_content(range.start - 1);
        if previous.len() > MAX_PREPARE_BYTES {
            return None;
        }
        if previous.trim().is_empty() {
            break;
        }
        range.start -= 1;
        if !within_budget(&range) {
            return None;
        }
    }
    while fences.is_some() && range.end < buffer.line_count() {
        let next = buffer.line_content(range.end);
        if next.len() > MAX_PREPARE_BYTES {
            return None;
        }
        if next.trim().is_empty() {
            break;
        }
        range.end += 1;
        if !within_budget(&range) {
            return None;
        }
    }
    if let Some(fences) = fences {
        let first = fences.partition_point(|fence| fence.end <= range.start);
        for fence in fences[first..].iter().take(MAX_PREPARE_ROWS) {
            if fence.start >= range.end {
                break;
            }
            range.start = range.start.min(fence.start);
            range.end = range.end.max(fence.end);
            if !within_budget(&range) {
                return None;
            }
        }
    }
    let mut line = range.start;
    while line < range.end {
        let decoration = decorations.get(line)?;
        if matches!(
            decoration.block_type,
            crate::markdown::BlockType::CodeBlock | crate::markdown::BlockType::CodeBlockFence
        ) {
            let code_line = |index: usize| {
                decorations.get(index).is_some_and(|row| {
                    matches!(
                        row.block_type,
                        crate::markdown::BlockType::CodeBlock
                            | crate::markdown::BlockType::CodeBlockFence
                    )
                })
            };
            while range.start > 0 && code_line(range.start - 1) {
                range.start -= 1;
                if !within_budget(&range) {
                    return None;
                }
            }
            while range.end < buffer.line_count() && code_line(range.end) {
                range.end += 1;
                if !within_budget(&range) {
                    return None;
                }
            }
        }
        if let Some(table) = &decoration.table_info {
            range.start = range.start.min(table.table_start_line);
            range.end = range.end.max(table.table_end_line);
        }
        if let Some(preview) = &decoration.block_preview {
            range.start = range.start.min(preview.start_line);
            range.end = range.end.max(preview.end_line.saturating_add(1));
        }
        if !within_budget(&range) {
            return None;
        }
        line += 1;
    }
    Some(range)
}

impl EditorView {
    pub(super) fn set_minimap_position(
        &mut self,
        position: MinimapPosition,
        cx: &mut Context<Self>,
    ) {
        let previous = self.app_settings.minimap_position;
        if previous == position {
            return;
        }
        if let Some(side) = position.side().or(previous.side()) {
            self.app_settings.minimap_last_side = Some(side);
        }
        self.app_settings.minimap_position = position;
        self.sync_minimap_motion(cx);
    }

    pub(super) fn toggle_minimap_visibility(&mut self, cx: &mut Context<Self>) {
        let position = self
            .app_settings
            .minimap_position
            .toggled(self.app_settings.minimap_last_side);
        self.set_minimap_position(position, cx);
        self.save_settings_in_background(cx);
        cx.notify();
    }

    pub(super) fn sync_minimap_motion(&mut self, cx: &mut Context<Self>) {
        self.minimap_state.cancel_drag();
        for pane in &mut self.panes {
            pane.state.minimap_state.cancel_drag();
        }
        self.minimap_motion_epoch = self.minimap_motion_epoch.wrapping_add(1);
        let epoch = self.minimap_motion_epoch;
        self.minimap_motion_task = None;
        let target = self.app_settings.minimap_position.side();
        if !self.animations_enabled() {
            self.minimap_motion.snap(target);
            if target.is_none() {
                self.clear_minimap_states();
            }
            self.notify_panel_presentation(cx);
            return;
        }
        let Some((mut start, mut end)) = self.minimap_motion.next_stage(target) else {
            if target.is_none() {
                self.clear_minimap_states();
            }
            self.notify_panel_presentation(cx);
            return;
        };
        let duration_ms = self
            .styles
            .utility_panel
            .utility_panel_visibility_animation_ms
            .max(1);
        self.minimap_motion_task = Some(cx.spawn(async move |this, cx| {
            let mut started = Instant::now();
            loop {
                let duration = (duration_ms as f32 * (end - start).abs()).max(1.0);
                let t = (started.elapsed().as_secs_f32() * 1000.0 / duration).clamp(0.0, 1.0);
                let next = this.update(cx, |this, cx| {
                    if this.minimap_motion_epoch != epoch {
                        return None;
                    }
                    if !this.animations_enabled() {
                        this.minimap_motion.snap(target);
                        this.minimap_motion_task = None;
                        if target.is_none() {
                            this.clear_minimap_states();
                        }
                        this.notify_panel_presentation(cx);
                        return None;
                    }
                    this.minimap_motion.fraction =
                        start + (end - start) * super::ease_in_out_cubic(t);
                    if t >= 1.0 {
                        this.minimap_motion.fraction = end;
                        if let Some(stage) = this.minimap_motion.next_stage(target) {
                            this.notify_panel_presentation(cx);
                            return Some(Some(stage));
                        }
                        this.minimap_motion_task = None;
                        if target.is_none() {
                            this.clear_minimap_states();
                        }
                        this.notify_panel_presentation(cx);
                        return None;
                    }
                    this.notify_panel_presentation(cx);
                    Some(None)
                });
                match next {
                    Ok(Some(Some((next_start, next_end)))) => {
                        start = next_start;
                        end = next_end;
                        started = Instant::now();
                    }
                    Ok(Some(None)) => {}
                    _ => return,
                }
                cx.background_executor()
                    .timer(Duration::from_millis(super::PANEL_ANIMATION_FRAME_MS))
                    .await;
            }
        }));
    }

    fn clear_minimap_states(&mut self) {
        self.minimap_state = Default::default();
        for pane in &mut self.panes {
            pane.state.minimap_state = Default::default();
        }
    }

    pub(super) fn minimap_display_is_left(&self) -> bool {
        self.minimap_motion.side == MinimapSide::Left
    }

    pub(super) fn minimap_lane_width(&self) -> f32 {
        self.styles.minimap.lane_width(self.ui_scale)
    }

    fn minimap_content_width(&self, viewport_width: f32) -> f32 {
        let editor_width = if self.editor_uses_expanded_layout() {
            viewport_width
        } else {
            viewport_width.min(self.max_editor_width())
        };
        (editor_width - self.padding_left() - self.padding_right()).max(self.scaled(120.0))
    }

    pub(super) fn minimap_render_rows(
        &self,
        bounds: Bounds<Pixels>,
    ) -> Option<std::ops::Range<usize>> {
        let pane = self.pane_bounds.get(&self.current_pane_id())?;
        if self.minimap_width(f32::from(pane.size.width)) == 0.0 {
            return None;
        }
        let projection = self.minimap_projection_for_width(
            f32::from(bounds.size.height) as f64,
            self.minimap_content_width(f32::from(bounds.size.width)),
        );
        let start = self.line_at_y((projection.origin / projection.scale) as f32);
        let end = self
            .line_at_y(((projection.origin + projection.track) / projection.scale) as f32)
            .saturating_add(1)
            .min(self.buffer.line_count());
        let chunk = super::LARGE_MARKDOWN_CHUNK_LINES;
        let range = if self.uses_lazy_markdown_decorations() {
            start / chunk * chunk
                ..end
                    .div_ceil(chunk)
                    .saturating_mul(chunk)
                    .min(self.buffer.line_count())
        } else {
            start..end
        };
        bounded_render_rows(
            &self.buffer,
            &self.decorations,
            range,
            self.lazy_markdown_decorations
                .as_ref()
                .and_then(|lazy| lazy.fenced_code_spans.as_deref()),
        )
    }

    pub(super) fn minimap_snapshot_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        let projection = self.minimap_projection(f32::from(bounds.size.height) as f64);
        Bounds::new(
            point(
                px(self.last_content_left),
                bounds.top()
                    + px((projection.origin / projection.scale - self.scroll_offset as f64) as f32),
            ),
            size(
                px(self.minimap_content_width(self.last_viewport_width)),
                px((projection.track / projection.scale) as f32),
            ),
        )
    }

    pub(super) fn minimap_snapshot_limit(&self) -> usize {
        snapshot_item_limit(self.panes.len())
    }

    pub(super) fn publish_minimap_snapshot(&mut self, snapshot: Snapshot) {
        if let Some(bench) = self.large_document_frame_bench.as_mut() {
            bench
                .minimap_initial_text_bottom
                .get_or_insert(snapshot.text_bottom);
            bench
                .minimap_initial_capture_height
                .get_or_insert(f32::from(snapshot.viewport.size.height));
        }
        self.minimap_state
            .publish(self.active_tab_id.clone(), snapshot);
    }

    pub(super) fn advance_minimap_drag_benchmark(&mut self, frame: usize, total: usize) {
        let Some(bounds) = self.minimap_state.bounds else {
            return;
        };
        let projection = self.minimap_projection(f32::from(bounds.size.height) as f64);
        let progress = frame as f64 / total.saturating_sub(1).max(1) as f64;
        self.cancel_pending_scroll_navigation();
        self.minimap_state.drag = Some(Drag {
            tab: self.active_tab_id.clone(),
            start_y: 0.0,
            current_y: progress * (projection.track - projection.thumb_height),
            start_thumb: 0.0,
        });
        self.refresh_minimap_drag_after_layout();
    }

    pub(super) fn minimap_width(&self, pane_width: f32) -> f32 {
        if supports_minimap(self.active_file_kind(), self.embedded_editor)
            && pane_width >= self.scaled(self.styles.minimap.min_pane_width_px)
        {
            self.minimap_lane_width() * self.minimap_motion.fraction
        } else {
            0.0
        }
    }

    fn minimap_projection(&self, track: f64) -> Projection {
        self.minimap_projection_for_width(
            track,
            self.minimap_content_width(self.last_viewport_width),
        )
    }

    fn minimap_projection_for_width(&self, track: f64, content_width: f32) -> Projection {
        let viewport = self.editor_visible_height() as f64;
        Projection::new(
            self.max_scroll_offset() as f64 + viewport,
            viewport,
            track,
            self.scroll_offset as f64,
            snapshot_scale(
                content_width,
                self.minimap_lane_width() - self.styles.minimap.border_width_px,
                self.scaled(self.styles.minimap.padding_px),
            ),
        )
    }

    pub(super) fn refresh_minimap_drag_after_layout(&mut self) -> bool {
        let Some(drag) = self.minimap_state.drag.clone() else {
            return false;
        };
        if drag.tab != self.active_tab_id
            || !self.app_settings.minimap_position.is_visible()
            || !self.active_tab_supports_text_buffer()
            || self.modal_overlay_is_open()
        {
            self.minimap_state = Default::default();
            return false;
        }
        let Some(bounds) = self.minimap_state.bounds else {
            return false;
        };
        let projection = self.minimap_projection(f32::from(bounds.size.height) as f64);
        let target =
            projection.drag_scroll(drag.start_thumb + drag.current_y - drag.start_y) as f32;
        let changed = (target - self.scroll_offset).abs() > 0.5;
        self.scroll_offset = target;
        changed
    }

    pub(super) fn update_minimap_drag(
        &mut self,
        position: Point<Pixels>,
        finish: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut handled = false;
        let ids = self
            .panes
            .iter()
            .filter_map(|pane| {
                let state = if pane.id == self.focused_pane_id {
                    &self.minimap_state
                } else {
                    &pane.state.minimap_state
                };
                state.drag.as_ref().map(|_| pane.id)
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.with_pane(id, |this| {
                if let Some(drag) = this.minimap_state.drag.as_mut() {
                    drag.current_y = f32::from(position.y) as f64;
                    this.refresh_minimap_drag_after_layout();
                    this.hide_line_format_toolbar_for_scroll(cx);
                    this.update_toolbar_visibility_for_scroll(cx);
                    if finish {
                        this.minimap_state.drag = None;
                        this.minimap_state.handle_hovered = this.minimap_state.handle_contains(
                            position,
                            this.scaled(this.styles.minimap.min_handle_hit_height_px),
                        );
                    }
                    this.reveal_editor_scrollbar(cx);
                    handled = true;
                }
            });
        }
        if handled {
            cx.notify();
        }
        handled
    }

    pub(super) fn render_animated_minimap(
        &self,
        pane_id: PaneId,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_left = self.minimap_display_is_left();
        div()
            .id(("minimap-animated-lane", pane_id))
            .debug_selector(move || format!("minimap-animated-lane-{pane_id}"))
            .relative()
            .w(px(width))
            .h_full()
            .flex_none()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .w(px(self.minimap_lane_width()))
                    .when(is_left, |lane| lane.left_0())
                    .when(!is_left, |lane| lane.right_0())
                    .child(self.render_minimap(pane_id, cx)),
            )
            .into_any_element()
    }

    fn render_minimap(&self, pane_id: PaneId, cx: &mut Context<Self>) -> AnyElement {
        let editor = cx.entity().clone();
        let style = self.styles.minimap;
        let scale = self.ui_scale;
        div()
            .id(("markdown-minimap", pane_id))
            .debug_selector(move || format!("minimap-full-lane-{pane_id}"))
            .w(px(self.minimap_lane_width()))
            .h_full()
            .flex_none()
            .relative()
            .opacity(style.opacity)
            .overflow_hidden()
            .occlude()
            .cursor(if self.minimap_state.is_dragging() {
                CursorStyle::ClosedHand
            } else if self.minimap_state.handle_hovered {
                CursorStyle::OpenHand
            } else {
                CursorStyle::Arrow
            })
            .bg(style.colors.placeholder)
            .when(self.minimap_display_is_left(), |lane| {
                lane.pr(px(style.border_width_px))
            })
            .when(!self.minimap_display_is_left(), |lane| {
                lane.pl(px(style.border_width_px))
            })
            .child(
                canvas(
                    move |bounds, _, cx| {
                        editor.update(cx, |this, _| {
                            this.with_pane(pane_id, |this| {
                                let projection =
                                    this.minimap_projection(f32::from(bounds.size.height) as f64);
                                if this.minimap_state.tab != this.active_tab_id {
                                    this.minimap_state = Default::default();
                                }
                                this.minimap_state.bounds = Some(bounds);
                                this.minimap_state.projection = Some(projection);
                                this.minimap_state.tab = this.active_tab_id.clone();
                                let snapshots = this.minimap_state.snapshots.clone();
                                if let Some(bench) = this.large_document_frame_bench.as_mut() {
                                    bench.minimap_max_marks = bench
                                        .minimap_max_marks
                                        .max(snapshots.iter().map(|snapshot| snapshot.len()).sum());
                                }
                                (
                                    snapshots,
                                    projection,
                                    this.minimap_state.handle_hovered,
                                    this.minimap_state.is_dragging(),
                                )
                            })
                        })
                    },
                    move |bounds, (snapshots, projection, hovered, dragging), window, cx| {
                        for snapshot in snapshots {
                            snapshot.paint(
                                bounds,
                                style.padding_px * scale,
                                projection.scale,
                                projection.origin,
                                window,
                                cx,
                            );
                        }
                        let thumb = Bounds::new(
                            bounds.origin + point(px(0.0), px(projection.thumb_top as f32)),
                            size(bounds.size.width, px(projection.thumb_height as f32)),
                        );
                        let hover =
                            window.hover_transition_progress(("minimap-handle", pane_id), hovered);
                        let handle_fill = if dragging {
                            style.handle_color(hovered, true)
                        } else {
                            gpui::interpolate_color(
                                style.handle_color(false, false),
                                style.handle_color(true, false),
                                hover,
                            )
                        };
                        window.paint_quad(gpui::fill(thumb, handle_fill));
                    },
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    if this.app_settings.minimap_position.side() != Some(this.minimap_motion.side) {
                        return;
                    }
                    this.focus_pane(pane_id, cx);
                    this.focused_sidebar_path = None;
                    window.focus(&this.focus_handle);
                    let (Some(bounds), Some(projection)) =
                        (this.minimap_state.bounds, this.minimap_state.projection)
                    else {
                        return;
                    };
                    this.cancel_pending_scroll_navigation();
                    let y = f32::from(event.position.y - bounds.origin.y) as f64;
                    if this
                        .minimap_state
                        .handle_contains(event.position, style.min_handle_hit_height_px * scale)
                    {
                        this.minimap_state.handle_hovered = true;
                        this.minimap_state.drag = Some(Drag {
                            tab: this.active_tab_id.clone(),
                            start_y: f32::from(event.position.y) as f64,
                            current_y: f32::from(event.position.y) as f64,
                            start_thumb: projection.thumb_top,
                        });
                    } else {
                        this.scroll_offset = projection.click_scroll(y) as f32;
                    }
                    this.hide_line_format_toolbar_for_scroll(cx);
                    this.update_toolbar_visibility_for_scroll(cx);
                    this.reveal_editor_scrollbar(cx);
                    cx.notify();
                }),
            )
            .on_scroll_wheel(
                cx.listener(move |this, event: &ScrollWheelEvent, window, cx| {
                    this.with_pane(pane_id, |this| {
                        if this.modal_overlay_is_open() {
                            return;
                        }
                        this.minimap_state.cancel_drag();
                        this.on_scroll(event, window, cx);
                        this.reveal_editor_scrollbar(cx);
                    });
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                this.with_pane(pane_id, |this| {
                    let hovered = this.minimap_state.handle_contains(
                        event.position,
                        this.scaled(style.min_handle_hit_height_px),
                    );
                    if this.minimap_state.handle_hovered != hovered {
                        this.minimap_state.handle_hovered = hovered;
                        cx.notify();
                    }
                });
                cx.stop_propagation();
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.with_pane(pane_id, |this| {
                        if this.minimap_state.handle_hovered {
                            this.minimap_state.handle_hovered = false;
                            cx.notify();
                        }
                    });
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.update_minimap_drag(event.position, true, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimap_motion_keeps_its_side_until_collapse_and_reverses() {
        for (start, other) in [
            (MinimapSide::Left, MinimapSide::Right),
            (MinimapSide::Right, MinimapSide::Left),
        ] {
            let mut motion = MinimapMotion::new(start.position(), None);
            assert_eq!(motion.side, start);
            assert_eq!(motion.fraction, 1.0);
            assert_eq!(motion.next_stage(None), Some((1.0, 0.0)));
            motion.fraction = 0.4;
            assert_eq!(motion.next_stage(Some(start)), Some((0.4, 1.0)));
            assert_eq!(motion.next_stage(Some(other)), Some((0.4, 0.0)));
            assert_eq!(motion.side, start);
            motion.fraction = 0.0;
            assert_eq!(motion.next_stage(Some(other)), Some((0.0, 1.0)));
            assert_eq!(motion.side, other);
            assert_eq!(start.position().toggled(Some(start)), MinimapPosition::Off);
            assert_eq!(MinimapPosition::Off.toggled(Some(start)), start.position());
            motion.fraction = 1.0;
            assert_eq!(motion.next_stage(Some(other)), None);
            motion.snap(None);
            assert_eq!(motion.fraction, 0.0);
            assert_eq!(motion.side, other);
        }
        assert_eq!(
            MinimapMotion::new(MinimapPosition::Off, None).side,
            MinimapSide::Right
        );
        assert_eq!(MinimapPosition::Off.toggled(None), MinimapPosition::Right);
        assert_eq!(
            MinimapMotion::new(MinimapPosition::Off, Some(MinimapSide::Left)).side,
            MinimapSide::Left
        );
    }

    #[test]
    fn minimap_last_side_roundtrips_with_legacy_settings() {
        let legacy: super::super::AppSettings =
            serde_yaml::from_str("minimap_visible: false").unwrap();
        assert_eq!(legacy.minimap_position, MinimapPosition::Off);
        assert_eq!(legacy.minimap_last_side, None);
        let legacy_outline: super::super::AppSettings =
            serde_yaml::from_str("outline_panel_left: true\nminimap_position: left").unwrap();
        assert_eq!(legacy_outline.minimap_position, MinimapPosition::Left);
        assert!(
            !serde_yaml::to_string(&legacy_outline)
                .unwrap()
                .contains("outline_panel_left")
        );
        let saved: super::super::AppSettings =
            serde_yaml::from_str("minimap_position: off\nminimap_last_side: left").unwrap();
        assert_eq!(saved.minimap_last_side, Some(MinimapSide::Left));
        let restored: super::super::AppSettings =
            serde_yaml::from_str(&serde_yaml::to_string(&saved).unwrap()).unwrap();
        assert_eq!(restored.minimap_last_side, Some(MinimapSide::Left));
        assert!(
            serde_yaml::from_str::<super::super::AppSettings>("minimap_last_side: off").is_err()
        );
    }

    #[cfg(feature = "gpui-test-support")]
    mod presentation_tests {
        use super::*;
        use gpui::{Entity, Render, Subscription, Window};

        struct Workspace(Entity<EditorView>, Subscription);

        impl Render for Workspace {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let _ = &self.1;
                let content = self.0.update(cx, |editor, cx| {
                    editor.render_workspace_content_column(window, cx)
                });
                div().size_full().flex().child(content)
            }
        }

        #[gpui::test]
        fn minimap_pointer_scrolling_updates_toolbar_visibility(cx: &mut gpui::TestAppContext) {
            let source = "paragraph\n\n".repeat(200);
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some(&source), false);
                    editor.embedded_editor = false;
                    editor.app_settings.simple_mode = false;
                    editor.styles.ui.spec.animations_enabled = false;
                    editor.styles.minimap.min_pane_width_px = 0.0;
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| {
                let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
                Workspace(editor.clone(), subscription)
            });
            for side in [MinimapSide::Left, MinimapSide::Right] {
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.app_settings.minimap_position = side.position();
                        editor.minimap_motion.snap(Some(side));
                        editor.scroll_offset = 0.0;
                        editor.update_toolbar_visibility_for_scroll(cx);
                        editor.sync_toolbar_opacity(cx);
                        cx.notify();
                    });
                });
                cx.run_until_parked();
                let start = cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    assert!(!editor.toolbar_hidden_by_scroll);
                    assert_eq!(editor.toolbar_opacity, 1.0);
                    let bounds = editor.minimap_state.bounds.expect("minimap bounds");
                    assert!(bounds.size.height > px(100.0));
                    let projection = editor.minimap_state.projection.expect("minimap projection");
                    point(
                        bounds.center().x,
                        bounds.top()
                            + px((projection.thumb_top + projection.thumb_height - 2.0) as f32),
                    )
                });
                cx.simulate_event(MouseDownEvent {
                    button: MouseButton::Left,
                    position: start,
                    ..Default::default()
                });
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        assert!(
                            editor.minimap_state.is_dragging(),
                            "{side:?}: position={start:?}, bounds={:?}, projection={:?}, scroll={}",
                            editor.minimap_state.bounds,
                            editor.minimap_state.projection,
                            editor.scroll_offset,
                        );
                        assert!(editor.update_minimap_drag(
                            start + point(px(0.0), px(120.0)),
                            false,
                            cx,
                        ));
                        assert!(editor.scroll_offset > 0.5);
                        assert!(editor.toolbar_hidden_by_scroll);
                        assert_eq!(editor.toolbar_opacity, 0.0);
                        assert!(editor.update_minimap_drag(start, true, cx));
                        assert!(editor.scroll_offset <= 0.5);
                        assert!(!editor.toolbar_hidden_by_scroll);
                        assert_eq!(editor.toolbar_opacity, 1.0);
                    });
                });
                cx.simulate_event(MouseUpEvent {
                    button: MouseButton::Left,
                    position: start,
                    ..Default::default()
                });
                cx.run_until_parked();
                let track =
                    cx.update(|_, cx| editor.read(cx).minimap_state.bounds.unwrap().center());
                cx.simulate_event(MouseDownEvent {
                    button: MouseButton::Left,
                    position: track,
                    ..Default::default()
                });
                cx.simulate_event(MouseUpEvent {
                    button: MouseButton::Left,
                    position: track,
                    ..Default::default()
                });
                cx.run_until_parked();
                cx.update(|_, cx| {
                    let editor = editor.read(cx);
                    assert!(editor.scroll_offset > 0.5);
                    assert!(editor.toolbar_hidden_by_scroll);
                    assert_eq!(editor.toolbar_opacity, 0.0);
                });
            }
        }

        #[gpui::test]
        fn minimap_lane_animates_from_the_configured_edge(cx: &mut gpui::TestAppContext) {
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some("# Example"), false);
                    editor.embedded_editor = false;
                    editor.app_settings.simple_mode = true;
                    editor.styles.minimap.min_pane_width_px = 0.0;
                    editor.last_window_width = 1000.0;
                    editor.last_window_height = 700.0;
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| {
                let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
                Workspace(editor.clone(), subscription)
            });
            let button = cx
                .debug_bounds("tab-bar-minimap-toggle")
                .expect("minimap button");
            let outline = cx
                .debug_bounds("tab-bar-outline-toggle")
                .expect("outline button");
            assert!(button.right() <= outline.left());
            let tab = cx.debug_bounds("tab-bar-0").expect("tab bar");
            for side in [MinimapSide::Left, MinimapSide::Right] {
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.app_settings.minimap_position = side.position();
                        editor.minimap_motion.side = side;
                        editor.minimap_motion.fraction = 1.0;
                        cx.notify();
                    });
                });
                cx.update(|window, cx| window.draw(cx).clear());
                let full = cx
                    .debug_bounds("minimap-animated-lane-0")
                    .expect("full minimap");
                let content = cx.debug_bounds("editor-content-0").expect("editor");
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.app_settings.minimap_position = MinimapPosition::Off;
                        editor.minimap_motion.fraction = 0.5;
                        cx.notify();
                    });
                });
                cx.update(|window, cx| window.draw(cx).clear());
                let half = cx
                    .debug_bounds("minimap-animated-lane-0")
                    .expect("closing minimap");
                let inner = cx
                    .debug_bounds("minimap-full-lane-0")
                    .expect("full-width clipped content");
                let resized = cx.debug_bounds("editor-content-0").expect("resized editor");
                assert_eq!(half.size.width, full.size.width * 0.5);
                assert_eq!(inner.size.width, full.size.width);
                assert_eq!(cx.debug_bounds("tab-bar-0"), Some(tab));
                if side == MinimapSide::Left {
                    assert_eq!(half.left(), full.left());
                    assert_eq!(inner.left(), half.left());
                    assert_eq!(resized.left(), content.left() - half.size.width);
                } else {
                    assert_eq!(half.right(), full.right());
                    assert_eq!(inner.right(), half.right());
                    assert_eq!(resized.right(), content.right() + half.size.width);
                }
            }
        }

        #[gpui::test]
        fn minimap_button_persists_and_restores_the_selected_side(cx: &mut gpui::TestAppContext) {
            const CHILD_FLAG: &str = "MOEKA_TEST_MINIMAP_BUTTON_CHILD";
            if std::env::var_os(CHILD_FLAG).is_none() {
                let config = tempfile::tempdir().unwrap();
                let module = module_path!()
                    .split_once("::")
                    .map_or(module_path!(), |(_, rest)| rest);
                let test_name =
                    format!("{module}::minimap_button_persists_and_restores_the_selected_side");
                let result = std::process::Command::new(std::env::current_exe().unwrap())
                    .arg("--exact")
                    .arg(test_name)
                    .arg("--nocapture")
                    .env(CHILD_FLAG, config.path())
                    .env("XDG_CONFIG_HOME", config.path())
                    .env("XDG_CACHE_HOME", config.path().join("cache"))
                    .env("APPDATA", config.path())
                    .env("LOCALAPPDATA", config.path().join("cache"))
                    .env("MOEKA_PROFILE_CPU", "0")
                    .output()
                    .unwrap();
                assert!(
                    result.status.success()
                        && String::from_utf8_lossy(&result.stdout).contains("1 passed"),
                    "isolated minimap button test failed:\n{}\n{}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
                return;
            }

            let config = std::path::PathBuf::from(std::env::var_os(CHILD_FLAG).unwrap());
            assert_eq!(
                super::super::super::config_paths::config_dir(),
                config.join("moeka"),
                "child settings must be isolated from the parent test process"
            );
            cx.executor().allow_parking();
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some("# Example"), false);
                    editor.embedded_editor = false;
                    editor.app_settings.simple_mode = true;
                    editor.app_settings.minimap_position = MinimapPosition::Left;
                    editor.app_settings.minimap_last_side = None;
                    editor.minimap_motion = MinimapMotion::new(MinimapPosition::Left, None);
                    editor.styles.ui.spec.animations_enabled = false;
                    editor.last_window_width = 1000.0;
                    editor.last_window_height = 700.0;
                    editor
                })
            });
            let (_, cx) = cx.add_window_view(|_, cx| {
                let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
                Workspace(editor.clone(), subscription)
            });
            let button = cx
                .debug_bounds("tab-bar-minimap-toggle")
                .expect("minimap button");
            let click = |cx: &mut gpui::VisualTestContext| {
                cx.simulate_event(MouseDownEvent {
                    button: MouseButton::Left,
                    position: button.center(),
                    click_count: 1,
                    ..Default::default()
                });
            };
            let wait_for_saved = |cx: &mut gpui::VisualTestContext, expected| {
                for _ in 0..200 {
                    cx.run_until_parked();
                    if super::super::super::config_paths::settings_file_path().exists() {
                        let saved =
                            super::super::super::config_store::load_settings_strict().unwrap();
                        if saved.minimap_position == expected
                            && saved.minimap_last_side == Some(MinimapSide::Left)
                        {
                            return;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                panic!("minimap settings were not persisted");
            };

            click(cx);
            wait_for_saved(cx, MinimapPosition::Off);
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.minimap_motion.fraction, 0.0);
                assert_eq!(
                    editor.app_settings.minimap_last_side,
                    Some(MinimapSide::Left)
                );
            });
            click(cx);
            wait_for_saved(cx, MinimapPosition::Left);
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.minimap_motion.fraction, 1.0);
                assert_eq!(editor.minimap_motion.side, MinimapSide::Left);
            });
        }

        #[gpui::test]
        fn minimap_setting_reversal_cancels_drags_and_respects_reduced_motion(
            cx: &mut gpui::TestAppContext,
        ) {
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some("# Example"), false);
                    editor.app_settings.minimap_position = MinimapPosition::Left;
                    editor.app_settings.minimap_last_side = None;
                    editor.minimap_motion = MinimapMotion::new(MinimapPosition::Left, None);
                    editor.styles.ui.spec.animations_enabled = true;
                    editor.embedded_editor = false;
                    editor
                })
            });
            editor.update(cx, |editor, cx| {
                let drag = Drag {
                    tab: editor.active_tab_id.clone(),
                    start_y: 1.0,
                    current_y: 2.0,
                    start_thumb: 0.0,
                };
                editor.minimap_state.drag = Some(drag.clone());
                let mut pane = super::super::super::pane::Pane::new(1);
                pane.state.active_file_kind_cache = Some(super::super::super::FileKind::Markdown);
                pane.state.minimap_state.drag = Some(drag);
                editor.panes.push(pane);

                editor.set_minimap_position(MinimapPosition::Off, cx);
                assert_eq!(
                    editor.app_settings.minimap_last_side,
                    Some(MinimapSide::Left)
                );
                assert!(editor.minimap_state.drag.is_none());
                assert!(editor.panes[1].state.minimap_state.drag.is_none());
                assert!(editor.minimap_motion_task.is_some());

                editor.minimap_motion.fraction = 0.5;
                let expected_width = editor.minimap_lane_width() * 0.5;
                assert_eq!(editor.minimap_width(1000.0), expected_width);
                assert_eq!(
                    editor.with_pane(1, |editor| editor.minimap_width(1000.0)),
                    expected_width
                );
                editor.set_minimap_position(MinimapPosition::Right, cx);
                assert_eq!(editor.minimap_motion.side, MinimapSide::Left);
                assert_eq!(
                    editor.app_settings.minimap_last_side,
                    Some(MinimapSide::Right)
                );
                assert!(editor.minimap_motion_task.is_some());

                editor.styles.ui.spec.animations_enabled = false;
                editor.sync_minimap_motion(cx);
                assert!(editor.minimap_motion_task.is_none());
                assert_eq!(editor.minimap_motion.side, MinimapSide::Right);
                assert_eq!(editor.minimap_motion.fraction, 1.0);
                editor.set_minimap_position(MinimapPosition::Off, cx);
                assert_eq!(editor.minimap_motion.fraction, 0.0);
                assert_eq!(
                    editor
                        .app_settings
                        .minimap_position
                        .toggled(editor.app_settings.minimap_last_side),
                    MinimapPosition::Right
                );
            });
        }

        #[gpui::test]
        fn minimap_motion_tasks_complete_hide_show_and_side_change(cx: &mut gpui::TestAppContext) {
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    let mut editor =
                        EditorView::new_with_mode(cx, true, false, Some("# Example"), false);
                    editor.styles.ui.spec.animations_enabled = true;
                    editor
                        .styles
                        .utility_panel
                        .utility_panel_visibility_animation_ms = 1;
                    editor.app_settings.minimap_position = MinimapPosition::Left;
                    editor.minimap_motion = MinimapMotion::new(MinimapPosition::Left, None);
                    editor
                })
            });
            let finish_frame = |cx: &mut gpui::TestAppContext| {
                cx.run_until_parked();
                std::thread::sleep(Duration::from_millis(5));
                cx.executor().advance_clock(Duration::from_millis(8));
                cx.run_until_parked();
            };
            editor.update(cx, |editor, cx| {
                editor.set_minimap_position(MinimapPosition::Off, cx);
                assert!(editor.minimap_motion_task.is_some());
            });
            finish_frame(cx);
            cx.update(|cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.minimap_motion.fraction, 0.0);
                assert!(editor.minimap_motion_task.is_none());
            });
            editor.update(cx, |editor, cx| {
                editor.set_minimap_position(MinimapPosition::Left, cx);
            });
            finish_frame(cx);
            cx.update(|cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.minimap_motion.fraction, 1.0);
                assert_eq!(editor.minimap_motion.side, MinimapSide::Left);
            });
            editor.update(cx, |editor, cx| {
                editor.set_minimap_position(MinimapPosition::Right, cx);
            });
            finish_frame(cx);
            cx.update(|cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.minimap_motion.fraction, 0.0);
                assert_eq!(editor.minimap_motion.side, MinimapSide::Right);
            });
            finish_frame(cx);
            cx.update(|cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.minimap_motion.fraction, 1.0);
                assert_eq!(editor.minimap_motion.side, MinimapSide::Right);
                assert!(editor.minimap_motion_task.is_none());
            });
        }
    }

    #[test]
    fn snapshot_limits_cover_dense_code_without_exceeding_window_memory_budget() {
        assert!(snapshot_item_limit(1) >= 16_384);
        for panes in 1..=64 {
            let limit = snapshot_item_limit(panes);
            assert!(limit <= MAX_QUADS);
            let allocated_capacity = limit.next_power_of_two();
            assert!(allocated_capacity * Snapshot::item_size() * 4 * panes <= WINDOW_MARK_BYTES);
        }
    }

    #[test]
    fn minimap_supports_all_document_text_kinds_but_not_embedded_or_binary_views() {
        for kind in [
            super::super::FileKind::Markdown,
            super::super::FileKind::Text,
            super::super::FileKind::Code {
                language: "rust".into(),
            },
        ] {
            assert!(supports_minimap(Some(&kind), false));
            assert!(!supports_minimap(Some(&kind), true));
        }
        assert!(!supports_minimap(
            Some(&super::super::FileKind::Image),
            false
        ));
        assert!(!supports_minimap(
            Some(&super::super::FileKind::Binary),
            false
        ));
        assert!(!supports_minimap(None, false));
    }

    #[test]
    fn minimap_width_is_independent_of_tab_controls_and_spacing() {
        let theme = crate::theme::ThemeKind::OneDark.build();
        let mut styles = crate::style::Styles::from_theme(&theme, &theme);
        for scale in [1.0, 1.25, 2.0] {
            for padding in [0.0, 9.0, 30.0] {
                styles.tab_bar.tab_action_button_padding_x_px = padding;
                styles.tab_bar.tab_item_padding_right_px = padding;
                assert_eq!(styles.minimap.lane_width(scale), 96.0 * scale);
            }
        }
        styles.minimap.width_px = 128.0;
        assert_eq!(styles.minimap.lane_width(1.5), 192.0);
    }

    #[test]
    fn minimap_settings_label_uses_navigation_name() {
        assert_eq!(
            crate::i18n::settings_label(crate::i18n::UiLanguage::ChineseSimplified, "minimap"),
            "Minimap 导航"
        );
        assert_eq!(
            crate::i18n::settings_label(crate::i18n::UiLanguage::English, "minimap"),
            "Minimap Navigation"
        );
        let keys = ["minimap_left", "minimap_right", "minimap_off"];
        for language in crate::i18n::UiLanguage::supported() {
            let labels = keys.map(|key| crate::i18n::settings_label(*language, key));
            assert!(labels.iter().all(|label| !label.is_empty()));
            assert_ne!(labels[0], labels[1]);
            assert_ne!(labels[1], labels[2]);
            assert_ne!(
                crate::i18n::minimap_action_label(*language, false),
                crate::i18n::minimap_action_label(*language, true)
            );
        }
        assert_eq!(
            keys.map(|key| crate::i18n::settings_label(
                crate::i18n::UiLanguage::ChineseSimplified,
                key
            )),
            ["左侧", "右侧", "关闭"]
        );
    }

    #[test]
    fn handle_hit_test_is_shared_by_hover_and_drag_and_stays_inside_lane() {
        let mut state = MinimapState::default();
        state.bounds = Some(Bounds::new(
            point(px(100.0), px(20.0)),
            size(px(96.0), px(600.0)),
        ));
        state.projection = Some(Projection::new(10_000.0, 600.0, 600.0, 0.0, 0.1));
        assert!(state.handle_contains(point(px(110.0), px(30.0)), 18.0));
        assert!(!state.handle_contains(point(px(110.0), px(100.0)), 18.0));
        assert!(!state.handle_contains(point(px(99.0), px(30.0)), 18.0));
        assert!(!state.handle_contains(point(px(110.0), px(19.0)), 18.0));
        state.bounds = None;
        assert!(!state.handle_contains(point(px(110.0), px(30.0)), 18.0));
    }

    #[test]
    fn minimap_opacity_and_handle_states_are_resolved_by_style() {
        for kind in crate::theme::ThemeKind::light_themes()
            .iter()
            .chain(crate::theme::ThemeKind::dark_themes())
        {
            let theme = kind.build();
            let mut style = crate::style::Styles::from_theme(&theme, &theme).minimap;
            assert_eq!(style.opacity, 0.4);
            let normal = style.handle_color(false, false);
            let hover = style.handle_color(true, false);
            let pressed = style.handle_color(true, true);
            assert!(normal.a > 0.0);
            assert!(hover.a > normal.a);
            assert!(pressed.a > hover.a);
            assert_eq!(style.handle_color(false, true), pressed);
            style.colors.viewport_hover = gpui::hsla(0.7, 0.6, 0.5, 0.9);
            assert_eq!(style.handle_color(true, false), style.colors.viewport_hover);
        }
    }

    #[test]
    fn snapshots_are_bounded_and_invalidated_for_resize_theme_and_tab_changes() {
        let mut state = MinimapState::default();
        let snapshot = |scroll, width, background| {
            Snapshot::new(
                Bounds::new(point(px(0.0), px(0.0)), size(px(width), px(600.0))),
                scroll,
                background,
                100,
            )
        };
        for scroll in 0..20 {
            state.publish(
                Some("one".into()),
                snapshot(scroll as f64, 800.0, gpui::white()),
            );
        }
        assert_eq!(state.snapshots.len(), MAX_SNAPSHOTS);
        state.publish(Some("one".into()), snapshot(19.0, 800.0, gpui::white()));
        assert_eq!(state.snapshots.len(), MAX_SNAPSHOTS);
        state.publish(Some("one".into()), snapshot(19.0, 900.0, gpui::white()));
        assert_eq!(state.snapshots.len(), 1);
        state.publish(Some("one".into()), snapshot(20.0, 900.0, gpui::black()));
        assert_eq!(state.snapshots.len(), 1);
        state.publish(Some("two".into()), snapshot(21.0, 900.0, gpui::black()));
        assert_eq!(state.snapshots.len(), 1);
        state.clear_snapshot();
        assert!(state.snapshots.is_empty());
    }

    #[test]
    fn snapshot_uses_one_scale_for_both_axes_and_fits_the_editor_width() {
        for width in [400.0, 700.0, 1000.0, 20_000.0] {
            let scale = snapshot_scale(width, 108.0, 4.0);
            assert!((width as f64 * scale - 100.0).abs() < 0.0001);
        }
        assert_eq!(snapshot_scale(400.0, 108.0, 4.0), 0.25);
    }

    #[test]
    fn minimap_background_has_exactly_twenty_percent_alpha() {
        for kind in crate::theme::ThemeKind::light_themes()
            .iter()
            .chain(crate::theme::ThemeKind::dark_themes())
        {
            assert_eq!(kind.build().minimap_palette().placeholder.a, 0.2);
        }
    }

    #[test]
    fn initial_minimap_range_covers_rows_beyond_the_editor_viewport() {
        let buffer = crate::buffer::Buffer::from_text(&"paragraph\n\n".repeat(300));
        let decorations = crate::editor::parse_decorations_for_file_kind(
            buffer.text(),
            &crate::editor::FileKind::Markdown,
        );
        let projection = Projection::new(
            14_400.0,
            800.0,
            800.0,
            0.0,
            snapshot_scale(700.0, 104.0, 4.0),
        );
        let last =
            ((projection.origin + projection.track) / projection.scale / 24.0).ceil() as usize;
        let range = bounded_render_rows(&buffer, &decorations, 0..last, None).unwrap();
        assert_eq!(range.start, 0);
        assert!(range.end > 200);
        assert!(range.end <= MAX_PREPARE_ROWS);
    }

    #[test]
    fn minimap_preparation_rejects_unbounded_blocks_and_byte_counts() {
        let buffer = crate::buffer::Buffer::from_text(&"paragraph\n".repeat(2000));
        let rows = vec![crate::markdown::LineDecoration::default(); buffer.line_count()];
        assert!(bounded_render_rows(&buffer, &rows, 0..100, Some(&[])).is_none());
        assert_eq!(
            bounded_render_rows(&buffer, &rows, 0..100, None),
            Some(0..100)
        );
        let fenced = crate::buffer::Buffer::from_text(&"text\n\n".repeat(2000));
        let rows = vec![crate::markdown::LineDecoration::default(); fenced.line_count()];
        assert!(
            bounded_render_rows(
                &fenced,
                &rows,
                0..100,
                Some(std::slice::from_ref(&(0..4000)))
            )
            .is_none()
        );
        let long = crate::buffer::Buffer::from_text(&"x".repeat(MAX_PREPARE_BYTES + 1));
        assert!(
            bounded_render_rows(
                &long,
                &[crate::markdown::LineDecoration::default()],
                0..1,
                None
            )
            .is_none()
        );
    }

    #[test]
    fn projection_round_trips_two_million_lines_without_float_pixel_loss() {
        let height = 2_000_000.0 * 24.0;
        for scroll in [0.0, 12345678.25, height - 800.0] {
            let p = Projection::new(height, 800.0, 800.0, scroll, 0.125);
            assert!((p.drag_scroll(p.thumb_top) - scroll).abs() < 0.001);
            assert!((p.click_scroll(p.thumb_top + p.thumb_height / 2.0) - scroll).abs() < 0.001);
        }
    }

    #[test]
    fn short_documents_and_outside_pointer_positions_are_clamped() {
        for height in [0.0, 400.0, 1200.0] {
            let p = Projection::new(height, 800.0, 800.0, 0.0, 0.125);
            assert_eq!(p.drag_scroll(-100.0), 0.0);
            assert_eq!(p.click_scroll(-100.0), 0.0);
            assert_eq!(p.drag_scroll(10000.0), (height - 800.0).max(0.0));
        }
    }

    #[test]
    fn stationary_drag_tracks_updated_height_without_moving_the_grab_point() {
        let old = Projection::new(1_000_000.0, 800.0, 600.0, 500_000.0, 0.125);
        let next = Projection::new(1_500_000.0, 800.0, 600.0, 500_000.0, 0.125);
        let target = next.drag_scroll(old.thumb_top);
        let painted = Projection::new(next.height, next.viewport, next.track, target, next.scale);
        assert!((painted.thumb_top - old.thumb_top).abs() < 0.001);
    }

    #[test]
    fn rectangle_budget_is_bounded_across_panes() {
        for panes in [1, 2, 8, 128, 4096] {
            let per_pane = MAX_QUADS.min(WINDOW_MARK_BYTES / 32 / panes / Snapshot::item_size());
            assert!(per_pane * 32 * panes * Snapshot::item_size() <= WINDOW_MARK_BYTES);
        }
    }
}
