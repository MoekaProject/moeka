//! Floating formatting chrome driven by the final text-selection paint geometry.
//!
//! The owner is absolute/full-pane but inserts no hitbox. Only the measured bar
//! returned by menus.rs is interactive; that bar must occlude pointer events and
//! preserve editor focus/selection. Build it in deferred prepaint, not render:
//! EditorElement has then published the current frame's clipped selection quads.
//! The bar subtree must not itself contain a deferred element: GPUI disallows
//! enqueueing deferred draws while processing the deferred prepaint queue.

use std::ops::Range;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, Entity, Focusable,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, MouseMoveEvent, PaintQuad, Pixels,
    Point, Position, Size, Style, Window, point, px, relative, size,
};

use super::{EditorView, PaneId};

pub(crate) struct SelectionToolbarAnimation {
    pane_id: PaneId,
    geometry: SelectionToolbarGeometry,
    from: f32,
    pub(crate) visible: bool,
    started: std::time::Instant,
}

impl SelectionToolbarAnimation {
    fn opacity(&self) -> f32 {
        let progress = (self.started.elapsed().as_secs_f32() / 0.14).clamp(0.0, 1.0);
        let target = if self.visible { 1.0 } else { 0.0 };
        self.from + (target - self.from) * super::ease_in_out_cubic(progress)
    }
}

#[derive(Clone)]
pub(crate) struct SelectionToolbarPopup {
    pane_id: PaneId,
    selection: SelectionToolbarSelectionKey,
    origin: Point<Pixels>,
    menu_owned: bool,
}

