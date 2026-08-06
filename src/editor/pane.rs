//! Split-pane infrastructure: the pane layout tree, per-pane editor state and
//! the state-swap primitives that let the single `EditorView` entity host
//! multiple independent editor panes.
//!
//! Architecture: `EditorView`'s flat document fields (buffer, cursor, scroll,
//! caches, hit areas, ...) always describe the **focused** pane. Every pane
//! owns a `PaneState` mirror of those fields; the focused pane's slot holds a
//! detached placeholder. Rendering or routing events to a non-focused pane
//! temporarily swaps its state into the root fields (`with_pane`), so the
//! entire existing editing pipeline works unchanged. Switching focus moves
//! states permanently (`focus_pane`).
//!
//! IMPORTANT: when adding a per-document field to `EditorView`, it must also be
//! added to the `pane_state!` list below, otherwise its value will leak
//! between panes.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

use gpui::{Bounds, Pixels, Point, ScrollHandle, Task};

use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::markdown::LineDecoration;

use super::*;

pub(crate) type PaneId = usize;

struct TakenTab {
    tab: OpenTab,
    generated_preview_cache: Option<GeneratedPreviewTabCache>,
}

// ─── Pane layout tree ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SplitAxis {
    /// Children are laid out left→right (a `flex_row`).
    Horizontal,
    /// Children are laid out top→bottom (a `flex_col`).
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SplitDirection {
    Up,
    Down,
    Left,
    Right,
}

impl SplitDirection {
    pub(crate) fn axis(self) -> SplitAxis {
        match self {
            SplitDirection::Up | SplitDirection::Down => SplitAxis::Vertical,
            SplitDirection::Left | SplitDirection::Right => SplitAxis::Horizontal,
        }
    }

