use crate::ui::self_drawn::ScrollbarControl;
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Hsla, MouseButton, MouseDownEvent, Pixels, Point,
    ScrollHandle, Task, canvas, div, point, prelude::*, px,
};

use crate::style::{ScrollbarStyles, Styles};

use super::{EditorScrollbarDragState, EditorView};

impl EditorScrollbarDragState {
    fn scroll_offset(&self, track_travel: f32, max_scroll: f32) -> f32 {
        if track_travel <= 0.0 || max_scroll <= 0.0 {
            return 0.0;
        }
        // Retain the pointer's original grab point in track coordinates, but
        // map it through the CURRENT range. Async previews can change both the
        // thumb size and the range even while the pointer is stationary.
        let thumb_top = self.start_thumb_top + self.position_y - self.start_position_y;
        (thumb_top / track_travel).clamp(0.0, 1.0) * max_scroll
    }
}

impl EditorView {
    pub(super) fn refresh_editor_scrollbar_drag_after_layout(&mut self) -> bool {
        let Some(drag) = self.editor_scrollbar_drag else {
            return false;
        };
        let track_travel = self
            .editor_scrollbar_thumb_geometry()
            .map(|(_, height)| (self.editor_scrollbar_track_height() - height).max(0.0))
            .unwrap_or(0.0);
        let target = drag.scroll_offset(track_travel, self.editor_scrollbar_max_scroll());
        let changed = (target - self.scroll_offset).abs() > 0.5;
        self.scroll_offset = target;
        changed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PanelScrollbarKind {
    Sidebar,
    WorkspaceSearch,
    FileList,
    Info,
    Outline,
    ChatTerminal,
}

impl PanelScrollbarKind {
    pub(crate) const ALL: [Self; 6] = [
        Self::Sidebar,
        Self::WorkspaceSearch,
        Self::FileList,
        Self::Info,
        Self::Outline,
        Self::ChatTerminal,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar-scrollbar",
            Self::WorkspaceSearch => "workspace-search-scrollbar",
            Self::FileList => "file-list-scrollbar",
            Self::Info => "info-scrollbar",
            Self::Outline => "outline-panel-scrollbar",
            Self::ChatTerminal => "chat-terminal-scrollbar",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PanelScrollbarDragState {
    start_position_y: f32,
    start_thumb_top: f32,
    track_travel: f32,
    max_scroll: f32,
}

pub(crate) struct PanelScrollbarState {
    hovered: bool,
    revealed: bool,
    opacity: f32,
    opacity_target: f32,
    opacity_epoch: usize,
    opacity_task: Option<Task<()>>,
    width: f32,
    width_target: f32,
    width_epoch: usize,
    width_task: Option<Task<()>>,
    hover_epoch: usize,
    hover_task: Option<Task<()>>,
    idle_epoch: usize,
    idle_task: Option<Task<()>>,
    track_bounds: Option<Bounds<Pixels>>,
    drag: Option<PanelScrollbarDragState>,
}

impl PanelScrollbarState {
    pub(crate) fn new(opacity: f32, width: f32) -> Self {
        Self {
            hovered: false,
            revealed: false,
            opacity,
            opacity_target: opacity,
            opacity_epoch: 0,
            opacity_task: None,
            width,
            width_target: width,
            width_epoch: 0,
            width_task: None,
            hover_epoch: 0,
            hover_task: None,
            idle_epoch: 0,
            idle_task: None,
            track_bounds: None,
            drag: None,
        }
    }

    fn reconcile_style(&mut self, styles: &ScrollbarStyles, can_show: bool) {
        self.opacity_epoch = self.opacity_epoch.wrapping_add(1);
        self.width_epoch = self.width_epoch.wrapping_add(1);
        self.opacity_task = None;
        self.width_task = None;
        self.opacity = self.desired_opacity(can_show, styles);
        self.opacity_target = self.opacity;
        self.width = ScrollbarControl::new(*styles, 1.0).width(self.hovered || self.drag.is_some());
        self.width_target = self.width;
    }

    fn stop_drag(&mut self, position: Point<Pixels>, can_show: bool) {
        self.drag = None;
        self.hovered = can_show
            && self
                .track_bounds
                .is_some_and(|bounds| bounds.contains(&position));
        if !can_show {
            self.revealed = false;
        }
    }

    fn desired_opacity(&self, can_show: bool, styles: &ScrollbarStyles) -> f32 {
        ScrollbarControl::new(*styles, 1.0).opacity(
            can_show,
            self.hovered || self.drag.is_some(),
            self.revealed,
        )
    }
}

fn panel_scrollbar_thumb_color(kind: PanelScrollbarKind, opacity: f32, styles: &Styles) -> Hsla {
    ScrollbarControl::new(styles.self_drawn.scrollbar, 1.0).color(
        matches!(
            kind,
            PanelScrollbarKind::Sidebar
                | PanelScrollbarKind::WorkspaceSearch
                | PanelScrollbarKind::FileList
        ),
        opacity,
    )
}

#[derive(Clone, Copy)]
struct VerticalScrollbarMetrics {
    viewport_height: f32,
    max_scroll: f32,
    scroll_top: f32,
}

impl VerticalScrollbarMetrics {
    fn from_scroll_handle(scroll_handle: &ScrollHandle) -> Option<Self> {
        let viewport_height = f32::from(scroll_handle.bounds().size.height);
        let max_scroll = f32::from(scroll_handle.max_offset().height);
        let scroll_top = -f32::from(scroll_handle.offset().y);
        Self::new(viewport_height, max_scroll, scroll_top)
    }

    fn new(viewport_height: f32, max_scroll: f32, scroll_top: f32) -> Option<Self> {
        if viewport_height <= 1.0 || max_scroll <= 1.0 {
            return None;
        }

        Some(Self {
            viewport_height,
            max_scroll,
            scroll_top: scroll_top.clamp(0.0, max_scroll),
        })
    }

    fn content_height(self) -> f32 {
        self.viewport_height + self.max_scroll
    }
}

#[derive(Clone, Copy)]
struct VerticalScrollbarGeometry {
    metrics: VerticalScrollbarMetrics,
    track_top: f32,
    track_height: f32,
    thumb_top: f32,
    thumb_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlStylePaneStep {
    Enter(super::PaneId),
    Leave(super::PaneId),
    Refresh,
}

/// Layout-dependent scrollbar visibility must use the pane's existing view
/// helpers. Restore the caller's routing around that traversal rather than
/// nesting another swap or approximating visibility from stale opacity values.
fn control_style_pane_steps(
    focused: super::PaneId,
    swapped: Option<super::PaneId>,
    panes: &[super::PaneId],
) -> Vec<ControlStylePaneStep> {
    let mut steps = Vec::new();
    if let Some(pane) = swapped {
        steps.push(ControlStylePaneStep::Leave(pane));
    }
    for &pane in panes {
        if pane != focused {
            steps.push(ControlStylePaneStep::Enter(pane));
        }
        steps.push(ControlStylePaneStep::Refresh);
        if pane != focused {
            steps.push(ControlStylePaneStep::Leave(pane));
        }
    }
    if let Some(pane) = swapped {
        steps.push(ControlStylePaneStep::Enter(pane));
    }
    steps
}

impl EditorView {
    /// Reconcile presentation state without rebuilding documents, losing a drag,
    /// or allowing an old animation task to install metrics from the prior spec.
    pub(crate) fn sync_control_style_metrics(&mut self, cx: &mut Context<Self>) {
        let styles = self.styles.self_drawn.scrollbar;
        for kind in PanelScrollbarKind::ALL {
            let can_show = self.panel_scrollbar_can_show(kind);
            let (pending_hover, pending_idle) =
                if let Some(state) = self.panel_scrollbar_state_mut(kind) {
                    state.reconcile_style(&styles, can_show);
                    (state.hover_task.is_some(), state.idle_task.is_some())
                } else {
                    continue;
                };
            if pending_hover {
                self.schedule_panel_scrollbar_hover_reveal(kind, cx);
            }
            if pending_idle {
                self.schedule_panel_scrollbar_idle_fade(kind, cx);
            }
        }
        for step in
            control_style_pane_steps(self.focused_pane_id, self.swapped_pane_id, &self.pane_ids())
        {
            match step {
                ControlStylePaneStep::Enter(pane) => {
                    self.begin_pane_swap(pane);
                }
                ControlStylePaneStep::Leave(pane) => self.end_pane_swap(pane),
                ControlStylePaneStep::Refresh => self.sync_current_pane_control_style_metrics(cx),
            }
        }
    }

    fn sync_current_pane_control_style_metrics(&mut self, cx: &mut Context<Self>) {
        self.editor_scrollbar_opacity_epoch = self.editor_scrollbar_opacity_epoch.wrapping_add(1);
        self.editor_scrollbar_width_epoch = self.editor_scrollbar_width_epoch.wrapping_add(1);
        self.editor_scrollbar_opacity_task = None;
        self.editor_scrollbar_width_task = None;
        self.editor_scrollbar_opacity = self.desired_editor_scrollbar_opacity();
        self.editor_scrollbar_opacity_target = self.editor_scrollbar_opacity;
        self.editor_scrollbar_width = self.desired_editor_scrollbar_width();
        self.editor_scrollbar_width_target = self.editor_scrollbar_width;
        if self.editor_scrollbar_idle_task.is_some() {
            self.schedule_editor_scrollbar_idle_fade(cx);
        }
        self.code_block_horizontal_scrollbar_opacity_epoch = self
            .code_block_horizontal_scrollbar_opacity_epoch
            .wrapping_add(1);
        self.code_block_horizontal_scrollbar_thickness_epoch = self
            .code_block_horizontal_scrollbar_thickness_epoch
            .wrapping_add(1);
        self.code_block_horizontal_scrollbar_opacity_task = None;
        self.code_block_horizontal_scrollbar_thickness_task = None;
        self.code_block_horizontal_scrollbar_opacity =
            self.desired_code_block_horizontal_scrollbar_opacity();
        self.code_block_horizontal_scrollbar_opacity_target =
            self.code_block_horizontal_scrollbar_opacity;
        self.code_block_horizontal_scrollbar_thickness =
            self.desired_code_block_horizontal_scrollbar_thickness();
        self.code_block_horizontal_scrollbar_thickness_target =
            self.code_block_horizontal_scrollbar_thickness;
        self.gutter_fold_reveal_epoch = self.gutter_fold_reveal_epoch.wrapping_add(1);
        self.gutter_fold_reveal_task = None;
        self.gutter_fold_reveal = self.gutter_fold_reveal_target;
        self.sync_line_control_style_metrics();
        self.clamp_preview_overlay_view();
    }

    fn panel_scrollbar_state(&self, kind: PanelScrollbarKind) -> Option<&PanelScrollbarState> {
        self.panel_scrollbars.get(&kind)
    }

    fn panel_scrollbar_state_mut(
        &mut self,
        kind: PanelScrollbarKind,
    ) -> Option<&mut PanelScrollbarState> {
        self.panel_scrollbars.get_mut(&kind)
    }

    pub(crate) fn render_scrollbar_for_handle(
        &self,
        kind: PanelScrollbarKind,
        scroll_handle: &ScrollHandle,
        track_height_px: f32,
        top_px: f32,
        right_px: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let metrics = VerticalScrollbarMetrics::from_scroll_handle(scroll_handle)?;
        self.render_scrollbar_for_metrics(
            kind,
            metrics.viewport_height,
            metrics.max_scroll,
            metrics.scroll_top,
            track_height_px,
            top_px,
            right_px,
            cx,
        )
    }

    pub(crate) fn render_scrollbar_for_metrics(
        &self,
        kind: PanelScrollbarKind,
        viewport_height: f32,
        max_scroll: f32,
        scroll_top: f32,
        track_height_px: f32,
        top_px: f32,
        right_px: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let metrics = VerticalScrollbarMetrics::new(viewport_height, max_scroll, scroll_top)?;
        let geometry =
            self.panel_scrollbar_geometry_from_metrics(metrics, track_height_px, top_px)?;
        let state = self.panel_scrollbar_state(kind)?;
        let width = self.scaled(state.width);
        let control = ScrollbarControl::new(self.styles.self_drawn.scrollbar, self.scaled(1.0));
        let hit_width = control.hit_width();
        let thumb_color = panel_scrollbar_thumb_color(kind, state.opacity, &self.styles);
        let editor = cx.entity().clone();

        Some(
            div()
                .id(kind.id())
                .absolute()
                .top(px(geometry.track_top))
                .right(px(0.0))
                .h(px(geometry.track_height))
                .w(px(hit_width + right_px))
                .cursor(CursorStyle::Arrow)
                .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                    this.set_panel_scrollbar_hovered(kind, *hovering, cx);
                }))
                .child(
                    canvas(
                        move |bounds, window, cx| {
                            let bounds = bounds.intersect(&window.content_mask().bounds);
                            editor.update(cx, |this, _| {
                                if let Some(state) = this.panel_scrollbar_state_mut(kind) {
                                    state.track_bounds = (!bounds.is_empty()).then_some(bounds);
                                }
                            });
                        },
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .child(
                    control
                        .thumb(width, geometry.thumb_height, thumb_color)
                        .top(px(geometry.thumb_top))
                        .right(px(right_px))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.start_panel_scrollbar_drag(kind, event.position, cx);
                            }),
                        ),
                )
                .into_any_element(),
        )
    }

    fn panel_scrollbar_geometry(
        &self,
        kind: PanelScrollbarKind,
    ) -> Option<VerticalScrollbarGeometry> {
        let (metrics, track_height_px, top_px) = match kind {
            PanelScrollbarKind::Sidebar => {
                let metrics = VerticalScrollbarMetrics::from_scroll_handle(
                    self.active_sidebar_scrollbar_handle(),
                )?;
                (metrics, self.sidebar_scrollbar_track_height(), 0.0)
            }
            PanelScrollbarKind::WorkspaceSearch => {
                let metrics = VerticalScrollbarMetrics::from_scroll_handle(
                    &self.workspace_search_scroll_handle,
                )?;
                (
                    metrics,
                    f32::from(self.workspace_search_scroll_handle.bounds().size.height),
                    0.0,
                )
            }
            PanelScrollbarKind::FileList => {
                let metrics =
                    VerticalScrollbarMetrics::from_scroll_handle(&self.file_list_scroll_handle)?;
                (metrics, self.last_window_height, 0.0)
            }
            PanelScrollbarKind::Info => {
                let metrics =
                    VerticalScrollbarMetrics::from_scroll_handle(&self.info_panel_scroll_handle)?;
                let track_height_px = f32::from(self.info_panel_scroll_handle.bounds().size.height);
                (metrics, track_height_px, 0.0)
            }
            PanelScrollbarKind::Outline => {
                let metrics = VerticalScrollbarMetrics::from_scroll_handle(
                    &self.outline_panel_scroll_handle,
                )?;
                let track_height_px =
                    f32::from(self.outline_panel_scroll_handle.bounds().size.height);
                (metrics, track_height_px, 0.0)
            }
            PanelScrollbarKind::ChatTerminal => {
                let (viewport_height, max_scroll, scroll_top) =
                    self.active_chat_terminal_scrollbar_metrics()?;
                let metrics =
                    VerticalScrollbarMetrics::new(viewport_height, max_scroll, scroll_top)?;
                (metrics, self.chat_terminal_scrollbar_track_height(), 0.0)
            }
        };

        self.panel_scrollbar_geometry_from_metrics(metrics, track_height_px, top_px)
    }

    fn panel_scrollbar_geometry_from_metrics(
        &self,
        metrics: VerticalScrollbarMetrics,
        track_height_px: f32,
        top_px: f32,
    ) -> Option<VerticalScrollbarGeometry> {
        let top_margin = self.scaled(self.styles.self_drawn.scrollbar.top_margin_px);
        let bottom_margin = self.scaled(self.styles.self_drawn.scrollbar.bottom_margin_px);
        let track_top = top_px + top_margin;
        let track_height =
            (track_height_px.max(metrics.viewport_height) - top_margin - bottom_margin).max(1.0);
        let (thumb_top, thumb_height, _) =
            ScrollbarControl::new(self.styles.self_drawn.scrollbar, self.scaled(1.0))
                .thumb_geometry(
                    track_height,
                    metrics.viewport_height,
                    metrics.content_height(),
                    metrics.scroll_top,
                    metrics.max_scroll,
                    true,
                );

        Some(VerticalScrollbarGeometry {
            metrics,
            track_top,
            track_height,
            thumb_top,
            thumb_height,
        })
    }

    fn active_sidebar_scrollbar_handle(&self) -> &ScrollHandle {
        &self.sidebar_scroll_handle
    }

    pub(crate) fn sidebar_scrollbar_track_height(&self) -> f32 {
        (self.last_window_height - self.status_bar_height()).max(1.0)
    }

    pub(crate) fn chat_terminal_scrollbar_track_height(&self) -> f32 {
        let terminal_padding =
            self.scaled(self.styles.utility_panel.utility_panel_terminal_padding_px);
        self.active_terminal_tab_id()
            .and_then(|tab_id| self.terminal_tab_state(&tab_id))
            .and_then(|state| state.input_bounds)
            .map(|bounds| f32::from(bounds.size.height) + terminal_padding * 2.0)
            .unwrap_or(self.last_window_height)
            .max(1.0)
    }

    fn panel_scrollbar_handle(&self, kind: PanelScrollbarKind) -> Option<&ScrollHandle> {
        match kind {
            PanelScrollbarKind::Sidebar => Some(self.active_sidebar_scrollbar_handle()),
            PanelScrollbarKind::WorkspaceSearch => Some(&self.workspace_search_scroll_handle),
            PanelScrollbarKind::FileList => Some(&self.file_list_scroll_handle),
            PanelScrollbarKind::Info => Some(&self.info_panel_scroll_handle),
            PanelScrollbarKind::Outline => Some(&self.outline_panel_scroll_handle),
            PanelScrollbarKind::ChatTerminal => None,
        }
    }

    fn panel_scrollbar_can_show(&self, kind: PanelScrollbarKind) -> bool {
        self.panel_scrollbar_geometry(kind).is_some()
    }

    fn desired_panel_scrollbar_opacity(&self, kind: PanelScrollbarKind) -> f32 {
        let Some(state) = self.panel_scrollbar_state(kind) else {
            return 0.0;
        };

        state.desired_opacity(
            self.panel_scrollbar_can_show(kind),
            &self.styles.self_drawn.scrollbar,
        )
    }

    fn desired_panel_scrollbar_width(&self, kind: PanelScrollbarKind) -> f32 {
        let Some(state) = self.panel_scrollbar_state(kind) else {
            return self.styles.self_drawn.scrollbar.width_px;
        };

        if state.hovered || state.drag.is_some() {
            self.styles.self_drawn.scrollbar.hover_width_px
        } else {
            self.styles.self_drawn.scrollbar.width_px
        }
    }

    fn sync_panel_scrollbar_opacity(&mut self, kind: PanelScrollbarKind, cx: &mut Context<Self>) {
        let target = self.desired_panel_scrollbar_opacity(kind);
        let duration_ms = self.styles.self_drawn.scrollbar.fade_animation_ms.max(1);
        let step_ms = self.styles.self_drawn.scrollbar.animation_frame_ms.max(1);
        let Some(state) = self.panel_scrollbar_state_mut(kind) else {
            return;
        };

        if (state.opacity_target - target).abs() < 0.001 && (state.opacity - target).abs() < 0.001 {
            state.opacity = target;
            state.opacity_target = target;
            state.opacity_task = None;
            return;
        }

        state.opacity_target = target;
        state.opacity_epoch = state.opacity_epoch.wrapping_add(1);
        let epoch = state.opacity_epoch;
        state.opacity_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(step_ms))
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.panel_scrollbar_state_mut(kind) else {
                    return;
                };
                if state.opacity_epoch != epoch {
                    return;
                }

                let target = state.opacity_target;
                if duration_ms == 0 || (state.opacity - target).abs() < 0.01 {
                    state.opacity = target;
                    state.opacity_task = None;
                    cx.notify();
                    return;
                }

                let progress = (step_ms as f32 / duration_ms as f32).clamp(0.05, 1.0);
                state.opacity += (target - state.opacity) * progress;
                cx.notify();
                this.sync_panel_scrollbar_opacity(kind, cx);
            });
        }));
    }

    fn sync_panel_scrollbar_width(&mut self, kind: PanelScrollbarKind, cx: &mut Context<Self>) {
        let target = self.desired_panel_scrollbar_width(kind);
        let duration_ms = self.styles.self_drawn.scrollbar.width_animation_ms.max(1);
        let step_ms = self.styles.self_drawn.scrollbar.animation_frame_ms.max(1);
        let Some(state) = self.panel_scrollbar_state_mut(kind) else {
            return;
        };

        if (state.width_target - target).abs() < 0.001 && (state.width - target).abs() < 0.001 {
            state.width = target;
            state.width_target = target;
            state.width_task = None;
            return;
        }

        state.width_target = target;
        state.width_epoch = state.width_epoch.wrapping_add(1);
        let epoch = state.width_epoch;
        state.width_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(step_ms))
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.panel_scrollbar_state_mut(kind) else {
                    return;
                };
                if state.width_epoch != epoch {
                    return;
                }

                let target = state.width_target;
                if duration_ms == 0 || (state.width - target).abs() < 0.1 {
                    state.width = target;
                    state.width_task = None;
                    cx.notify();
                    return;
                }

                let progress = (step_ms as f32 / duration_ms as f32).clamp(0.05, 1.0);
                state.width += (target - state.width) * progress;
                cx.notify();
                this.sync_panel_scrollbar_width(kind, cx);
            });
        }));
    }

    fn cancel_panel_scrollbar_idle_fade(&mut self, kind: PanelScrollbarKind) {
        if let Some(state) = self.panel_scrollbar_state_mut(kind) {
            state.idle_epoch = state.idle_epoch.wrapping_add(1);
            state.idle_task = None;
        }
    }

    fn cancel_panel_scrollbar_hover_reveal(&mut self, kind: PanelScrollbarKind) {
        if let Some(state) = self.panel_scrollbar_state_mut(kind) {
            state.hover_epoch = state.hover_epoch.wrapping_add(1);
            state.hover_task = None;
        }
    }

    fn schedule_panel_scrollbar_hover_reveal(
        &mut self,
        kind: PanelScrollbarKind,
        cx: &mut Context<Self>,
    ) {
        let hover_reveal_delay_ms = self.styles.self_drawn.scrollbar.hover_reveal_delay_ms;
        let Some(state) = self.panel_scrollbar_state_mut(kind) else {
            return;
        };

        state.hover_epoch = state.hover_epoch.wrapping_add(1);
        let epoch = state.hover_epoch;
        state.hover_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(hover_reveal_delay_ms))
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.panel_scrollbar_state_mut(kind) else {
                    return;
                };
                if state.hover_epoch != epoch || !state.hovered || state.drag.is_some() {
                    return;
                }
                state.revealed = true;
                state.hover_task = None;
                this.cancel_panel_scrollbar_idle_fade(kind);
                this.sync_panel_scrollbar_opacity(kind, cx);
                this.sync_panel_scrollbar_width(kind, cx);
                cx.notify();
            });
        }));
    }

    fn schedule_panel_scrollbar_idle_fade(
        &mut self,
        kind: PanelScrollbarKind,
        cx: &mut Context<Self>,
    ) {
        let idle_delay_ms = self.styles.self_drawn.scrollbar.idle_delay_ms;
        let Some(state) = self.panel_scrollbar_state_mut(kind) else {
            return;
        };

        state.idle_epoch = state.idle_epoch.wrapping_add(1);
        let epoch = state.idle_epoch;
        state.idle_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(idle_delay_ms))
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.panel_scrollbar_state_mut(kind) else {
                    return;
                };
                if state.idle_epoch != epoch || state.hovered || state.drag.is_some() {
                    return;
                }
                state.revealed = false;
                state.idle_task = None;
                this.sync_panel_scrollbar_opacity(kind, cx);
            });
        }));
    }

    pub(crate) fn reveal_panel_scrollbar(
        &mut self,
        kind: PanelScrollbarKind,
        cx: &mut Context<Self>,
    ) {
        if !self.panel_scrollbar_can_show(kind) {
            if let Some(state) = self.panel_scrollbar_state_mut(kind) {
                state.hovered = false;
                state.revealed = false;
                state.track_bounds = None;
                state.drag = None;
            }
            self.cancel_panel_scrollbar_hover_reveal(kind);
            self.cancel_panel_scrollbar_idle_fade(kind);
            self.sync_panel_scrollbar_opacity(kind, cx);
            self.sync_panel_scrollbar_width(kind, cx);
            return;
        }

        if let Some(state) = self.panel_scrollbar_state_mut(kind) {
            state.revealed = true;
        }
        self.cancel_panel_scrollbar_idle_fade(kind);
        let target = self.desired_panel_scrollbar_opacity(kind);
        if let Some(state) = self.panel_scrollbar_state_mut(kind) {
            state.opacity_epoch = state.opacity_epoch.wrapping_add(1);
            state.opacity_task = None;
            state.opacity = target;
            state.opacity_target = target;
        }
        self.sync_panel_scrollbar_width(kind, cx);
        let should_schedule = self
            .panel_scrollbar_state(kind)
            .map(|state| !state.hovered && state.drag.is_none())
            .unwrap_or(false);
        if should_schedule {
            self.schedule_panel_scrollbar_idle_fade(kind, cx);
        }
        cx.notify();
    }

    pub(crate) fn set_panel_scrollbar_hovered(
        &mut self,
        kind: PanelScrollbarKind,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.panel_scrollbar_state(kind) else {
            return;
        };
        if state.hovered == hovered {
            return;
        }

        if let Some(state) = self.panel_scrollbar_state_mut(kind) {
            state.hovered = hovered;
        }
        if hovered {
            self.cancel_panel_scrollbar_idle_fade(kind);
            self.schedule_panel_scrollbar_hover_reveal(kind, cx);
        } else {
            self.cancel_panel_scrollbar_hover_reveal(kind);
            let should_schedule = self
                .panel_scrollbar_state(kind)
                .map(|state| state.revealed && state.drag.is_none())
                .unwrap_or(false);
            if should_schedule {
                self.schedule_panel_scrollbar_idle_fade(kind, cx);
            }
        }
        self.sync_panel_scrollbar_opacity(kind, cx);
        self.sync_panel_scrollbar_width(kind, cx);
        cx.notify();
    }

    pub(crate) fn start_panel_scrollbar_drag(
        &mut self,
        kind: PanelScrollbarKind,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(geometry) = self.panel_scrollbar_geometry(kind) else {
            return;
        };
        let track_travel = (geometry.track_height - geometry.thumb_height).max(0.0);
        if track_travel <= 0.0 {
            return;
        }

        if let Some(state) = self.panel_scrollbar_state_mut(kind) {
            state.hovered = true;
            state.revealed = true;
            state.drag = Some(PanelScrollbarDragState {
                start_position_y: f32::from(position.y),
                start_thumb_top: geometry.thumb_top,
                track_travel,
                max_scroll: geometry.metrics.max_scroll,
            });
        }
        self.cancel_panel_scrollbar_hover_reveal(kind);
        self.cancel_panel_scrollbar_idle_fade(kind);
        self.sync_panel_scrollbar_opacity(kind, cx);
        self.sync_panel_scrollbar_width(kind, cx);
        cx.notify();
    }

    pub(crate) fn update_panel_scrollbar_drag(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((kind, drag)) = PanelScrollbarKind::ALL.into_iter().find_map(|kind| {
            self.panel_scrollbar_state(kind)
                .and_then(|state| state.drag.map(|drag| (kind, drag)))
        }) else {
            return false;
        };

        let delta = f32::from(position.y) - drag.start_position_y;
        let thumb_top = (drag.start_thumb_top + delta).clamp(0.0, drag.track_travel);
        let scroll_top = if drag.track_travel <= 0.0 {
            0.0
        } else {
            (thumb_top / drag.track_travel) * drag.max_scroll
        };
        self.set_panel_scrollbar_scroll_top(kind, scroll_top, cx);
        true
    }

    pub(crate) fn stop_panel_scrollbar_drag(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(kind) = PanelScrollbarKind::ALL.into_iter().find(|kind| {
            self.panel_scrollbar_state(*kind)
                .map(|state| state.drag.is_some())
                .unwrap_or(false)
        }) else {
            return false;
        };

        let can_show = self.panel_scrollbar_can_show(kind);
        if let Some(state) = self.panel_scrollbar_state_mut(kind) {
            state.stop_drag(position, can_show);
        }
        self.cancel_panel_scrollbar_hover_reveal(kind);
        let should_schedule = self
            .panel_scrollbar_state(kind)
            .map(|state| state.revealed && !state.hovered)
            .unwrap_or(false);
        if should_schedule {
            self.schedule_panel_scrollbar_idle_fade(kind, cx);
        } else {
            self.cancel_panel_scrollbar_idle_fade(kind);
        }
        self.sync_panel_scrollbar_opacity(kind, cx);
        self.sync_panel_scrollbar_width(kind, cx);
        cx.notify();
        true
    }

    fn set_panel_scrollbar_scroll_top(
        &mut self,
        kind: PanelScrollbarKind,
        scroll_top: f32,
        cx: &mut Context<Self>,
    ) {
        match kind {
            PanelScrollbarKind::ChatTerminal => {
                self.set_active_chat_terminal_scrollbar_top(scroll_top, cx);
            }
            PanelScrollbarKind::Sidebar
            | PanelScrollbarKind::WorkspaceSearch
            | PanelScrollbarKind::FileList
            | PanelScrollbarKind::Info
            | PanelScrollbarKind::Outline => {
                if let Some(handle) = self.panel_scrollbar_handle(kind) {
                    let max_scroll = f32::from(handle.max_offset().height);
                    let scroll_top = scroll_top.clamp(0.0, max_scroll);
                    let offset = handle.offset();
                    handle.set_offset(point(offset.x, px(-scroll_top)));
                    cx.notify();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use crate::{style::Styles, theme::Theme};

    use super::{
        PanelScrollbarDragState, PanelScrollbarKind, PanelScrollbarState, VerticalScrollbarMetrics,
        panel_scrollbar_thumb_color,
    };

    #[test]
    fn editor_scrollbar_drag_tracks_current_range_during_async_preview_reflow() {
        let drag = super::EditorScrollbarDragState {
            start_position_y: 320.0,
            start_thumb_top: 200.0,
            position_y: 320.0,
        };
        // Neither the pointer nor the desired thumb top moves while previews
        // complete. The same drag must use each new range, not mouse-down's.
        for (travel, max_scroll) in [(400.0, 1_000.0), (450.0, 4_000.0), (300.0, 600.0)] {
            let offset = drag.scroll_offset(travel, max_scroll);
            assert!((offset / max_scroll * travel - 200.0).abs() < 0.001);
            assert_eq!(drag.scroll_offset(travel, max_scroll), offset);
        }
    }

    #[test]
    fn editor_scrollbar_drag_reaches_new_bottom_and_clamps_after_content_shrinks() {
        let mut drag = super::EditorScrollbarDragState {
            start_position_y: 320.0,
            start_thumb_top: 200.0,
            position_y: 570.0,
        };
        assert_eq!(drag.scroll_offset(450.0, 4_000.0), 4_000.0);
        assert_eq!(drag.scroll_offset(300.0, 600.0), 600.0);
        assert_eq!(drag.scroll_offset(0.0, 600.0), 0.0);
        assert_eq!(drag.scroll_offset(450.0, 0.0), 0.0);
        drag.position_y = 0.0;
        assert_eq!(drag.scroll_offset(450.0, 4_000.0), 0.0);
    }

    fn drag_state() -> PanelScrollbarDragState {
        PanelScrollbarDragState {
            start_position_y: 0.0,
            start_thumb_top: 0.0,
            track_travel: 100.0,
            max_scroll: 200.0,
        }
    }

    #[test]
    fn panel_scrollbars_hide_when_idle_and_show_when_hovered_revealed_or_dragged() {
        for theme in [Theme::one_light(), Theme::one_dark()] {
            let styles = Styles::from_theme(&theme, &theme);
            assert_eq!(styles.self_drawn.scrollbar.idle_opacity, 0.0);
            for kind in PanelScrollbarKind::ALL {
                let mut state = PanelScrollbarState::new(
                    styles.self_drawn.scrollbar.idle_opacity,
                    styles.self_drawn.scrollbar.width_px,
                );
                for hovered in [false, true] {
                    for revealed in [false, true] {
                        for dragging in [false, true] {
                            state.hovered = hovered;
                            state.revealed = revealed;
                            state.drag = dragging.then(drag_state);
                            let expected = if hovered || dragging {
                                styles.self_drawn.scrollbar.hover_opacity
                            } else if revealed {
                                styles.self_drawn.scrollbar.opacity
                            } else {
                                0.0
                            };
                            assert_eq!(
                                state.desired_opacity(true, &styles.self_drawn.scrollbar),
                                expected
                            );
                            state.opacity = expected;
                            let color = panel_scrollbar_thumb_color(kind, state.opacity, &styles);
                            if hovered || revealed || dragging {
                                assert!(color.a > 0.0, "{kind:?} should be visible");
                            } else {
                                assert_eq!(color.a, 0.0, "{kind:?} should be hidden at idle");
                            }
                            assert_eq!(
                                state.desired_opacity(false, &styles.self_drawn.scrollbar),
                                0.0
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn panel_scrollbar_drag_release_reconciles_hover_with_current_track_bounds() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);
        let bounds = Bounds::new(point(px(100.0), px(20.0)), size(px(11.0), px(200.0)));
        for kind in PanelScrollbarKind::ALL {
            for (position, inside) in [
                (point(px(105.0), px(100.0)), true),
                (point(px(99.0), px(100.0)), false),
                (point(px(112.0), px(100.0)), false),
                (point(px(105.0), px(19.0)), false),
                (point(px(105.0), px(221.0)), false),
            ] {
                for stale_hover in [false, true] {
                    let mut state = PanelScrollbarState::new(0.3, 6.0);
                    state.track_bounds = Some(bounds);
                    state.hovered = stale_hover;
                    state.revealed = true;
                    state.drag = Some(drag_state());
                    state.stop_drag(position, true);
                    assert!(state.drag.is_none());
                    assert_eq!(state.hovered, inside, "{kind:?}");
                    assert!(state.revealed);
                    assert_eq!(
                        state.desired_opacity(true, &styles.self_drawn.scrollbar),
                        if inside { 0.3 } else { 0.2 },
                        "{kind:?}"
                    );
                    state.revealed = false;
                    assert_eq!(
                        state.desired_opacity(true, &styles.self_drawn.scrollbar),
                        if inside { 0.3 } else { 0.0 },
                        "{kind:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn panel_scrollbar_drag_release_clears_hover_without_a_scrollable_track() {
        let position = point(px(105.0), px(100.0));
        for can_show in [false, true] {
            let mut state = PanelScrollbarState::new(0.3, 6.0);
            state.hovered = true;
            state.revealed = true;
            state.drag = Some(drag_state());
            if !can_show {
                state.track_bounds = Some(Bounds::new(
                    point(px(100.0), px(20.0)),
                    size(px(11.0), px(200.0)),
                ));
            }
            state.stop_drag(position, can_show);
            assert!(state.drag.is_none());
            assert!(!state.hovered);
            assert_eq!(state.revealed, can_show);
        }
    }

    #[test]
    fn panel_scrollbar_thumb_preserves_palette_and_has_no_opacity_floor() {
        let theme = Theme::one_light();
        let styles = Styles::from_theme(&theme, &theme);
        for kind in PanelScrollbarKind::ALL {
            let base = match kind {
                PanelScrollbarKind::Sidebar
                | PanelScrollbarKind::WorkspaceSearch
                | PanelScrollbarKind::FileList => styles.self_drawn.scrollbar.sidebar_thumb_color,
                _ => styles.self_drawn.scrollbar.thumb_color,
            };
            for opacity in [-1.0_f32, 0.0, 0.01, 0.2, 0.3, 1.0, 2.0] {
                let mut expected = base;
                expected.a *= opacity.clamp(0.0, 1.0);
                assert_eq!(
                    panel_scrollbar_thumb_color(kind, opacity, &styles),
                    expected
                );
            }
        }
    }

    #[test]
    fn panel_scrollbar_respects_configured_idle_opacity() {
        let theme = Theme::one_dark();
        let mut styles = Styles::from_theme(&theme, &theme);
        styles.self_drawn.scrollbar.idle_opacity = 0.1;
        let state = PanelScrollbarState::new(0.1, styles.self_drawn.scrollbar.width_px);
        assert_eq!(
            state.desired_opacity(true, &styles.self_drawn.scrollbar),
            0.1
        );
    }

    #[test]
    fn panel_scrollbar_metrics_hide_without_overflow_and_clamp_scroll_position() {
        assert!(VerticalScrollbarMetrics::new(200.0, 0.0, 0.0).is_none());
        assert!(VerticalScrollbarMetrics::new(200.0, 1.0, 0.0).is_none());
        assert!(VerticalScrollbarMetrics::new(1.0, 200.0, 0.0).is_none());
        let above = VerticalScrollbarMetrics::new(200.0, 300.0, -10.0).unwrap();
        let below = VerticalScrollbarMetrics::new(200.0, 300.0, 400.0).unwrap();
        assert_eq!(above.scroll_top, 0.0);
        assert_eq!(below.scroll_top, 300.0);
        assert_eq!(below.content_height(), 500.0);
    }
}

#[cfg(test)]
mod style_reconciliation_tests {
    use super::*;
    #[test]
    fn self_drawn_reconciliation_retargets_without_losing_scrollbar_drag() {
        let theme = crate::theme::ThemeKind::OneDark.build();
        let mut spec = crate::style::UiStyleSpec::default();
        spec.self_drawn.scrollbar.width_px = 10.0;
        spec.self_drawn.scrollbar.hover_width_px = 14.0;
        spec.self_drawn.scrollbar.hover_opacity = 0.7;
        let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
        let mut state = PanelScrollbarState::new(0.1, 6.0);
        state.drag = Some(PanelScrollbarDragState {
            start_position_y: 12.0,
            start_thumb_top: 15.0,
            track_travel: 100.0,
            max_scroll: 500.0,
        });
        let epoch = state.width_epoch;
        state.reconcile_style(&styles.self_drawn.scrollbar, true);
        assert_eq!((state.width, state.width_target), (14.0, 14.0));
        assert_eq!((state.opacity, state.opacity_target), (0.7, 0.7));
        assert!(state.width_epoch > epoch);
        assert_eq!(state.drag.unwrap().start_thumb_top, 15.0);
        let original = Styles::from_theme(&theme, &theme);
        state.reconcile_style(&original.self_drawn.scrollbar, true);
        assert_eq!((state.width, state.opacity), (6.0, 0.3));
    }
}

#[cfg(test)]
mod pane_refresh_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn self_drawn_refresh_visits_every_document_and_restores_normal_or_swapped_slots() {
        let focused = 1;
        let ids = [7, 1, 3];
        for swapped in [None, Some(7), Some(3)] {
            // The focused slot is always a hollow placeholder. In a temporary
            // swap the selected slot parks the focused document, not itself.
            let mut root = swapped.unwrap_or(focused);
            let mut slots = HashMap::from([(1, 99), (7, 7), (3, 3)]);
            if let Some(pane) = swapped {
                slots.insert(pane, focused);
            }
            let original_root = root;
            let original_slots = slots.clone();
            let mut active_swap = swapped;
            let mut visited = Vec::new();
            for step in control_style_pane_steps(focused, swapped, &ids) {
                match step {
                    ControlStylePaneStep::Enter(pane) => {
                        assert_eq!(active_swap, None, "a refresh must never nest pane swaps");
                        std::mem::swap(&mut root, slots.get_mut(&pane).unwrap());
                        active_swap = Some(pane);
                    }
                    ControlStylePaneStep::Leave(pane) => {
                        assert_eq!(active_swap, Some(pane));
                        std::mem::swap(&mut root, slots.get_mut(&pane).unwrap());
                        active_swap = None;
                    }
                    ControlStylePaneStep::Refresh => visited.push(root),
                }
            }
            assert_eq!(visited, ids, "each live document is refreshed exactly once");
            assert_eq!(root, original_root);
            assert_eq!(slots, original_slots);
            assert_eq!(active_swap, swapped);
            assert_eq!(slots[&focused], 99, "the hollow slot is never refreshed");
        }
    }
}