/// Escape dismissal follows the selection, not its scroll position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectionToolbarSelectionKey {
    pub(crate) tab_id: String,
    pub(crate) primary_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectionToolbarSnapshotKey {
    pub(crate) selection: SelectionToolbarSelectionKey,
    pub(crate) cursor_at_end: bool,
    pub(crate) scroll_offset: f32,
    /// All code-block pans, not merely the block containing the caret. A
    /// multi-line primary selection can include several independently panned blocks.
    pub(crate) horizontal_offsets: Vec<(usize, f32)>,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectionToolbarGeometry {
    pub(crate) key: SelectionToolbarSnapshotKey,
    /// Unclipped EditorElement bounds, used to reject resized/moved cached views.
    pub(crate) editor_bounds: Bounds<Pixels>,
    /// Effective pane viewport after ancestor clipping (excludes the tab strip).
    pub(crate) viewport: Bounds<Pixels>,
    pub(crate) selected_bounds: Bounds<Pixels>,
    pub(crate) top_row_anchor: Bounds<Pixels>,
    pub(crate) cursor_endpoint_x: Pixels,
    /// Set only after the floating bar actually prepaints, for Escape routing.
    pub(crate) toolbar_bounds: Option<Bounds<Pixels>>,
}

#[derive(Clone, Debug)]
pub(crate) struct LineFormatToolbarGeometry {
    pub(crate) line: usize,
    pub(crate) viewport: Bounds<Pixels>,
    pub(crate) content_left: Pixels,
    pub(crate) trigger_bounds: Bounds<Pixels>,
    pub(crate) line_top: Pixels,
    pub(crate) line_bottom: Pixels,
}

impl LineFormatToolbarGeometry {
    fn toolbar_origin(
        &self,
        measured: Size<Pixels>,
        margin: Pixels,
        gap: Pixels,
    ) -> Option<Point<Pixels>> {
        if !positive_bounds(Bounds::new(point(px(0.0), px(0.0)), measured))
            || measured.width + margin * 2.0 > self.viewport.size.width
            || measured.height + margin * 2.0 > self.viewport.size.height
        {
            return None;
        }
        let left = self.viewport.left() + margin;
        let right = self.viewport.right() - margin - measured.width;
        let x = self.content_left.clamp(left, right);
        let above = self.line_top - gap - measured.height;
        let below = self.line_bottom + gap;
        let y = if above >= self.viewport.top() + margin {
            above
        } else if below + measured.height <= self.viewport.bottom() - margin {
            below
        } else {
            return None;
        };
        Some(point(x, y))
    }
}

fn positive_bounds(bounds: Bounds<Pixels>) -> bool {
    f32::from(bounds.origin.x).is_finite()
        && f32::from(bounds.origin.y).is_finite()
        && f32::from(bounds.size.width).is_finite()
        && f32::from(bounds.size.height).is_finite()
        && bounds.size.width > px(0.0)
        && bounds.size.height > px(0.0)
}

fn visible_intersection(a: Bounds<Pixels>, b: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
    let clipped = a.intersect(&b);
    positive_bounds(clipped).then_some(clipped)
}

impl SelectionToolbarGeometry {
    pub(crate) fn from_quads(
        key: SelectionToolbarSnapshotKey,
        editor_bounds: Bounds<Pixels>,
        ancestor_clip: Bounds<Pixels>,
        horizontal_clip: Option<Bounds<Pixels>>,
        selections: &[PaintQuad],
        clipped_code_block_selections: &[(Bounds<Pixels>, Vec<PaintQuad>)],
    ) -> Option<Self> {
        let viewport = visible_intersection(editor_bounds, ancestor_clip)?;
        // Match the masks used in EditorElement::paint exactly. Table cell
        // selections live in `selections`; code-block masks are independent of
        // the optional ordinary-text horizontal mask.
        let ordinary_clip = horizontal_clip.unwrap_or(viewport).intersect(&viewport);
        let visible = selections
            .iter()
            .filter_map(|quad| visible_intersection(quad.bounds, ordinary_clip))
            .chain(
                clipped_code_block_selections
                    .iter()
                    .flat_map(|(clip, quads)| {
                        let clip = clip.intersect(&viewport);
                        quads
                            .iter()
                            .filter_map(move |quad| visible_intersection(quad.bounds, clip))
                    }),
            )
            .collect::<Vec<_>>();
        Self::from_visible_bounds(key, editor_bounds, viewport, &visible)
    }

    fn from_visible_bounds(
        key: SelectionToolbarSnapshotKey,
        editor_bounds: Bounds<Pixels>,
        viewport: Bounds<Pixels>,
        visible: &[Bounds<Pixels>],
    ) -> Option<Self> {
        let first = *visible.iter().min_by_key(|quad| quad.top())?;
        let selected_bounds = visible.iter().copied().reduce(|a, b| a.union(&b))?;
        // Use vertical overlap with the earliest quad, rather than emission
        // order: tables and wrapped selections can be emitted out of order.
        let top_row_anchor = visible
            .iter()
            .copied()
            .filter(|quad| quad.top() < first.bottom() && quad.bottom() > first.top())
            .fold(first, |row, quad| row.union(&quad));
        let endpoint_seed = if key.cursor_at_end {
            *visible.iter().max_by_key(|quad| quad.bottom())?
        } else {
            first
        };
        let endpoint_row = visible
            .iter()
            .copied()
            .filter(|quad| {
                quad.top() < endpoint_seed.bottom() && quad.bottom() > endpoint_seed.top()
            })
            .fold(endpoint_seed, |row, quad| row.union(&quad));
        let cursor_endpoint_x = if key.cursor_at_end {
            endpoint_row.right()
        } else {
            endpoint_row.left()
        };
        Some(Self {
            key,
            editor_bounds,
            viewport,
            selected_bounds,
            top_row_anchor,
            cursor_endpoint_x,
            toolbar_bounds: None,
        })
    }

    fn matches(&self, key: &SelectionToolbarSnapshotKey, editor_bounds: Bounds<Pixels>) -> bool {
        self.key == *key && self.editor_bounds == editor_bounds
    }

    /// Center on the first visible selected row. Prefer above; below the whole
    /// visible selection is the only safe fallback. Never clamp vertically onto
    /// selected text, even when a selection fills the pane.
    fn toolbar_origin(
        &self,
        measured: Size<Pixels>,
        margin: Pixels,
        gap: Pixels,
    ) -> Option<Point<Pixels>> {
        let viewport = self.viewport;
        if !positive_bounds(Bounds::new(point(px(0.0), px(0.0)), measured))
            || measured.width + margin * 2.0 > viewport.size.width
            || measured.height + margin * 2.0 > viewport.size.height
        {
            return None;
        }
        let left = viewport.left() + margin;
        let right = viewport.right() - margin - measured.width;
        let x = (self.cursor_endpoint_x - measured.width / 2.0).clamp(left, right);
        let above = self.top_row_anchor.top() - gap - measured.height;
        let below = self.selected_bounds.bottom() + gap;
        let y = if above >= viewport.top() + margin {
            above
        } else if below + measured.height <= viewport.bottom() - margin {
            below
        } else {
            return None;
        };
        Some(point(x, y))
    }
}

#[derive(Clone, Copy, Debug)]
struct SelectionToolbarVisibility {
    enabled: bool,
    editable_markdown: bool,
    focused: bool,
    window_active: bool,
    primary_nonempty: bool,
    extra_cursors: bool,
    selecting: bool,
    composing: bool,
    selected_handle: bool,
    table_drag: bool,
    preview_overlay: bool,
    modal_overlay: bool,
    sidebar_edit_or_focus: bool,
    search_focus: bool,
    other_drag: bool,
    dismissed: bool,
}

impl SelectionToolbarVisibility {
    fn visible(self) -> bool {
        self.enabled
            && self.editable_markdown
            && self.focused
            && self.window_active
            && self.primary_nonempty
            && !self.extra_cursors
            && !self.selecting
            && !self.composing
            && !self.selected_handle
            && !self.table_drag
            && !self.preview_overlay
            && !self.modal_overlay
            && !self.sidebar_edit_or_focus
            && !self.search_focus
            && !self.other_drag
            && !self.dismissed
    }
}

impl EditorView {
    fn desired_format_trigger_opacity(&self) -> f32 {
        if self.hovering_format_trigger_line.is_some() || self.line_format_toolbar_hovered {
            0.9
        } else if self.hovering_format_line.is_some() {
            0.34
        } else {
            0.0
        }
    }

    fn sync_format_trigger_opacity(&mut self, cx: &mut gpui::Context<Self>) {
        let target = self.desired_format_trigger_opacity();
        if (self.format_trigger_opacity_target - target).abs() <= 0.001 {
            return;
        }
        self.format_trigger_opacity_target = target;
        self.format_trigger_animation_epoch = self.format_trigger_animation_epoch.wrapping_add(1);
        let epoch = self.format_trigger_animation_epoch;
        let pane_id = self.current_pane_id();
        self.format_trigger_animation_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.format_trigger_animation_epoch != epoch {
                                return true;
                            }
                            let difference =
                                this.format_trigger_opacity_target - this.format_trigger_opacity;
                            if difference.abs() <= 0.01 {
                                this.format_trigger_opacity = this.format_trigger_opacity_target;
                                this.format_trigger_animation_task = None;
                                if this.format_trigger_opacity <= 0.0 {
                                    this.format_trigger_line = None;
                                }
                                cx.notify();
                                return true;
                            }
                            this.format_trigger_opacity += difference * 0.22;
                            cx.notify();
                            false
                        })
                        .unwrap_or(true)
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
        }));
    }

    fn desired_line_format_toolbar_opacity(&self) -> f32 {
        if self.line_format_toolbar_line.is_some()
            && (self.hovering_format_trigger_line.is_some()
                || self.line_format_toolbar_hovered
                || self.format_toolbar_owns_hover_menu())
        {
            1.0
        } else {
            0.0
        }
    }

    fn sync_line_format_toolbar_opacity(&mut self, cx: &mut gpui::Context<Self>) {
        let target = self.desired_line_format_toolbar_opacity();
        if (self.line_format_toolbar_opacity_target - target).abs() <= 0.001 {
            return;
        }
        self.line_format_toolbar_opacity_target = target;
        self.line_format_toolbar_animation_epoch =
            self.line_format_toolbar_animation_epoch.wrapping_add(1);
        let epoch = self.line_format_toolbar_animation_epoch;
        let pane_id = self.current_pane_id();
        self.line_format_toolbar_animation_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.line_format_toolbar_animation_epoch != epoch {
                                return true;
                            }
                            let difference = this.line_format_toolbar_opacity_target
                                - this.line_format_toolbar_opacity;
                            if difference.abs() <= 0.01 {
                                this.line_format_toolbar_opacity =
                                    this.line_format_toolbar_opacity_target;
                                this.line_format_toolbar_animation_task = None;
                                if this.line_format_toolbar_opacity <= 0.0 {
                                    this.line_format_toolbar_line = None;
                                    this.line_format_toolbar_bounds = None;
                                }
                                cx.notify();
                                return true;
                            }
                            this.line_format_toolbar_opacity += difference * 0.22;
                            cx.notify();
                            false
                        })
                        .unwrap_or(true)
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
        }));
    }

    pub(crate) fn update_line_format_hover(
        &mut self,
        line: Option<usize>,
        trigger_line: Option<usize>,
        cx: &mut gpui::Context<Self>,
    ) {
        if let (Some(active_line), Some(next_line)) = (self.line_format_toolbar_line, trigger_line)
            && active_line != next_line
        {
            return;
        }
        let changed =
            self.hovering_format_line != line || self.hovering_format_trigger_line != trigger_line;
        self.hovering_format_line = line;
        self.hovering_format_trigger_line = trigger_line;
        if let Some(line) = line {
            self.format_trigger_line = Some(line);
        }
        if let Some(line) = trigger_line {
            self.line_format_toolbar_line = Some(line);
        }
        self.sync_format_trigger_opacity(cx);
        self.sync_line_format_toolbar_opacity(cx);
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn set_line_format_toolbar_hovered(
        &mut self,
        hovered: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        self.line_format_toolbar_hovered = hovered;
        if hovered {
            self.hovering_format_trigger_line = None;
        }
        self.sync_format_trigger_opacity(cx);
        self.sync_line_format_toolbar_opacity(cx);
    }

    pub(crate) fn clear_line_format_hover(&mut self, cx: &mut gpui::Context<Self>) {
        self.update_line_format_hover(None, None, cx);
    }

    pub(crate) fn dismiss_line_format_toolbar(&mut self, cx: &mut gpui::Context<Self>) {
        self.hovering_format_line = None;
        self.hovering_format_trigger_line = None;
        self.format_trigger_line = None;
        self.line_format_toolbar_line = None;
        self.line_format_toolbar_bounds = None;
        self.line_format_toolbar_hovered = false;
        self.line_format_toolbar_opacity = 0.0;
        self.line_format_toolbar_opacity_target = 0.0;
        self.line_format_toolbar_animation_epoch =
            self.line_format_toolbar_animation_epoch.wrapping_add(1);
        self.line_format_toolbar_animation_task = None;
        self.sync_format_trigger_opacity(cx);
        cx.notify();
    }

    pub(crate) fn hide_selection_toolbar_for_scroll(&mut self, cx: &mut gpui::Context<Self>) {
        if self.embedded_editor || !self.active_tab_is_markdown() {
            return;
        }
        self.dismiss_line_format_toolbar(cx);
        let pane_id = self.current_pane_id();
        if let Some(key) = self.selection_toolbar_selection_key() {
            self.selection_toolbar_dismissed = Some((pane_id, key));
        }
        if self
            .selection_toolbar_popup
            .as_ref()
            .is_some_and(|popup| popup.pane_id == pane_id)
        {
            if self
                .selection_toolbar_popup
                .as_ref()
                .is_some_and(|popup| popup.menu_owned)
            {
                self.close_context_menu_immediately(cx);
            }
            self.selection_toolbar_popup = None;
        }
        if self
            .selection_toolbar_animation
            .as_ref()
            .is_some_and(|state| state.pane_id == pane_id)
        {
            self.selection_toolbar_animation = None;
        }
        if let Some(geometry) = self.selection_toolbar_geometry.get_mut(&pane_id) {
            geometry.toolbar_bounds = None;
        }
        cx.notify();
    }

    pub(crate) fn retain_selection_toolbar_after_format(&mut self) {
        let key = self.selection_toolbar_selection_key();
        if let (Some(popup), Some(key)) = (&mut self.selection_toolbar_popup, key)
            && popup.pane_id == self.focused_pane_id
            && popup.selection.tab_id == key.tab_id
        {
            popup.selection = key;
        }
    }

    pub(crate) fn mark_selection_toolbar_menu(&mut self) {
        if let Some(popup) = &mut self.selection_toolbar_popup {
            popup.menu_owned = true;
        }
    }

    pub(crate) fn clear_selection_toolbar_menu_owner(&mut self) {
        if let Some(popup) = &mut self.selection_toolbar_popup {
            popup.menu_owned = false;
        }
    }

    pub(crate) fn selection_toolbar_format_session(&self) -> bool {
        if !self
            .selection_toolbar_animation
            .as_ref()
            .is_some_and(|state| state.visible)
        {
            return false;
        }
        self.selection_toolbar_popup.as_ref().is_some_and(|popup| {
            popup.pane_id == self.focused_pane_id
                && self.selection_toolbar_selection_key().as_ref() == Some(&popup.selection)
                && (self.context_menu.is_none() || popup.menu_owned)
        })
    }

    fn selection_toolbar_selection_key(&self) -> Option<SelectionToolbarSelectionKey> {
        let primary_range = self.cursor.selection_range();
        if primary_range.is_empty() || self.has_extra_cursors() {
            return None;
        }
        Some(SelectionToolbarSelectionKey {
            tab_id: self.active_tab_id.clone()?,
            primary_range,
        })
    }

    pub(crate) fn selection_toolbar_snapshot_key(&self) -> Option<SelectionToolbarSnapshotKey> {
        if self.embedded_editor || !self.active_tab_is_markdown() {
            return None;
        }
        let selection = self.selection_toolbar_selection_key()?;
        let mut horizontal_offsets = self
            .code_block_display_overrides
            .get(&selection.tab_id)
            .into_iter()
            .flat_map(|blocks| blocks.iter())
            .filter_map(|(line, state)| {
                (state.horizontal_offset != 0.0).then_some((*line, state.horizontal_offset))
            })
            .collect::<Vec<_>>();
        horizontal_offsets.sort_unstable_by_key(|(line, _)| *line);
        Some(SelectionToolbarSnapshotKey {
            selection,
            cursor_at_end: self.cursor.offset >= self.cursor.anchor,
            scroll_offset: self.scroll_offset,
            horizontal_offsets,
        })
    }

    /// Parent's Escape handler should call this before clearing a text selection
    /// (but after dismissing a menu/dialog), and notify if it returns true.
    /// No selection, focus, menu, or document state is mutated here.
    pub(crate) fn dismiss_selection_toolbar(&mut self, window: &Window, cx: &App) -> bool {
        let pane_id = self.current_pane_id();
        let Some(key) = self.selection_toolbar_snapshot_key() else {
            return false;
        };
        if !self.selection_toolbar_visible(pane_id, window, cx)
            || !self
                .selection_toolbar_geometry
                .get(&pane_id)
                .is_some_and(|geometry| geometry.key == key && geometry.toolbar_bounds.is_some())
        {
            return false;
        }
        self.selection_toolbar_dismissed = Some((pane_id, key.selection));
        true
    }

    fn selection_toolbar_visible(&self, pane_id: PaneId, window: &Window, cx: &App) -> bool {
        if self.line_format_toolbar_line.is_some() {
            return false;
        }
        let search_focus = self.workspace_search_input_focused
            || [&self.search_query_editor, &self.search_replace_editor]
                .into_iter()
                .flatten()
                .any(|input| input.read(cx).focus_handle(cx).is_focused(window));
        SelectionToolbarVisibility {
            enabled: self.app_settings.format_toolbar_visible,
            editable_markdown: self.active_tab_is_markdown()
                && !self.active_tab_is_read_only()
                && !self.embedded_editor,
            focused: pane_id == self.focused_pane_id && self.focus_handle.is_focused(window),
            window_active: window.is_window_active(),
            primary_nonempty: self.cursor.has_selection(),
            extra_cursors: self.has_extra_cursors(),
            selecting: self.is_selecting,
            composing: self.marked_range.is_some() || !self.extra_marked_ranges.is_empty(),
            selected_handle: self.selected_handle.is_some(),
            table_drag: self.table_drag.is_some(),
            preview_overlay: self.preview_overlay_state().is_some(),
            modal_overlay: self.dialog.is_some()
                || self.settings_font_picker.is_some()
                || self.quick_open_visible
                || self.command_palette_visible
                || self.language_picker_visible
                || self.branch_picker_visible
                || self.git_remote_input_visible
                || (self.context_menu.is_some()
                    && !self.selection_toolbar_popup.as_ref().is_some_and(|popup| {
                        popup.menu_owned
                            && popup.pane_id == pane_id
                            && self.context_menu.as_ref().is_some_and(|menu| {
                                matches!(
                                    menu.kind,
                                    super::ContextMenuKind::Format
                                        | super::ContextMenuKind::FormatHeading
                                        | super::ContextMenuKind::FormatMore
                                )
                            })
                    })),
            sidebar_edit_or_focus: self.sidebar_edit.is_some()
                || self.focused_sidebar_path.is_some(),
            search_focus,
            other_drag: self.image_drag_state.is_some()
                || self.generated_preview_drag_state.is_some()
                || self.pane_resize_drag.is_some()
                || self.tab_dragging_id.is_some()
                || self.phantom_selection.is_some()
                || self.phantom_selecting,
            dismissed: self
                .selection_toolbar_dismissed
                .as_ref()
                .is_some_and(|(pane, key)| {
                    *pane == pane_id && self.selection_toolbar_selection_key().as_ref() == Some(key)
                }),
        }
        .visible()
    }
}