    /// Whether the new pane is inserted after (right/below) the target pane.
    pub(crate) fn inserts_after(self) -> bool {
        matches!(self, SplitDirection::Down | SplitDirection::Right)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum PaneLayout {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        children: Vec<PaneLayout>,
        /// Relative sizes; always `children.len()` entries summing to
        /// `children.len()`.
        flexes: Vec<f32>,
    },
}

impl PaneLayout {
    // Tree helpers below are part of the in-progress split-pane work; not all
    // call sites have landed yet.
    #[allow(dead_code)]
    pub(crate) fn contains(&self, pane_id: PaneId) -> bool {
        match self {
            PaneLayout::Leaf(id) => *id == pane_id,
            PaneLayout::Split { children, .. } => {
                children.iter().any(|child| child.contains(pane_id))
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn first_leaf(&self) -> PaneId {
        match self {
            PaneLayout::Leaf(id) => *id,
            PaneLayout::Split { children, .. } => children[0].first_leaf(),
        }
    }

    pub(crate) fn leaf_ids(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect_leaf_ids(&mut out);
        out
    }

    fn collect_leaf_ids(&self, out: &mut Vec<PaneId>) {
        match self {
            PaneLayout::Leaf(id) => out.push(*id),
            PaneLayout::Split { children, .. } => {
                for child in children {
                    child.collect_leaf_ids(out);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn leaf_count(&self) -> usize {
        match self {
            PaneLayout::Leaf(_) => 1,
            PaneLayout::Split { children, .. } => children.iter().map(PaneLayout::leaf_count).sum(),
        }
    }

    /// Splits `target`'s leaf, placing `new_pane` on the `direction` side of
    /// it. Returns true if the target leaf was found.
    pub(crate) fn split(
        &mut self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
    ) -> bool {
        match self {
            PaneLayout::Leaf(id) if *id == target => {
                *self = Self::new_split(target, new_pane, direction);
                true
            }
            PaneLayout::Leaf(_) => false,
            PaneLayout::Split {
                axis,
                children,
                flexes,
            } => {
                for index in 0..children.len() {
                    match &mut children[index] {
                        PaneLayout::Leaf(id) if *id == target => {
                            if direction.axis() == *axis {
                                let insert_at = if direction.inserts_after() {
                                    index + 1
                                } else {
                                    index
                                };
                                children.insert(insert_at, PaneLayout::Leaf(new_pane));
                                *flexes = vec![1.0; children.len()];
                            } else {
                                children[index] = Self::new_split(target, new_pane, direction);
                            }
                            return true;
                        }
                        PaneLayout::Leaf(_) => {}
                        child @ PaneLayout::Split { .. } => {
                            if child.split(target, new_pane, direction) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
        }
    }

    fn new_split(existing: PaneId, new_pane: PaneId, direction: SplitDirection) -> PaneLayout {
        let children = if direction.inserts_after() {
            vec![PaneLayout::Leaf(existing), PaneLayout::Leaf(new_pane)]
        } else {
            vec![PaneLayout::Leaf(new_pane), PaneLayout::Leaf(existing)]
        };
        PaneLayout::Split {
            axis: direction.axis(),
            children,
            flexes: vec![1.0; 2],
        }
    }

    /// Removes `target`'s leaf from the tree, collapsing single-child splits.
    /// Returns true when the leaf was found and removed. Removing the root
    /// leaf is not allowed (the tree always keeps at least one pane).
    pub(crate) fn remove(&mut self, target: PaneId) -> bool {
        match self {
            PaneLayout::Leaf(_) => false,
            PaneLayout::Split { .. } => {
                let removed = Self::remove_from_split(self, target);
                if removed {
                    Self::collapse(self);
                }
                removed
            }
        }
    }

    fn remove_from_split(node: &mut PaneLayout, target: PaneId) -> bool {
        let PaneLayout::Split {
            children, flexes, ..
        } = node
        else {
            return false;
        };

        if let Some(index) = children
            .iter()
            .position(|child| matches!(child, PaneLayout::Leaf(id) if *id == target))
        {
            children.remove(index);
            *flexes = vec![1.0; children.len()];
            return true;
        }

        for child in children.iter_mut() {
            if matches!(child, PaneLayout::Split { .. }) && Self::remove_from_split(child, target) {
                Self::collapse(child);
                return true;
            }
        }
        false
    }

    /// Collapses a split node that has a single remaining child (promoting the
    /// child) and merges nested splits that share the parent's axis.
    fn collapse(node: &mut PaneLayout) {
        let PaneLayout::Split {
            axis,
            children,
            flexes,
        } = node
        else {
            return;
        };

        if children.len() == 1 {
            let only = children.pop().expect("split had one child");
            *node = only;
            return;
        }

        // Merge nested splits with the same axis into this node.
        let own_axis = *axis;
        if children
            .iter()
            .any(|child| matches!(child, PaneLayout::Split { axis, .. } if *axis == own_axis))
        {
            let mut merged = Vec::with_capacity(children.len());
            for child in children.drain(..) {
                match child {
                    PaneLayout::Split {
                        axis,
                        children: nested,
                        ..
                    } if axis == own_axis => merged.extend(nested),
                    other => merged.push(other),
                }
            }
            *children = merged;
            *flexes = vec![1.0; children.len()];
        }
    }

    /// Adjusts the flex weights so the divider after `child_index` moves by
    /// `delta_fraction` of the container's main-axis size. Sizes are clamped
    /// so neither adjacent child drops below `min_fraction` of the container.
    pub(crate) fn resize_children(
        flexes: &mut [f32],
        child_index: usize,
        delta_fraction: f32,
        min_fraction: f32,
    ) {
        if child_index + 1 >= flexes.len() {
            return;
        }
        let total = flexes.len() as f32;
        let min_flex = (min_fraction * total).min(1.0);
        let flex_delta = delta_fraction * total;
        let current = flexes[child_index];
        let next = flexes[child_index + 1];
        let applied = flex_delta.max(min_flex - current).min(next - min_flex);
        flexes[child_index] = current + applied;
        flexes[child_index + 1] = next - applied;
    }
}

// ─── Per-pane state ─────────────────────────────────────────────────────────

/// Declares every per-document field mirrored between `EditorView`'s root
/// fields and `PaneState`. Single source of truth for the struct definition,
/// the detached constructor, and the swap routine.
macro_rules! pane_state {
    ($(($field:ident, $ty:ty, $default:expr)),* $(,)?) => {
        pub(crate) struct PaneState {
            $(pub(crate) $field: $ty,)*
        }

        impl PaneState {
            /// A hollow placeholder state used for the focused pane's slot and
            /// as the initial state of freshly created panes.
            pub(crate) fn detached() -> Self {
                Self {
                    $($field: $default,)*
                }
            }
        }

        impl EditorView {
            /// Swaps every per-document root field with `state`.
            pub(crate) fn swap_pane_state(&mut self, state: &mut PaneState) {
                $(std::mem::swap(&mut self.$field, &mut state.$field);)*
            }
        }
    };
}

pane_state! {
    // ── Core document/edit state ──
    (buffer, Buffer, Buffer::from_text("")),
    (cursor, Cursor, Cursor::new()),
    (extra_cursors, Vec<Cursor>, Vec::new()),
    (decorations, Vec<LineDecoration>, Vec::new()),
    (scroll_offset, f32, 0.0),
    (is_selecting, bool, false),
    (vertical_move_goal_x, Option<f32>, None),
    (last_scroll_cursor_visual_x, Option<f32>, None),
    (wrap_boundary_affinity, Option<WrapBoundaryAffinity>, None),
    (line_display_info, LineDisplayCache, Vec::new()),
    (pending_line_display_transfer, Option<PendingLineDisplayTransfer>, None),
    (line_display_trim_update_counter, Cell<usize>, Cell::new(0)),
    (line_geometry_cache_dirty, Cell<bool>, Cell::new(true)),
    (line_geometry_cache, RefCell<LineGeometryCacheState>, RefCell::new(LineGeometryCacheState::default())),
    (indent_guide_step_cache_dirty, Cell<bool>, Cell::new(true)),
    (indent_guide_step_cache, Cell<usize>, Cell::new(4)),
    (layout_dirty, bool, false),
    (pending_scroll_to_cursor, bool, false),
    (pending_center_cursor_in_view, bool, false),
    (pending_center_cursor_in_view_immediately, bool, false),
    (pending_preview_reveal, Option<PendingPreviewReveal>, None),
    (pending_typing_scroll_guard, Option<PendingTypingScrollGuard>, None),
    (pending_click_anchor, Option<ClickScrollAnchor>, None),
    (code_mode_preview_key, Option<String>, None),
    (generated_preview_rerender_task, Option<Task<()>>, None),
    (generated_preview_rerender_pending, bool, false),
    (last_viewport_height, f32, 600.0),
    (last_viewport_width, f32, 800.0),
    // ── Whiteboard canvas interaction ──
    (whiteboard_canvas_bounds, Option<Bounds<Pixels>>, None),
    (whiteboard_drag, Option<WhiteboardDrag>, None),
    (whiteboard_pointer, Option<Point<Pixels>>, None),
    (whiteboard_bind_targets, Vec<String>, Vec::new()),
    (whiteboard_text_editor, Option<whiteboard_input::WhiteboardTextEditor>, None),
    // ── IME ──
    (marked_range, Option<Range<usize>>, None),
    (extra_marked_ranges, Vec<Range<usize>>, Vec::new()),
    (ime_composition_snapshot, Option<EditorSnapshot>, None),
    // ── Hit areas + hover state (rebuilt each paint) ──
    (checkbox_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (hovering_checkbox, bool, false),
    (hovering_checkbox_line, Option<usize>, None),
    (image_click_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (image_preview_zoom_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (generated_preview_click_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (generated_preview_image_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (generated_preview_zoom_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (generated_preview_action_areas, Vec<(gpui::Bounds<Pixels>, usize, GeneratedPreviewAction)>, Vec::new()),
    (preview_overlay_action_areas, Vec<(gpui::Bounds<Pixels>, GeneratedPreviewAction)>, Vec::new()),
    (hovering_image_preview_line, Option<usize>, None),
    (hovering_generated_preview_line, Option<usize>, None),
    (hovering_generated_preview_zoom_line, Option<usize>, None),
    (hovering_generated_preview_action, Option<(usize, GeneratedPreviewAction)>, None),
    (hovering_preview_overlay_action, Option<GeneratedPreviewAction>, None),
    (generated_preview_actions_visible_line, Option<usize>, None),
    (generated_preview_actions_opacity, f32, 0.0),
    (generated_preview_actions_opacity_target, f32, 0.0),
    (generated_preview_actions_opacity_epoch, usize, 0),
    (generated_preview_actions_opacity_task, Option<Task<()>>, None),
    (code_block_hover_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (code_block_copy_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (hovering_code_block_start, Option<usize>, None),
    (hovering_code_block_copy_start, Option<usize>, None),
    (copied_code_block_start, Option<usize>, None),
    (code_block_copy_feedback_epoch, usize, 0),
    (table_click_areas, Vec<(gpui::Bounds<Pixels>, usize, usize, usize)>, Vec::new()),
    (table_col_handle_areas, Vec<(gpui::Bounds<Pixels>, usize, usize)>, Vec::new()),
    (table_row_handle_areas, Vec<(gpui::Bounds<Pixels>, usize, usize)>, Vec::new()),
    (table_drag, Option<TableDragState>, None),
    (hovering_table_start, Option<usize>, None),
    (hovering_handle, Option<(usize, bool, usize)>, None),
    (selected_handle, Option<(usize, bool, usize)>, None),
    (generated_preview_drag_state, Option<GeneratedPreviewDragState>, None),
    // ── Tabs owned by this pane ──
    (current_file_path, Option<PathBuf>, None),
    (tabs, Vec<OpenTab>, Vec::new()),
    (tab_generated_preview_cache, HashMap<String, GeneratedPreviewTabCache>, HashMap::new()),
    (active_tab_id, Option<String>, None),
    (active_tab_index_cache, Cell<Option<usize>>, Cell::new(None)),
    (active_file_kind_cache, Option<FileKind>, None),
    (tab_bar_scroll_handle, ScrollHandle, ScrollHandle::new()),
    (editor_left_aligned, bool, false),
    // ── Layout geometry from the last prepaint ──
    (last_editor_left, f32, 0.0),
    (last_editor_width, f32, 800.0),
    (last_content_left, f32, 0.0),
    (last_text_width, f32, 800.0),
    (content_origin, Point<Pixels>, Point::default()),
    (last_stable_input_bounds, Option<Bounds<Pixels>>, None),
    (pending_input_bounds, Option<Bounds<Pixels>>, None),
    (preview_overlay, Option<PreviewOverlayState>, None),
    (preview_overlay_bounds, Option<Bounds<Pixels>>, None),
    (preview_overlay_image_bounds, Option<Bounds<Pixels>>, None),
    (last_mouse_down_target, Option<(Point<Pixels>, usize)>, None),
    (selection_drag_has_moved, bool, false),
    // ── Folding / gutter ──
    (folded_sections, HashSet<usize>, HashSet::new()),
    (folded_image_previews, HashSet<usize>, HashSet::new()),
    (generated_preview_keys, HashSet<String>, HashSet::new()),
    (gutter_fold_areas, Vec<(gpui::Bounds<Pixels>, usize, bool)>, Vec::new()),
    (image_edit_areas, Vec<(gpui::Bounds<Pixels>, usize)>, Vec::new()),
    (hovering_image_edit_line, Option<usize>, None),
    (hovering_fold_line, Option<usize>, None),
    (hovering_gutter_fold_area, bool, false),
    (gutter_fold_reveal, f32, 0.0),
    (gutter_fold_reveal_target, f32, 0.0),
    (gutter_fold_reveal_epoch, usize, 0),
    (gutter_fold_reveal_task, Option<Task<()>>, None),
    (image_drag_state, Option<ImageDragState>, None),
    (hovered_editor_position, Option<Point<Pixels>>, None),
    // ── Editor scrollbar ──
    (editor_scrollbar_hovered, bool, false),
    (editor_scrollbar_revealed, bool, false),
    (editor_scrollbar_opacity, f32, 0.0),
    (editor_scrollbar_opacity_target, f32, 0.0),
    (editor_scrollbar_opacity_epoch, usize, 0),
    (editor_scrollbar_opacity_task, Option<Task<()>>, None),
    (editor_scrollbar_width, f32, 0.0),
    (editor_scrollbar_width_target, f32, 0.0),
    (editor_scrollbar_width_epoch, usize, 0),
    (editor_scrollbar_width_task, Option<Task<()>>, None),
    (editor_scrollbar_idle_epoch, usize, 0),
    (editor_scrollbar_idle_task, Option<Task<()>>, None),
    (editor_scrollbar_drag, Option<EditorScrollbarDragState>, None),
    // ── Typing/undo grouping ──
    (last_typing_checkpoint_time, Option<Instant>, None),
    (last_typing_checkpoint_offset, Option<usize>, None),
    // ── Buffer-anchored popup menus ──
    (slash_menu_start, Option<usize>, None),
    (slash_filter, String, String::new()),
    (wiki_link_start, Option<usize>, None),
    (wiki_link_query, String, String::new()),
    (wiki_link_recent_results, Vec<WorkspaceFileCandidate>, Vec::new()),
    (wiki_link_search_results, Vec<QuickOpenResult>, Vec::new()),
    (wiki_link_recent_task, Option<Task<()>>, None),
    (wiki_link_search_task, Option<Task<()>>, None),
    // ── Inline diff ──
    (inline_diff_dirty, bool, false),
    (inline_diff_base_dirty, bool, false),
    (inline_diff_epoch, u64, 0),
    (inline_diff_gutter_areas, Vec<(gpui::Bounds<gpui::Pixels>, usize)>, Vec::new()),
    (hovering_inline_diff_gutter_hunk, Option<usize>, None),
    (inline_diff_hunk_areas, Vec<(gpui::Bounds<gpui::Pixels>, usize)>, Vec::new()),
    (inline_diff_revert_areas, Vec<(gpui::Bounds<gpui::Pixels>, usize)>, Vec::new()),
    (hovering_inline_diff_hunk, Option<usize>, None),
    (hovering_inline_diff_revert_hunk, Option<usize>, None),
    (inline_diff_phantom_height_above, HashMap<usize, f32>, HashMap::new()),
    (inline_diff_phantom_height_below, HashMap<usize, f32>, HashMap::new()),
    (phantom_row_hits, Vec<PhantomRowHit>, Vec::new()),
    (phantom_selection, Option<PhantomSelection>, None),
    (phantom_selecting, bool, false),
    // ── In-buffer search ──
    (search_visible, bool, false),
    (search_query, String, String::new()),
    (search_replace, String, String::new()),
    (search_replace_visible, bool, false),
    (search_case_sensitive, bool, false),
    (search_whole_word, bool, false),
    (search_use_regex, bool, false),
    (search_matches, Vec<std::ops::Range<usize>>, Vec::new()),
    (search_current_index, Option<usize>, None),
    (search_query_editor, Option<gpui::Entity<crate::text_input::TextInput>>, None),
    (search_replace_editor, Option<gpui::Entity<crate::text_input::TextInput>>, None),
}

pub(crate) struct Pane {
    pub(crate) id: PaneId,
    /// The pane's document state. For the focused pane this slot holds a
    /// detached placeholder (its real state lives in `EditorView`'s root
    /// fields); while `with_pane` runs it temporarily holds the previous root
    /// state.
    pub(crate) state: PaneState,
}

impl Pane {
    pub(crate) fn new(id: PaneId) -> Self {
        Self {
            id,
            state: PaneState::detached(),
        }
    }
}

/// An in-progress drag on a divider between two children of a split node.
#[derive(Clone, Debug)]
pub(crate) struct PaneResizeDrag {
    /// Child-index path from the layout root to the split node.
    pub(crate) path: Vec<usize>,
    /// The divider sits between `divider_index` and `divider_index + 1`.
    pub(crate) divider_index: usize,
    pub(crate) axis: SplitAxis,
    /// Mouse main-axis coordinate at drag start (px).
    pub(crate) start_mouse: f32,
    /// Flex values of the two adjacent children at drag start.
    pub(crate) start_flexes: (f32, f32),
    /// Estimated main-axis size of the whole split container at drag start.
    pub(crate) container_px: f32,
}

impl PaneLayout {
    /// Returns the split node at `path` (child-index path from the root).
    pub(crate) fn node_at_path_mut(&mut self, path: &[usize]) -> Option<&mut PaneLayout> {
        let mut node = self;
        for &index in path {
            let PaneLayout::Split { children, .. } = node else {
                return None;
            };
            node = children.get_mut(index)?;
        }
        Some(node)
    }
}

// ─── EditorView pane primitives ─────────────────────────────────────────────

impl EditorView {
    pub(crate) fn pane_index(&self, pane_id: PaneId) -> Option<usize> {
        self.panes.iter().position(|pane| pane.id == pane_id)
    }

    /// Ids of all live panes, in layout order.
    pub(crate) fn pane_ids(&self) -> Vec<PaneId> {
        self.panes.iter().map(|pane| pane.id).collect()
    }

    /// The pane whose state currently occupies the root fields: the swapped-in
    /// pane during `with_pane`, otherwise the focused pane.
    pub(crate) fn current_pane_id(&self) -> PaneId {
        self.swapped_pane_id.unwrap_or(self.focused_pane_id)
    }

    /// Whether the active tab in the focused pane is a terminal. During
    /// `with_pane`, the focused pane's live root state is temporarily parked in
    /// the swapped pane's slot, so this must not inspect the swapped-in tab.
    pub(crate) fn focused_pane_active_tab_is_terminal(&self) -> bool {
        let Some(swapped_pane_id) = self.swapped_pane_id else {
            return self.active_tab_is_terminal();
        };
        self.panes
            .iter()
            .find(|pane| pane.id == swapped_pane_id)
            .and_then(|pane| {
                let active_id = pane.state.active_tab_id.as_deref()?;
                pane.state.tabs.iter().find(|tab| tab.id == active_id)
            })
            .is_some_and(|tab| tab.kind.is_terminal())
    }

    /// Runs `f` with `pane_id`'s state swapped into the root fields. For the
    /// focused (or already swapped-in) pane this is a plain call.
    pub(crate) fn with_pane<R>(&mut self, pane_id: PaneId, f: impl FnOnce(&mut Self) -> R) -> R {
        let swapped = self.begin_pane_swap(pane_id);
        let result = f(self);
        if swapped {
            self.end_pane_swap(pane_id);
        }
        result
    }

    /// First half of `with_pane`: swaps `pane_id`'s state into the root fields
    /// (parking the previous root state in the pane's slot). Returns true when
    /// a swap actually happened and `end_pane_swap` must be called. Used
    /// directly by custom elements whose prepaint/paint bodies are separate
    /// functions.
    pub(crate) fn begin_pane_swap(&mut self, pane_id: PaneId) -> bool {
        if pane_id == self.current_pane_id() {
            return false;
        }
        debug_assert!(
            self.swapped_pane_id.is_none(),
            "pane swaps must not nest across different panes"
        );
        let Some(index) = self.pane_index(pane_id) else {
            return false;
        };
        let mut state = std::mem::replace(&mut self.panes[index].state, PaneState::detached());
        self.swap_pane_state(&mut state);
        // The pane's slot temporarily holds the focused pane's live state.
        self.panes[index].state = state;
        self.swapped_pane_id = Some(pane_id);
        true
    }

    /// Second half of `with_pane`: restores the root fields to the focused
    /// pane's state and returns `pane_id`'s state to its slot.
    pub(crate) fn end_pane_swap(&mut self, pane_id: PaneId) {
        debug_assert_eq!(self.swapped_pane_id, Some(pane_id));
        let Some(index) = self.pane_index(pane_id) else {
            self.swapped_pane_id = None;
            return;
        };
        let mut state = std::mem::replace(&mut self.panes[index].state, PaneState::detached());
        self.swap_pane_state(&mut state);
        self.panes[index].state = state;
        self.swapped_pane_id = None;
    }

    /// Runs `f` with `pane_id` swapped in only when that pane still exists.
    /// Animation/debounce tasks use this so pending ticks of a closed pane
    /// don't mutate another pane's state.
    pub(crate) fn with_pane_if_alive<R>(
        &mut self,
        pane_id: PaneId,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if self.pane_index(pane_id).is_some() {
            Some(self.with_pane(pane_id, f))
        } else {
            None
        }
    }

    /// Moves keyboard focus (and the root document state) to `pane_id`.
    pub(crate) fn focus_pane(&mut self, pane_id: PaneId, cx: &mut Context<Self>) {
        if pane_id == self.focused_pane_id {
            return;
        }
        debug_assert!(
            self.swapped_pane_id.is_none(),
            "focus_pane must not run inside with_pane"
        );
        let Some(new_index) = self.pane_index(pane_id) else {
            return;
        };

        // Park the current root state into the old focused pane's slot.
        if let Some(old_index) = self.pane_index(self.focused_pane_id) {
            let mut state =
                std::mem::replace(&mut self.panes[old_index].state, PaneState::detached());
            self.swap_pane_state(&mut state);
            self.panes[old_index].state = state;
        }

        // Load the new pane's state into the root fields.
        let mut state = std::mem::replace(&mut self.panes[new_index].state, PaneState::detached());
        self.swap_pane_state(&mut state);
        self.panes[new_index].state = state;

        self.focused_pane_id = pane_id;

        // Cancel animations that were driving the previous pane's root fields.
        self.scroll_anim_epoch = self.scroll_anim_epoch.wrapping_add(1);

        // Panels/status/outline follow the focused pane.
        self.invalidate_active_file_panel_caches();
        self.outline_headings_cache_dirty.set(true);
        self.mark_inline_diff_dirty(false);
        if let Some(path) = self.current_file_path.clone() {
            self.sync_path_in_sidebar_state(path, cx);
        }
        self.pause_blinking(cx);
        if let Some(terminal_tab_id) = self.active_terminal_tab_id() {
            self.hide_right_panels_for_terminal(cx);
            self.pending_terminal_focus = Some(terminal_tab_id);
            cx.notify();
        } else {
            self.request_editor_focus(cx);
        }
        cx.notify();
    }

    // ── Global (cross-pane) tab helpers ──

    /// The leaf pane at the visual top-left corner of the pane area (its tab
    /// bar carries the macOS traffic-light safe margin when the sidebar is
    /// hidden).
    pub(crate) fn top_left_pane_id(&self) -> PaneId {
        fn walk(layout: &PaneLayout) -> PaneId {
            match layout {
                PaneLayout::Leaf(id) => *id,
                PaneLayout::Split { children, .. } => walk(&children[0]),
            }
        }
        walk(&self.pane_layout)
    }

    /// The leaf pane at the visual top-right corner of the pane area (its tab
    /// bar carries the global utility-panel toggle buttons).
    pub(crate) fn top_right_pane_id(&self) -> PaneId {
        fn walk(layout: &PaneLayout) -> PaneId {
            match layout {
                PaneLayout::Leaf(id) => *id,
                PaneLayout::Split { axis, children, .. } => match axis {
                    SplitAxis::Horizontal => walk(children.last().expect("split has children")),
                    SplitAxis::Vertical => walk(&children[0]),
                },
            }
        }
        walk(&self.pane_layout)
    }

    /// Iterates every open tab across all panes. The focused pane's tabs live
    /// in the root `self.tabs`; unfocused panes' tabs live in their state
    /// slots (the focused slot is empty), so chaining yields each tab once.
    pub(crate) fn all_tabs(&self) -> impl Iterator<Item = &OpenTab> {
        self.tabs
            .iter()
            .chain(self.panes.iter().flat_map(|pane| pane.state.tabs.iter()))
    }

    pub(crate) fn all_tabs_mut(&mut self) -> impl Iterator<Item = &mut OpenTab> {
        self.tabs.iter_mut().chain(
            self.panes
                .iter_mut()
                .flat_map(|pane| pane.state.tabs.iter_mut()),
        )
    }

    /// Finds which pane owns the tab with `tab_id`.
    pub(crate) fn pane_id_for_tab(&self, tab_id: &str) -> Option<PaneId> {
        if self.tabs.iter().any(|tab| tab.id == tab_id) {
            return Some(self.current_pane_id());
        }
        self.panes
            .iter()
            .find(|pane| pane.state.tabs.iter().any(|tab| tab.id == tab_id))
            .map(|pane| pane.id)
    }

    /// Finds which pane owns an editor tab for `path`.
    pub(crate) fn pane_id_for_editor_path(&self, path: &std::path::Path) -> Option<PaneId> {
        let matches_path = |tab: &OpenTab| {
            tab.kind == TabKind::Editor
                && tab.path.as_ref().is_some_and(|tab_path| tab_path == path)
        };
        if self.tabs.iter().any(matches_path) {
            return Some(self.current_pane_id());
        }
        self.panes
            .iter()
            .find(|pane| pane.state.tabs.iter().any(matches_path))
            .map(|pane| pane.id)
    }

    pub(crate) fn pane_tab_count(&self, pane_id: PaneId) -> usize {
        if pane_id == self.current_pane_id() {
            self.tabs.len()
        } else {
            self.pane_index(pane_id)
                .map(|index| self.panes[index].state.tabs.len())
                .unwrap_or(0)
        }
    }

    // ── Tab movement between panes ──

    /// Removes `tab_id` from the pane whose state currently occupies the root
    /// fields, persisting live editor state into the tab snapshot first and
    /// activating a neighbouring tab when the active one is taken.
    fn take_tab_from_current_pane(
        &mut self,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<TakenTab> {
        let index = self.tabs.iter().position(|tab| tab.id == tab_id)?;
        let was_active = self.active_tab_id.as_deref() == Some(tab_id);
        if was_active {
            self.persist_active_tab_state();
            self.stash_active_tab_layout_cache();
            self.stash_active_tab_generated_preview_cache();
        }
        let tab = self.tabs.remove(index);
        let generated_preview_cache = self.tab_generated_preview_cache.remove(tab_id);
        self.active_tab_index_cache.set(None);
        if was_active {
            if self.tabs.is_empty() {
                self.active_tab_id = None;
                self.active_file_kind_cache = None;
                self.current_file_path = None;
            } else {
                let fallback = index.min(self.tabs.len() - 1);
                self.activate_tab_by_index_without_persist(fallback, cx);
            }
        }
        Some(TakenTab {
            tab,
            generated_preview_cache,
        })
    }

    /// Moves a tab into `target` pane (at `insert_index`, or appended),
    /// focuses the target pane and activates the tab. Collapses the source
    /// pane when it becomes empty.
    pub(crate) fn move_tab_to_pane(
        &mut self,
        tab_id: &str,
        target: PaneId,
        insert_index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.pane_id_for_tab(tab_id) else {
            return;
        };
        if source == target || self.pane_index(target).is_none() {
            return;
        }
        let tab_id = tab_id.to_string();
        let Some(taken) =
            self.with_pane(source, |this| this.take_tab_from_current_pane(&tab_id, cx))
        else {
            return;
        };
        self.with_pane(target, |this| {
            let index = insert_index.unwrap_or(this.tabs.len()).min(this.tabs.len());
            this.tabs.insert(index, taken.tab);
            if let Some(cache) = taken.generated_preview_cache {
                this.tab_generated_preview_cache
                    .insert(tab_id.clone(), cache);
            }
            this.ensure_settings_tab_first();
            this.active_tab_index_cache.set(None);
        });
        self.focus_pane(target, cx);
        self.switch_to_tab(&tab_id, cx);
        self.remove_pane_if_empty(source, cx);
        session_store::persist_session(self);
        cx.notify();
    }

    /// Splits `target` pane in `direction`, moving `tab_id` into the newly
    /// created pane and focusing it.
    pub(crate) fn split_pane_with_tab(
        &mut self,
        target: PaneId,
        direction: SplitDirection,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.pane_id_for_tab(tab_id) else {
            return;
        };
        if self.pane_index(target).is_none() {
            return;
        }
        // Dragging a pane's only tab onto its own edge is a no-op.
        if source == target && self.pane_tab_count(source) <= 1 {
            return;
        }
        let tab_id = tab_id.to_string();
        let Some(taken) =
            self.with_pane(source, |this| this.take_tab_from_current_pane(&tab_id, cx))
        else {
            return;
        };

        let new_id = self.next_pane_id;
        self.next_pane_id += 1;
        let mut pane = Pane::new(new_id);
        pane.state.editor_scrollbar_opacity = self.styles.editor.editor_scrollbar_idle_opacity;
        pane.state.editor_scrollbar_opacity_target = pane.state.editor_scrollbar_opacity;
        pane.state.editor_scrollbar_width = self.styles.editor.editor_scrollbar_width_px;
        pane.state.editor_scrollbar_width_target = pane.state.editor_scrollbar_width;
        pane.state.tabs.push(taken.tab);
        if let Some(cache) = taken.generated_preview_cache {
            pane.state
                .tab_generated_preview_cache
                .insert(tab_id.clone(), cache);
        }
        self.panes.push(pane);
        if !self.pane_layout.split(target, new_id, direction) {
            // Should not happen; restore the tab into the source pane.
            let mut pane = self.panes.pop().expect("pane was just pushed");
            let tab = pane
                .state
                .tabs
                .into_iter()
                .next()
                .expect("tab was just pushed");
            let generated_preview_cache = pane.state.tab_generated_preview_cache.remove(&tab_id);
            self.with_pane(source, |this| {
                this.tabs.push(tab);
                if let Some(cache) = generated_preview_cache {
                    this.tab_generated_preview_cache
                        .insert(tab_id.clone(), cache);
                }
                this.active_tab_index_cache.set(None);
            });
            return;
        }

        self.focus_pane(new_id, cx);
        // focus_pane swapped the (single-tab, no-active) state in; activate it.
        self.activate_tab_by_index(0, cx);
        self.remove_pane_if_empty(source, cx);
        session_store::persist_session(self);
        cx.notify();
    }

    /// Removes `pane_id` from the layout when it holds no tabs (and is not the
    /// last remaining pane), focusing a neighbour if it was focused.
    pub(crate) fn remove_pane_if_empty(&mut self, pane_id: PaneId, cx: &mut Context<Self>) {
        if self.panes.len() <= 1 || self.pane_index(pane_id).is_none() {
            return;
        }
        if self.pane_tab_count(pane_id) > 0 {
            return;
        }
        if self.focused_pane_id == pane_id {
            let fallback = self
                .pane_layout
                .leaf_ids()
                .into_iter()
                .find(|id| *id != pane_id);
            if let Some(fallback) = fallback {
                self.focus_pane(fallback, cx);
            }
        }
        if self.pane_layout.remove(pane_id) {
            if let Some(index) = self.pane_index(pane_id) {
                self.panes.remove(index);
            }
            self.pane_bounds.remove(&pane_id);
            cx.notify();
        }
    }

    // ── Keyboard actions ──

    fn split_focused_pane(&mut self, direction: SplitDirection, cx: &mut Context<Self>) {
        let Some(active_tab_id) = self.active_tab_id.clone() else {
            return;
        };
        let focused = self.focused_pane_id;
        self.split_pane_with_tab(focused, direction, &active_tab_id, cx);
    }

    pub(crate) fn on_split_pane_right(
        &mut self,
        _: &crate::actions::SplitPaneRight,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.split_focused_pane(SplitDirection::Right, cx);
    }

    pub(crate) fn on_split_pane_left(
        &mut self,
        _: &crate::actions::SplitPaneLeft,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.split_focused_pane(SplitDirection::Left, cx);
    }

    pub(crate) fn on_split_pane_up(
        &mut self,
        _: &crate::actions::SplitPaneUp,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.split_focused_pane(SplitDirection::Up, cx);
    }

    pub(crate) fn on_split_pane_down(
        &mut self,
        _: &crate::actions::SplitPaneDown,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.split_focused_pane(SplitDirection::Down, cx);
    }

    pub(crate) fn on_focus_next_pane(
        &mut self,
        _: &crate::actions::FocusNextPane,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let leaves = self.pane_layout.leaf_ids();
        if leaves.len() < 2 {
            return;
        }
        let current = leaves
            .iter()
            .position(|id| *id == self.focused_pane_id)
            .unwrap_or(0);
        let next = leaves[(current + 1) % leaves.len()];
        self.focus_pane(next, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{PaneLayout, SplitAxis, SplitDirection};

    #[test]
    fn split_leaf_creates_axis() {
        let mut layout = PaneLayout::Leaf(0);
        assert!(layout.split(0, 1, SplitDirection::Right));
        assert_eq!(
            layout,
            PaneLayout::Split {
                axis: SplitAxis::Horizontal,
                children: vec![PaneLayout::Leaf(0), PaneLayout::Leaf(1)],
                flexes: vec![1.0, 1.0],
            }
        );
    }

    #[test]
    fn split_before_inserts_first() {
        let mut layout = PaneLayout::Leaf(0);
        assert!(layout.split(0, 1, SplitDirection::Up));
        assert_eq!(
            layout,
            PaneLayout::Split {
                axis: SplitAxis::Vertical,
                children: vec![PaneLayout::Leaf(1), PaneLayout::Leaf(0)],
                flexes: vec![1.0, 1.0],
            }
        );
    }

    #[test]
    fn split_same_axis_inserts_sibling() {
        let mut layout = PaneLayout::Leaf(0);
        layout.split(0, 1, SplitDirection::Right);
        assert!(layout.split(0, 2, SplitDirection::Right));
        assert_eq!(
            layout,
            PaneLayout::Split {
                axis: SplitAxis::Horizontal,
                children: vec![
                    PaneLayout::Leaf(0),
                    PaneLayout::Leaf(2),
                    PaneLayout::Leaf(1)
                ],
                flexes: vec![1.0, 1.0, 1.0],
            }
        );
    }

    #[test]
    fn split_cross_axis_nests() {
        let mut layout = PaneLayout::Leaf(0);
        layout.split(0, 1, SplitDirection::Right);
        assert!(layout.split(1, 2, SplitDirection::Down));
        assert_eq!(
            layout,
            PaneLayout::Split {
                axis: SplitAxis::Horizontal,
                children: vec![
                    PaneLayout::Leaf(0),
                    PaneLayout::Split {
                        axis: SplitAxis::Vertical,
                        children: vec![PaneLayout::Leaf(1), PaneLayout::Leaf(2)],
                        flexes: vec![1.0, 1.0],
                    }
                ],
                flexes: vec![1.0, 1.0],
            }
        );
    }

    #[test]
    fn remove_collapses_to_leaf() {
        let mut layout = PaneLayout::Leaf(0);
        layout.split(0, 1, SplitDirection::Right);
        assert!(layout.remove(1));
        assert_eq!(layout, PaneLayout::Leaf(0));
    }

    #[test]
    fn remove_nested_promotes_child() {
        let mut layout = PaneLayout::Leaf(0);
        layout.split(0, 1, SplitDirection::Right);
        layout.split(1, 2, SplitDirection::Down);
        // Removing 1 collapses the vertical split; 2 replaces it.
        assert!(layout.remove(1));
        assert_eq!(
            layout,
            PaneLayout::Split {
                axis: SplitAxis::Horizontal,
                children: vec![PaneLayout::Leaf(0), PaneLayout::Leaf(2)],
                flexes: vec![1.0, 1.0],
            }
        );
    }

    #[test]
    fn remove_merges_same_axis_after_collapse() {
        // H[0, V[1, H[2, 3]]] — removing 1 promotes H[2, 3] into the root H.
        let mut layout = PaneLayout::Leaf(0);
        layout.split(0, 1, SplitDirection::Right);
        layout.split(1, 2, SplitDirection::Down);
        layout.split(2, 3, SplitDirection::Right);
        assert!(layout.remove(1));
        assert_eq!(
            layout,
            PaneLayout::Split {
                axis: SplitAxis::Horizontal,
                children: vec![
                    PaneLayout::Leaf(0),
                    PaneLayout::Leaf(2),
                    PaneLayout::Leaf(3)
                ],
                flexes: vec![1.0, 1.0, 1.0],
            }
        );
    }

    #[test]
    fn remove_root_leaf_is_rejected() {
        let mut layout = PaneLayout::Leaf(0);
        assert!(!layout.remove(0));
        assert_eq!(layout, PaneLayout::Leaf(0));
    }

    #[test]
    fn leaf_ids_in_visual_order() {
        let mut layout = PaneLayout::Leaf(0);
        layout.split(0, 1, SplitDirection::Right);
        layout.split(0, 2, SplitDirection::Down);
        assert_eq!(layout.leaf_ids(), vec![0, 2, 1]);
        assert_eq!(layout.first_leaf(), 0);
        assert_eq!(layout.leaf_count(), 3);
        assert!(layout.contains(2));
        assert!(!layout.contains(9));
    }

    #[test]
    fn resize_children_clamps_to_min() {
        let mut flexes = vec![1.0, 1.0];
        PaneLayout::resize_children(&mut flexes, 0, 0.25, 0.1);
        assert!((flexes[0] - 1.5).abs() < 1e-6);
        assert!((flexes[1] - 0.5).abs() < 1e-6);
        // Push far past the min: the neighbour is clamped at 0.1 * total.
        PaneLayout::resize_children(&mut flexes, 0, 10.0, 0.1);
        assert!((flexes[1] - 0.2).abs() < 1e-6);
        assert!((flexes[0] - 1.8).abs() < 1e-6);
        let total: f32 = flexes.iter().sum();
        assert!((total - 2.0).abs() < 1e-6);
    }
}
