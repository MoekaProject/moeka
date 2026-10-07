use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Hsla, MouseButton, MouseDownEvent, Pixels, Point,
    ScrollHandle, Task, canvas, div, point, prelude::*, px,
};

use crate::style::{EditorStyles, Styles};

use super::EditorView;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PanelScrollbarKind {
    Sidebar,
    WorkspaceSearch,
    FileList,
    Info,
    Outline,
    GitHistory,
    GitChanges,
    ChatTerminal,
}

impl PanelScrollbarKind {
    pub(crate) const ALL: [Self; 8] = [
        Self::Sidebar,
        Self::WorkspaceSearch,
        Self::FileList,
        Self::Info,
        Self::Outline,
        Self::GitHistory,
        Self::GitChanges,
        Self::ChatTerminal,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar-scrollbar",
            Self::WorkspaceSearch => "workspace-search-scrollbar",
            Self::FileList => "file-list-scrollbar",
            Self::Info => "info-scrollbar",
            Self::Outline => "outline-panel-scrollbar",
            Self::GitHistory => "git-history-scrollbar",
            Self::GitChanges => "git-changes-scrollbar",
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

    fn desired_opacity(&self, can_show: bool, styles: &EditorStyles) -> f32 {
        if !can_show {
            0.0
        } else if self.hovered || self.drag.is_some() {
            styles.editor_scrollbar_hover_opacity
        } else if self.revealed {
            styles.editor_scrollbar_opacity
        } else {
            styles.editor_scrollbar_idle_opacity
        }
    }
}

fn panel_scrollbar_thumb_color(kind: PanelScrollbarKind, opacity: f32, styles: &Styles) -> Hsla {
    // Sidebar-side panels sit on the dark chrome palette even in light
    // mode, so their thumb must not use the (dark) editor text color.
    let mut color = match kind {
        PanelScrollbarKind::Sidebar
        | PanelScrollbarKind::WorkspaceSearch
        | PanelScrollbarKind::FileList => styles.sidebar.sidebar_scrollbar_thumb_color,
        _ => styles.editor.editor_scrollbar_thumb_color,
    };
    color.a *= opacity.clamp(0.0, 1.0);
    color
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

impl EditorView {
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
        let hit_width = self.scaled(self.styles.editor.editor_scrollbar_hit_width_px);
        let radius = self.scaled(self.styles.editor.editor_scrollbar_radius_px);
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
                    div()
                        .absolute()
                        .top(px(geometry.thumb_top))
                        .right(px(right_px))
                        .w(px(width))
                        .h(px(geometry.thumb_height))
                        .cursor(CursorStyle::Arrow)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.start_panel_scrollbar_drag(kind, event.position, cx);
                            }),
                        )
                        .rounded(px(radius))
                        .bg(thumb_color),
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
            PanelScrollbarKind::GitHistory => {
                let metrics =
                    VerticalScrollbarMetrics::from_scroll_handle(&self.git_history_scroll_handle)?;
                let track_height_px =
                    f32::from(self.git_history_scroll_handle.bounds().size.height);
                (metrics, track_height_px, 0.0)
            }
            PanelScrollbarKind::GitChanges => {
                let metrics =
                    VerticalScrollbarMetrics::from_scroll_handle(&self.git_changes_scroll_handle)?;
                let track_height_px =
                    f32::from(self.git_changes_scroll_handle.bounds().size.height);
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
        let top_margin = self.scaled(self.styles.editor.editor_scrollbar_top_margin_px);
        let bottom_margin = self.scaled(self.styles.editor.editor_scrollbar_bottom_margin_px);
        let track_top = top_px + top_margin;
        let track_height =
            (track_height_px.max(metrics.viewport_height) - top_margin - bottom_margin).max(1.0);
        let min_thumb_height = self.scaled(self.styles.editor.editor_scrollbar_min_thumb_height_px);
        let thumb_height = (track_height * (metrics.viewport_height / metrics.content_height()))
            .clamp(min_thumb_height.min(track_height), track_height);
        let track_travel = (track_height - thumb_height).max(0.0);
        let thumb_top = if track_travel <= 0.0 {
            0.0
        } else {
            (metrics.scroll_top / metrics.max_scroll).clamp(0.0, 1.0) * track_travel
        };

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
            PanelScrollbarKind::GitHistory => Some(&self.git_history_scroll_handle),
            PanelScrollbarKind::GitChanges => Some(&self.git_changes_scroll_handle),
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

        state.desired_opacity(self.panel_scrollbar_can_show(kind), &self.styles.editor)
    }

    fn desired_panel_scrollbar_width(&self, kind: PanelScrollbarKind) -> f32 {
        let Some(state) = self.panel_scrollbar_state(kind) else {
            return self.styles.editor.editor_scrollbar_width_px;
        };

        if state.hovered || state.drag.is_some() {
            self.styles.editor.editor_scrollbar_hover_width_px
        } else {
            self.styles.editor.editor_scrollbar_width_px
        }
    }

    fn sync_panel_scrollbar_opacity(&mut self, kind: PanelScrollbarKind, cx: &mut Context<Self>) {
        let target = self.desired_panel_scrollbar_opacity(kind);
        let duration_ms = self.styles.editor.editor_scrollbar_fade_animation_ms.max(1);
        let step_ms = self
            .styles
            .editor
            .editor_scrollbar_animation_frame_ms
            .max(1);
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
        let duration_ms = self
            .styles
            .editor
            .editor_scrollbar_width_animation_ms
            .max(1);
        let step_ms = self
            .styles
            .editor
            .editor_scrollbar_animation_frame_ms
            .max(1);
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
        const HOVER_REVEAL_DELAY_MS: u64 = 100;
        let Some(state) = self.panel_scrollbar_state_mut(kind) else {
            return;
        };

        state.hover_epoch = state.hover_epoch.wrapping_add(1);
        let epoch = state.hover_epoch;
        state.hover_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(HOVER_REVEAL_DELAY_MS))
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
        let idle_delay_ms = self.styles.editor.editor_scrollbar_idle_delay_ms;
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
            | PanelScrollbarKind::Outline
            | PanelScrollbarKind::GitHistory
            | PanelScrollbarKind::GitChanges => {
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
            assert_eq!(styles.editor.editor_scrollbar_idle_opacity, 0.0);
            for kind in PanelScrollbarKind::ALL {
                let mut state = PanelScrollbarState::new(
                    styles.editor.editor_scrollbar_idle_opacity,
                    styles.editor.editor_scrollbar_width_px,
                );
                for hovered in [false, true] {
                    for revealed in [false, true] {
                        for dragging in [false, true] {
                            state.hovered = hovered;
                            state.revealed = revealed;
                            state.drag = dragging.then(drag_state);
                            let expected = if hovered || dragging {
                                styles.editor.editor_scrollbar_hover_opacity
                            } else if revealed {
                                styles.editor.editor_scrollbar_opacity
                            } else {
                                0.0
                            };
                            assert_eq!(state.desired_opacity(true, &styles.editor), expected);
                            state.opacity = expected;
                            let color = panel_scrollbar_thumb_color(kind, state.opacity, &styles);
                            if hovered || revealed || dragging {
                                assert!(color.a > 0.0, "{kind:?} should be visible");
                            } else {
                                assert_eq!(color.a, 0.0, "{kind:?} should be hidden at idle");
                            }
                            assert_eq!(state.desired_opacity(false, &styles.editor), 0.0);
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
                        state.desired_opacity(true, &styles.editor),
                        if inside { 0.3 } else { 0.2 },
                        "{kind:?}"
                    );
                    state.revealed = false;
                    assert_eq!(
                        state.desired_opacity(true, &styles.editor),
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
                | PanelScrollbarKind::FileList => styles.sidebar.sidebar_scrollbar_thumb_color,
                _ => styles.editor.editor_scrollbar_thumb_color,
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
        styles.editor.editor_scrollbar_idle_opacity = 0.1;
        let state = PanelScrollbarState::new(0.1, styles.editor.editor_scrollbar_width_px);
        assert_eq!(state.desired_opacity(true, &styles.editor), 0.1);
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