pub(crate) struct SelectionToolbarElement {
    pub(crate) editor: Entity<EditorView>,
    pub(crate) pane_id: PaneId,
}

pub(crate) struct SelectionToolbarPrepaintState {
    toolbar: AnyElement,
    viewport: Bounds<Pixels>,
}

impl IntoElement for SelectionToolbarElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for SelectionToolbarElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<SelectionToolbarPrepaintState>;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name(
            format!("selection-toolbar-{}", self.pane_id).into(),
        ))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.inset.left = px(0.0).into();
        style.inset.top = px(0.0).into();
        style.size = size(relative(1.0).into(), relative(1.0).into());
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Only the focused pane may own the bar. In particular, do not swap
        // state in this deferred pass or build callbacks for an unfocused pane.
        let (mut toolbar, geometry, margin, gap) = self.editor.update(cx, |editor, cx| {
            if let Some(geometry) = editor.selection_toolbar_geometry.get_mut(&self.pane_id) {
                geometry.toolbar_bounds = None;
            }
            if editor.focused_pane_id != self.pane_id {
                return None;
            }
            if editor
                .selection_toolbar_dismissed
                .as_ref()
                .is_some_and(|(pane, key)| {
                    *pane == self.pane_id
                        && editor.selection_toolbar_selection_key().as_ref() != Some(key)
                })
            {
                editor.selection_toolbar_dismissed = None;
            }
            let current_geometry = editor.selection_toolbar_snapshot_key().and_then(|key| {
                editor
                    .selection_toolbar_geometry
                    .get(&self.pane_id)
                    .filter(|geometry| geometry.matches(&key, bounds))
                    .cloned()
            });
            let visible = editor.selection_toolbar_visible(self.pane_id, window, cx)
                && current_geometry.is_some();
            if editor
                .selection_toolbar_animation
                .as_ref()
                .is_some_and(|state| {
                    state.pane_id != self.pane_id
                        || state.geometry.key.selection.tab_id
                            != editor.active_tab_id.as_deref().unwrap_or("")
                })
            {
                editor.selection_toolbar_animation = None;
                editor.selection_toolbar_popup = None;
            }
            if visible && editor.selection_toolbar_animation.is_none() {
                editor.selection_toolbar_animation = Some(SelectionToolbarAnimation {
                    pane_id: self.pane_id,
                    geometry: current_geometry.clone()?,
                    from: 0.0,
                    visible: true,
                    started: std::time::Instant::now(),
                });
            }
            let animation = editor.selection_toolbar_animation.as_mut()?;
            if animation.visible != visible {
                animation.from = animation.opacity();
                animation.visible = visible;
                animation.started = std::time::Instant::now();
            }
            if visible {
                animation.geometry = current_geometry?;
            }
            let opacity = animation.opacity();
            if !visible && opacity <= 0.0 {
                editor.selection_toolbar_animation = None;
                editor.selection_toolbar_popup = None;
                return None;
            }
            if animation.started.elapsed().as_secs_f32() < 0.14 {
                window.request_animation_frame();
            }
            let geometry = animation.geometry.clone();
            let interactive = animation.visible;
            let margin = px(editor.scaled(6.0));
            let gap = px(editor.scaled(6.0));
            let available_width = geometry.viewport.size.width - margin * 2.0;
            if available_width <= px(0.0) {
                return None;
            }
            // Parent owns compact grouping, actual bar width, occlusion and
            // selection-preserving actions. Do not wrap it in a pane hitbox.
            let toolbar = editor.render_format_toolbar(
                f32::from(available_width),
                opacity,
                interactive,
                false,
                None,
                cx,
            );
            Some((toolbar, geometry, margin, gap))
        })?;
        let measured = toolbar.layout_as_root(
            size(
                AvailableSpace::Definite(geometry.viewport.size.width - margin * 2.0),
                AvailableSpace::MinContent,
            ),
            window,
            cx,
        );
        let previous = self.editor.read(cx).selection_toolbar_popup.clone();
        let origin = if let Some(popup) = previous.filter(|popup| {
            popup.pane_id == self.pane_id && popup.selection == geometry.key.selection
        }) {
            // Formatting can reflow or resize the selection. Keep the popup's
            // original position; only clamp if the viewport can no longer fit it.
            if measured.width + margin * 2.0 > geometry.viewport.size.width
                || measured.height + margin * 2.0 > geometry.viewport.size.height
            {
                return None;
            }
            point(
                popup.origin.x.clamp(
                    geometry.viewport.left() + margin,
                    geometry.viewport.right() - margin - measured.width,
                ),
                popup.origin.y.clamp(
                    geometry.viewport.top() + margin,
                    geometry.viewport.bottom() - margin - measured.height,
                ),
            )
        } else {
            geometry.toolbar_origin(measured, margin, gap)?
        };
        window.with_content_mask(
            Some(ContentMask {
                bounds: geometry.viewport,
            }),
            |window| {
                toolbar.prepaint_at(origin, window, cx);
            },
        );
        self.editor.update(cx, |editor, _| {
            let menu_owned = editor
                .selection_toolbar_popup
                .as_ref()
                .is_some_and(|popup| {
                    popup.pane_id == self.pane_id
                        && popup.selection == geometry.key.selection
                        && popup.menu_owned
                        && editor.context_menu.is_some()
                });
            editor.selection_toolbar_popup = Some(SelectionToolbarPopup {
                pane_id: self.pane_id,
                selection: geometry.key.selection.clone(),
                origin,
                menu_owned,
            });
            if let Some(snapshot) = editor.selection_toolbar_geometry.get_mut(&self.pane_id) {
                if editor
                    .selection_toolbar_animation
                    .as_ref()
                    .is_some_and(|state| state.visible)
                {
                    snapshot.toolbar_bounds = Some(Bounds::new(origin, measured));
                }
            }
        });
        Some(SelectionToolbarPrepaintState {
            toolbar,
            viewport: geometry.viewport,
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(state) = prepaint {
            window.with_content_mask(
                Some(ContentMask {
                    bounds: state.viewport,
                }),
                |window| {
                    state.toolbar.paint(window, cx);
                },
            );
        }
    }
}

pub(crate) struct LineFormatToolbarElement {
    pub(crate) editor: Entity<EditorView>,
    pub(crate) pane_id: PaneId,
}

pub(crate) struct LineFormatToolbarPrepaintState {
    toolbar: AnyElement,
    viewport: Bounds<Pixels>,
    toolbar_hitbox: gpui::Hitbox,
    trigger_hitbox: gpui::Hitbox,
    interaction_bounds: Bounds<Pixels>,
}

impl IntoElement for LineFormatToolbarElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for LineFormatToolbarElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<LineFormatToolbarPrepaintState>;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name(
            format!("line-format-toolbar-{}", self.pane_id).into(),
        ))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.inset.left = px(0.0).into();
        style.inset.top = px(0.0).into();
        style.size = size(relative(1.0).into(), relative(1.0).into());
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (mut toolbar, geometry, margin, gap) = self.editor.update(cx, |editor, cx| {
            editor.line_format_toolbar_bounds = None;
            if editor.focused_pane_id != self.pane_id
                || (editor.cursor.has_selection() && editor.line_format_toolbar_line.is_none())
                || !editor.app_settings.format_toolbar_visible
                || !editor.active_tab_is_markdown()
                || editor.active_tab_is_read_only()
            {
                return None;
            }
            let pointer_line = editor
                .format_trigger_areas
                .iter()
                .find(|(bounds, _)| bounds.contains(&window.mouse_position()))
                .map(|(_, line)| *line);
            let line = editor.line_format_toolbar_line.or(pointer_line)?;
            if editor.line_format_toolbar_line.is_none()
                && pointer_line.is_some()
                && (editor.hovering_format_trigger_line != Some(line)
                    || editor.line_format_toolbar_line != Some(line))
            {
                editor.update_line_format_hover(Some(line), Some(line), cx);
            }
            let geometry = editor
                .line_format_toolbar_geometries
                .iter()
                .find(|geometry| geometry.line == line)
                .cloned()?;
            let margin = px(editor.scaled(6.0));
            let gap = px(0.0);
            let available_width = geometry.viewport.size.width - margin * 2.0;
            if available_width <= px(0.0) {
                return None;
            }
            let opacity = editor.line_format_toolbar_opacity.clamp(0.0, 1.0);
            let toolbar = editor.render_format_toolbar(
                f32::from(available_width),
                opacity,
                true,
                true,
                Some(line),
                cx,
            );
            Some((toolbar, geometry, margin, gap))
        })?;
        let measured = toolbar.layout_as_root(
            size(
                AvailableSpace::Definite(geometry.viewport.size.width - margin * 2.0),
                AvailableSpace::MinContent,
            ),
            window,
            cx,
        );
        let origin = geometry.toolbar_origin(measured, margin, gap)?;
        let toolbar_bounds = Bounds::new(origin, measured);
        let interaction_bounds = toolbar_bounds.union(&geometry.trigger_bounds);
        let toolbar_hitbox =
            window.insert_hitbox(toolbar_bounds, gpui::HitboxBehavior::BlockMouseExceptScroll);
        let trigger_hitbox = window.insert_hitbox(
            geometry.trigger_bounds,
            gpui::HitboxBehavior::BlockMouseExceptScroll,
        );
        window.with_content_mask(
            Some(ContentMask {
                bounds: geometry.viewport,
            }),
            |window| {
                toolbar.prepaint_at(origin, window, cx);
            },
        );
        self.editor.update(cx, |editor, _| {
            editor.line_format_toolbar_bounds = Some(toolbar_bounds);
        });
        Some(LineFormatToolbarPrepaintState {
            toolbar,
            viewport: geometry.viewport,
            toolbar_hitbox,
            trigger_hitbox,
            interaction_bounds,
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(state) = prepaint {
            window.set_cursor_style(gpui::CursorStyle::Arrow, &state.toolbar_hitbox);
            window.set_cursor_style(gpui::CursorStyle::Arrow, &state.trigger_hitbox);
            let interaction_bounds = state.interaction_bounds;
            let editor = self.editor.clone();
            let pane_id = self.pane_id;
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Capture {
                    return;
                }
                let hovered = interaction_bounds.contains(&window.mouse_position());
                editor.update(cx, |this, cx| {
                    this.with_pane(pane_id, |this| {
                        this.set_line_format_toolbar_hovered(hovered, cx);
                    });
                });
            });
            window.with_content_mask(
                Some(ContentMask {
                    bounds: state.viewport,
                }),
                |window| {
                    state.toolbar.paint(window, cx);
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn key() -> SelectionToolbarSnapshotKey {
        SelectionToolbarSnapshotKey {
            selection: SelectionToolbarSelectionKey {
                tab_id: "tab-a".into(),
                primary_range: 4..28,
            },
            cursor_at_end: true,
            scroll_offset: 120.0,
            horizontal_offsets: vec![(3, 18.0)],
        }
    }

    fn geometry(selected: &[Bounds<Pixels>]) -> SelectionToolbarGeometry {
        let viewport = rect(100.0, 50.0, 400.0, 300.0);
        SelectionToolbarGeometry::from_visible_bounds(key(), viewport, viewport, selected).unwrap()
    }

    #[test]
    fn selection_masks_clip_tables_and_panned_code_and_discard_offscreen_quads() {
        let viewport = rect(100.0, 50.0, 400.0, 300.0);
        let quads = [
            rect(110.0, 40.0, 40.0, 30.0),
            rect(180.0, 50.0, 50.0, 20.0),
            rect(110.0, 400.0, 50.0, 20.0),
        ]
        .map(|bounds| gpui::fill(bounds, gpui::black()));
        let blocks = vec![(
            rect(120.0, 100.0, 200.0, 40.0),
            vec![gpui::fill(rect(90.0, 90.0, 300.0, 70.0), gpui::black())],
        )];
        let result =
            SelectionToolbarGeometry::from_quads(key(), viewport, viewport, None, &quads, &blocks)
                .unwrap();
        assert_eq!(result.top_row_anchor, rect(110.0, 50.0, 120.0, 20.0));
        assert_eq!(result.selected_bounds, rect(110.0, 50.0, 210.0, 90.0));
        assert!(
            SelectionToolbarGeometry::from_quads(key(), viewport, viewport, None, &quads[2..], &[])
                .is_none()
        );
    }

    #[test]
    fn ordinary_horizontal_mask_is_not_applied_to_code_block_selections() {
        let viewport = rect(0.0, 0.0, 300.0, 200.0);
        let quads = [gpui::fill(rect(0.0, 50.0, 100.0, 20.0), gpui::black())];
        let blocks = vec![(
            rect(10.0, 80.0, 50.0, 20.0),
            vec![gpui::fill(rect(0.0, 80.0, 100.0, 20.0), gpui::black())],
        )];
        let result = SelectionToolbarGeometry::from_quads(
            key(),
            viewport,
            viewport,
            Some(rect(40.0, 0.0, 200.0, 200.0)),
            &quads,
            &blocks,
        )
        .unwrap();
        assert_eq!(result.top_row_anchor, rect(40.0, 50.0, 60.0, 20.0));
        assert_eq!(result.selected_bounds, rect(10.0, 50.0, 90.0, 50.0));
    }

    #[test]
    fn anchors_first_visible_row_not_emission_order_and_clamps_horizontally() {
        let right = geometry(&[
            rect(400.0, 200.0, 60.0, 20.0),
            rect(450.0, 140.0, 30.0, 20.0),
            rect(420.0, 140.0, 20.0, 20.0),
        ]);
        assert_eq!(right.top_row_anchor, rect(420.0, 140.0, 60.0, 20.0));
        assert_eq!(
            right.toolbar_origin(size(px(160.0), px(32.0)), px(6.0), px(6.0)),
            Some(point(px(334.0), px(102.0)))
        );
        let left = geometry(&[rect(100.0, 140.0, 10.0, 20.0)]);
        assert_eq!(
            left.toolbar_origin(size(px(160.0), px(32.0)), px(6.0), px(6.0))
                .unwrap()
                .x,
            px(106.0)
        );
    }

    #[test]
    fn cursor_endpoint_tracks_selection_direction() {
        let viewport = rect(100.0, 50.0, 400.0, 300.0);
        let selected = [
            rect(160.0, 80.0, 120.0, 20.0),
            rect(120.0, 110.0, 200.0, 20.0),
        ];
        let forward =
            SelectionToolbarGeometry::from_visible_bounds(key(), viewport, viewport, &selected)
                .unwrap();
        assert_eq!(forward.cursor_endpoint_x, px(320.0));

        let mut reverse_key = key();
        reverse_key.cursor_at_end = false;
        let reverse = SelectionToolbarGeometry::from_visible_bounds(
            reverse_key,
            viewport,
            viewport,
            &selected,
        )
        .unwrap();
        assert_eq!(reverse.cursor_endpoint_x, px(160.0));
    }

    #[test]
    fn line_toolbar_aligns_with_content_and_prefers_above() {
        let geometry = LineFormatToolbarGeometry {
            line: 4,
            viewport: rect(100.0, 50.0, 500.0, 300.0),
            content_left: px(180.0),
            trigger_bounds: rect(180.0, 180.0, 18.0, 22.0),
            line_top: px(180.0),
            line_bottom: px(202.0),
        };
        assert_eq!(
            geometry.toolbar_origin(size(px(280.0), px(52.0)), px(6.0), px(4.0),),
            Some(point(px(180.0), px(124.0)))
        );

        let near_top = LineFormatToolbarGeometry {
            line_top: px(60.0),
            line_bottom: px(82.0),
            ..geometry
        };
        assert_eq!(
            near_top.toolbar_origin(size(px(280.0), px(52.0)), px(6.0), px(4.0),),
            Some(point(px(180.0), px(86.0)))
        );
    }

    #[test]
    fn below_fallback_clears_entire_selection_or_hides() {
        let short = geometry(&[
            rect(180.0, 52.0, 60.0, 20.0),
            rect(150.0, 80.0, 100.0, 20.0),
        ]);
        assert_eq!(
            short
                .toolbar_origin(size(px(160.0), px(32.0)), px(6.0), px(6.0))
                .unwrap()
                .y,
            px(106.0)
        );
        let full = geometry(&[rect(180.0, 52.0, 60.0, 285.0)]);
        assert!(
            full.toolbar_origin(size(px(160.0), px(32.0)), px(6.0), px(6.0))
                .is_none()
        );
        assert!(
            short
                .toolbar_origin(size(px(389.0), px(32.0)), px(6.0), px(6.0))
                .is_none()
        );
        assert!(
            short
                .toolbar_origin(size(px(0.0), px(32.0)), px(6.0), px(6.0))
                .is_none()
        );
    }

    #[test]
    fn rejects_stale_tab_range_scroll_pan_and_viewport() {
        let snapshot = geometry(&[rect(180.0, 140.0, 60.0, 20.0)]);
        assert!(snapshot.matches(&key(), snapshot.editor_bounds));
        let mut changed = key();
        changed.selection.tab_id = "tab-b".into();
        assert!(!snapshot.matches(&changed, snapshot.editor_bounds));
        let mut changed = key();
        changed.selection.primary_range = 5..28;
        assert!(!snapshot.matches(&changed, snapshot.editor_bounds));
        let mut changed = key();
        changed.scroll_offset += 0.1;
        assert!(!snapshot.matches(&changed, snapshot.editor_bounds));
        let mut changed = key();
        changed.horizontal_offsets[0].1 += 0.1;
        assert!(!snapshot.matches(&changed, snapshot.editor_bounds));
        assert!(!snapshot.matches(&key(), rect(101.0, 50.0, 400.0, 300.0)));
        assert!(!snapshot.matches(&key(), rect(100.0, 50.0, 399.0, 300.0)));
    }

    #[test]
    fn ancestor_clip_excludes_tab_strip_and_accepts_measured_narrow_bar() {
        let editor_bounds = rect(100.0, 0.0, 180.0, 350.0);
        let viewport = rect(100.0, 50.0, 180.0, 300.0);
        let selections = [gpui::fill(rect(120.0, 48.0, 50.0, 22.0), gpui::black())];
        let result = SelectionToolbarGeometry::from_quads(
            key(),
            editor_bounds,
            viewport,
            None,
            &selections,
            &[],
        )
        .unwrap();
        assert_eq!(result.viewport, viewport);
        assert_eq!(result.top_row_anchor, rect(120.0, 50.0, 50.0, 20.0));
        let origin = result
            .toolbar_origin(size(px(156.0), px(48.0)), px(12.0), px(12.0))
            .unwrap();
        assert_eq!(origin, point(px(112.0), px(82.0)));
        assert!(
            result
                .toolbar_origin(size(px(157.0), px(48.0)), px(12.0), px(12.0))
                .is_none()
        );
    }

    #[test]
    fn dismissal_key_ignores_pan_but_not_tab_or_primary_range() {
        let before = key();
        let mut after = before.clone();
        after.scroll_offset += 200.0;
        after.horizontal_offsets[0].1 += 50.0;
        assert_eq!(before.selection, after.selection);
        after.selection.primary_range = 8..30;
        assert_ne!(before.selection, after.selection);
        after.selection = before.selection.clone();
        after.selection.tab_id = "tab-b".into();
        assert_ne!(before.selection, after.selection);
    }

    #[test]
    fn every_visibility_gate_blocks_display() {
        let ready = SelectionToolbarVisibility {
            enabled: true,
            editable_markdown: true,
            focused: true,
            window_active: true,
            primary_nonempty: true,
            extra_cursors: false,
            selecting: false,
            composing: false,
            selected_handle: false,
            table_drag: false,
            preview_overlay: false,
            modal_overlay: false,
            sidebar_edit_or_focus: false,
            search_focus: false,
            other_drag: false,
            dismissed: false,
        };
        assert!(ready.visible());
        for index in 0..16 {
            let mut state = ready;
            let gates = [
                &mut state.enabled,
                &mut state.editable_markdown,
                &mut state.focused,
                &mut state.window_active,
                &mut state.primary_nonempty,
                &mut state.extra_cursors,
                &mut state.selecting,
                &mut state.composing,
                &mut state.selected_handle,
                &mut state.table_drag,
                &mut state.preview_overlay,
                &mut state.modal_overlay,
                &mut state.sidebar_edit_or_focus,
                &mut state.search_focus,
                &mut state.other_drag,
                &mut state.dismissed,
            ];
            *gates.into_iter().nth(index).unwrap() ^= true;
            assert!(!state.visible(), "gate {index}");
        }
    }
}
