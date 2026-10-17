use std::{collections::HashMap, ops::Range};

use gpui::{
    App, Bounds, Context, EntityInputHandler, FocusHandle, Focusable, Pixels, Point,
    UTF16Selection, Window, WrappedLine, point, px, size,
};

use crate::markdown::{BlockType, TableLineInfo};

use super::input::{InlineLineDisplayInfo, ime_debug_enabled, ime_debug_log};
use super::*;

fn input_bounds_for_wrapped_position(
    wrapped: &WrappedLine,
    position: Point<Pixels>,
    font_size: f32,
    line_height: f32,
    content_x: f32,
    text_top_y: f32,
) -> Bounds<Pixels> {
    let row = wrapped.row_for_y(position.y, px(line_height));
    Bounds::new(
        point(px(content_x) + position.x, px(text_top_y) + position.y),
        size(
            px(font_size * 0.6),
            wrapped.row_height(row, px(line_height)),
        ),
    )
}

#[cfg(test)]
#[test]
fn ime_bounds_use_actual_row_height_after_tall_wrapped_rows() {
    let wrapped = super::input_wrap::test_wrapped_line(&[24.0, 60.0, 24.0]);
    let tall = input_bounds_for_wrapped_position(
        &wrapped,
        point(px(10.0), px(24.0)),
        20.0,
        24.0,
        100.0,
        200.0,
    );
    assert_eq!(tall.origin, point(px(110.0), px(224.0)));
    assert_eq!(tall.size, size(px(12.0), px(60.0)));
    let following = input_bounds_for_wrapped_position(
        &wrapped,
        point(px(10.0), px(84.0)),
        20.0,
        24.0,
        100.0,
        200.0,
    );
    assert_eq!(following.origin.y, px(284.0));
    assert_eq!(following.size.height, px(24.0));
    let uniform = super::input_wrap::test_wrapped_line(&[]);
    assert_eq!(
        input_bounds_for_wrapped_position(
            &uniform,
            point(px(0.0), px(48.0)),
            20.0,
            24.0,
            100.0,
            200.0,
        )
        .size
        .height,
        px(24.0)
    );
}

fn uses_table_ime_geometry(active_tab_is_diff: bool, table_info: &TableLineInfo) -> bool {
    !active_tab_is_diff && !table_info.is_separator && !table_info.is_front_matter
}

#[cfg(test)]
#[test]
fn front_matter_uses_inline_ime_geometry() {
    let front_matter = crate::markdown::parse_decorations("---\ntitle: Moeka\n---");
    let front_matter_info = front_matter[1].table_info.as_ref().unwrap();
    let table = crate::markdown::parse_decorations("| Key | Value |\n| --- | --- |\n| A | B |");
    let table_info = table[0].table_info.as_ref().unwrap();

    assert!(!uses_table_ime_geometry(false, front_matter_info));
    assert!(uses_table_ime_geometry(false, table_info));
    assert!(!uses_table_ime_geometry(true, table_info));
}

fn stable_input_bounds_are_usable(
    layout_dirty: bool,
    group_geometry_dirty: bool,
    anchor_rows: Option<usize>,
) -> bool {
    layout_dirty && !group_geometry_dirty && anchor_rows != Some(0)
}

/// Native composition may replace neither folded source nor the displayed summary.
/// A range that merely starts outside the block can still overwrite it.
fn ime_lines_touch_protected_front_matter(
    folded: Option<Range<usize>>,
    summary: Option<usize>,
    first: usize,
    last: usize,
) -> bool {
    folded.is_some_and(|folded| first < folded.end && last >= folded.start)
        || summary.is_some_and(|line| (first..=last).contains(&line))
}

#[cfg(test)]
#[test]
fn front_matter_ime_protects_folded_source_and_summary_for_overlapping_ranges() {
    let folded = Some(1..5);
    assert!(ime_lines_touch_protected_front_matter(
        folded.clone(),
        Some(1),
        0,
        2
    ));
    assert!(ime_lines_touch_protected_front_matter(
        folded.clone(),
        None,
        4,
        6
    ));
    assert!(!ime_lines_touch_protected_front_matter(
        folded.clone(),
        None,
        0,
        0
    ));
    assert!(!ime_lines_touch_protected_front_matter(
        folded.clone(),
        None,
        5,
        6
    ));
    // Empty metadata falls back to a summary on the opener.
    assert!(ime_lines_touch_protected_front_matter(None, Some(0), 0, 0));
}

#[cfg(test)]
#[test]
fn front_matter_ime_rejects_hidden_caches_and_stable_anchors_before_next_prepaint() {
    assert!(!cached_input_geometry_is_visible(0, 0.0, false));
    assert!(!cached_input_geometry_is_visible(0, 24.0, false));
    assert!(!cached_input_geometry_is_visible(1, 24.0, true));
    assert!(cached_input_geometry_is_visible(1, 24.0, false));
    // A stable anchor from before a group expansion must not bypass fresh inline shaping.
    for layout_dirty in [false, true] {
        assert!(!stable_input_bounds_are_usable(layout_dirty, true, Some(1)));
        assert!(!stable_input_bounds_are_usable(
            layout_dirty,
            false,
            Some(0)
        ));
    }
    let wrapped = super::input_wrap::test_wrapped_line(&[24.0]);
    let bounds = input_bounds_for_wrapped_position(
        &wrapped,
        point(px(10.0), px(0.0)),
        20.0,
        24.0,
        100.0,
        200.0,
    );
    assert_eq!(bounds.size.height, px(24.0));
    assert!(bounds.size.height > px(0.0));
}

#[cfg(test)]
#[test]
fn front_matter_ime_top_accounts_for_header_and_list_visibility_before_prepaint() {
    // A previously hidden header now contributes a physical text row.
    assert_eq!(
        input_top_with_current_front_matter_heights(200.0, [(0.0, 24.0)]),
        224.0
    );
    // Default unmeasured physical rows must not count hidden list items.
    assert_eq!(
        input_top_with_current_front_matter_heights(
            200.0,
            [(24.0, 24.0), (24.0, 0.0), (24.0, 0.0)]
        ),
        152.0
    );
    // Long-key fallback wraps source visibly in both focused and collapsed shapes.
    assert_eq!(
        input_top_with_current_front_matter_heights(200.0, [(24.0, 72.0)]),
        248.0
    );
}

fn cached_input_geometry_is_visible(
    visual_rows: usize,
    line_height: f32,
    group_geometry_dirty: bool,
) -> bool {
    !group_geometry_dirty && visual_rows > 0 && line_height > 0.0
}

fn input_top_with_current_front_matter_heights(
    top: f32,
    heights: impl IntoIterator<Item = (f32, f32)>,
) -> f32 {
    top + heights
        .into_iter()
        .map(|(cached, current)| current - cached)
        .sum::<f32>()
}

/// Survives local unmarking and unfolding: neither proves that the native
/// input method has stopped sending callbacks for the old composition.
#[derive(Debug, Default)]
pub(crate) struct FoldImeCancellation {
    blocked: bool,
    epoch: usize,
    scheduled: bool,
    resume_after: Option<f64>,
}

impl FoldImeCancellation {
    pub(super) fn arm(&mut self) {
        self.blocked = true;
        self.epoch = self.epoch.wrapping_add(1);
        self.scheduled = false;
        self.resume_after = None;
    }

    fn accepts(&mut self, key_event_timestamp: Option<f64>, terminal_callback: bool) -> bool {
        if !self.blocked {
            return true;
        }
        if self
            .resume_after
            .zip(key_event_timestamp)
            .is_some_and(|(after, event)| event > after)
        {
            self.blocked = false;
            return true;
        }
        // Platforms without a native cancellation bridge drain the old
        // transaction through its commit/unmark callback before accepting input.
        #[cfg(not(target_os = "macos"))]
        if terminal_callback {
            self.blocked = false;
        }
        #[cfg(target_os = "macos")]
        let _ = terminal_callback;
        false
    }
}

#[cfg(target_os = "macos")]
fn native_key_event_timestamp(window: &Window) -> Option<f64> {
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return None;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    unsafe {
        let app: *mut objc::runtime::Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut objc::runtime::Object = msg_send![app, currentEvent];
        if event.is_null() {
            return None;
        }
        let view = appkit.ns_view.as_ptr() as *mut objc::runtime::Object;
        let editor_window: *mut objc::runtime::Object = msg_send![view, window];
        let event_window: *mut objc::runtime::Object = msg_send![event, window];
        if editor_window.is_null() || event_window != editor_window {
            return None;
        }
        let kind: usize = msg_send![event, type];
        // NSEventTypeKeyDown. Mouse events and callbacks with no fresh key must
        // not lift the barrier, even if local selection/fold state has changed.
        (kind == 10).then(|| {
            let timestamp: f64 = msg_send![event, timestamp];
            timestamp
        })
    }
}

#[cfg(not(target_os = "macos"))]
fn native_key_event_timestamp(_window: &Window) -> Option<f64> {
    None
}

#[cfg(target_os = "macos")]
struct NativeImeContext(*mut objc::runtime::Object);

#[cfg(target_os = "macos")]
impl NativeImeContext {
    fn capture() -> Self {
        use objc::{class, msg_send, sel, sel_impl};
        unsafe {
            let mut input: *mut objc::runtime::Object =
                msg_send![class!(NSTextInputContext), currentInputContext];
            // Mouse-driven folding need not be inside handleEvent:, where
            // currentInputContext is guaranteed. GPUI's native view remains
            // the key window's first responder between keyboard events.
            if input.is_null() {
                let app: *mut objc::runtime::Object =
                    msg_send![class!(NSApplication), sharedApplication];
                let window: *mut objc::runtime::Object = msg_send![app, keyWindow];
                let responder: *mut objc::runtime::Object = msg_send![window, firstResponder];
                if !responder.is_null() {
                    let responds: objc::runtime::BOOL =
                        msg_send![responder, respondsToSelector: sel!(inputContext)];
                    if responds != objc::runtime::NO {
                        input = msg_send![responder, inputContext];
                    }
                }
            }
            if !input.is_null() {
                let _: *mut objc::runtime::Object = msg_send![input, retain];
            }
            Self(input)
        }
    }

    fn discard_marked_text(&self) -> f64 {
        use objc::{class, msg_send, sel, sel_impl};
        unsafe {
            if !self.0.is_null() {
                let () = msg_send![self.0, discardMarkedText];
            }
            let process: *mut objc::runtime::Object = msg_send![class!(NSProcessInfo), processInfo];
            let uptime: f64 = msg_send![process, systemUptime];
            uptime
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for NativeImeContext {
    fn drop(&mut self) {
        use objc::{msg_send, sel, sel_impl};
        if !self.0.is_null() {
            unsafe {
                let () = msg_send![self.0, release];
            }
        }
    }
}

#[cfg(test)]
mod fold_ime_tests {
    use super::FoldImeCancellation;

    #[test]
    fn front_matter_fold_ime_cancellation_blocks_range_less_updates_and_commits() {
        for terminal in [false, true] {
            let source = "---\ntitle: 中\n---\nBody";
            let mut buffer = crate::buffer::Buffer::from_text(source);
            // Collapse has locally unmarked and moved the caret to the opener.
            let mut cursor = crate::cursor::Cursor {
                anchor: 3,
                offset: 3,
            };
            let mut cancellation = FoldImeCancellation::default();
            cancellation.arm();
            let accepted = cancellation.accepts(None, terminal);
            if accepted {
                cursor.insert(&mut buffer, "中文");
            }
            assert!(!accepted);
            assert_eq!(
                buffer.text(),
                source,
                "late callback must not produce ---中文"
            );
            assert_eq!((cursor.anchor, cursor.offset), (3, 3));
        }
        let mut cancellation = FoldImeCancellation::default();
        cancellation.arm();
        // Local unmarking, caret movement, unfolding and reparsing cannot
        // identify a fresh input transaction and do not remove the tombstone.
        for _ in 0..3 {
            assert!(!cancellation.accepts(None, false));
        }
    }

    #[test]
    fn front_matter_fold_ime_cancellation_resumes_only_after_native_discard_and_fresh_key() {
        let mut cancellation = FoldImeCancellation::default();
        cancellation.arm();
        // A key alone is insufficient while native cancellation is pending.
        assert!(!cancellation.accepts(Some(100.0), false));
        cancellation.resume_after = Some(100.0);
        for old_event in [None, Some(99.0), Some(100.0)] {
            assert!(!cancellation.accepts(old_event, false));
        }
        assert!(cancellation.accepts(Some(101.0), false));
        // Later callbacks in the new composition need no replacement range
        // or native event identity, just like the ordinary input path.
        assert!(cancellation.accepts(None, false));
        assert!(cancellation.accepts(None, true));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn front_matter_fold_ime_cancellation_survives_unmark_then_delayed_commit() {
        let mut cancellation = FoldImeCancellation::default();
        cancellation.arm();
        cancellation.resume_after = Some(100.0);
        assert!(!cancellation.accepts(None, true)); // unmarkText
        assert!(!cancellation.accepts(None, true)); // late insertText
        assert!(!cancellation.accepts(None, false)); // late setMarkedText
        assert!(cancellation.accepts(Some(101.0), true)); // first new typing, not swallowed
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn front_matter_fold_ime_cancellation_drains_terminal_callback_without_native_bridge() {
        let mut cancellation = FoldImeCancellation::default();
        cancellation.arm();
        assert!(!cancellation.accepts(None, false));
        assert!(!cancellation.accepts(None, true));
        assert!(cancellation.accepts(None, false));
    }

    #[test]
    fn front_matter_fold_ime_cancellation_is_pane_local_and_rearming_invalidates_completion() {
        let mut first = super::PaneState::detached();
        let mut second = super::PaneState::detached();
        first.fold_ime_cancellation.arm();
        let epoch = first.fold_ime_cancellation.epoch;
        assert!(second.fold_ime_cancellation.accepts(None, false));
        assert!(!first.fold_ime_cancellation.accepts(None, false));
        first.fold_ime_cancellation.arm();
        assert_ne!(first.fold_ime_cancellation.epoch, epoch);
        assert!(first.fold_ime_cancellation.resume_after.is_none());
        std::mem::swap(&mut first, &mut second);
        assert!(first.fold_ime_cancellation.accepts(None, false));
        assert!(!second.fold_ime_cancellation.accepts(None, false));
    }
}

impl EditorView {
    fn materialize_native_table_boundary(
        &mut self,
        range_utf16: &mut Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.source_mode {
            return true;
        }
        let Some(boundary) = self.table_boundary_caret else {
            return true;
        };
        // Native offsets describe the old buffer. Resolve them before inserting
        // the outside line, including the ambiguous pipe-less final-cell end.
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range));
        let marked = self.marked_range.clone();
        let insertion =
            super::input_table::table_boundary_offsets(&self.buffer, &self.decorations, boundary);
        if !self.insert_line_at_table_boundary(cx) {
            return false;
        }
        *range_utf16 = range.map(|range| {
            self.range_to_utf16(
                &super::table_cell_edit::native_range_after_boundary_insertion(range, insertion),
            )
        });
        self.marked_range = marked.map(|range| {
            super::table_cell_edit::native_range_after_boundary_insertion(range, insertion)
        });
        true
    }

    pub(super) fn schedule_fold_ime_cancellation(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            let state = &mut self.fold_ime_cancellation;
            if !state.blocked
                || state.scheduled
                || state.resume_after.is_some()
                || self.swapped_pane_id.is_some()
            {
                return;
            }
            state.scheduled = true;
            let epoch = state.epoch;
            let pane_id = self.focused_pane_id;
            let editor = cx.entity().clone();
            // Retain the originating context; a focus/window change before the
            // deferred effect must not discard another client's composition.
            let native_context = NativeImeContext::capture();
            cx.defer(move |cx| {
                let ready = {
                    let this = editor.read(cx);
                    this.focused_pane_id == pane_id
                        && this.fold_ime_cancellation.epoch == epoch
                        && this.fold_ime_cancellation.blocked
                };
                // discardMarkedText can synchronously reenter the input handler.
                // Do not call it while EditorView is mutably borrowed.
                let resume_after = ready.then(|| native_context.discard_marked_text());
                editor.update(cx, |this, _| {
                    this.with_pane_if_alive(pane_id, |this| {
                        let state = &mut this.fold_ime_cancellation;
                        if state.epoch == epoch {
                            state.scheduled = false;
                            state.resume_after = resume_after;
                        }
                    });
                });
            });
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    fn accepts_fold_ime_callback(
        &mut self,
        terminal_callback: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.schedule_fold_ime_cancellation(cx);
        self.fold_ime_cancellation
            .accepts(native_key_event_timestamp(window), terminal_callback)
    }

    fn ime_range_touches_folded_front_matter(&self, range_utf16: &Range<usize>) -> bool {
        if self.source_mode {
            return false;
        }
        let range = self.range_from_utf16(range_utf16);
        ime_lines_touch_protected_front_matter(
            self.collapsed_front_matter_range(),
            self.front_matter_summary_line(),
            self.buffer.offset_to_line_col(range.start).0,
            self.buffer.offset_to_line_col(range.end).0,
        )
    }

    fn current_front_matter_input_top(
        &self,
        window: &mut Window,
        line: usize,
        bounds: Bounds<Pixels>,
    ) -> f32 {
        let top = self.text_y_offset_for_line(line);
        if !self.front_matter_group_geometry_dirty {
            return top;
        }
        let end = self
            .decorations
            .first()
            .and_then(|d| d.table_info.as_ref())
            .filter(|table| table.is_front_matter)
            .map_or(0, |table| table.table_end_line.min(line));
        let group_plans = self.front_matter_group_plan_for_width(f32::from(bounds.size.width));
        input_top_with_current_front_matter_heights(
            top,
            (0..end).map(|row| {
                let old = self.visual_height_for_line(row);
                let height = self
                    .inline_line_display_info_with_group_plan(window, row, bounds, &group_plans)
                    .map_or(0.0, |info| {
                        let padding = self.block_padding_for_line(row);
                        f32::from(info.wrapped_line.size(px(info.line_height)).height)
                            + padding.0
                            + padding.1
                    });
                (old, height)
            }),
        )
    }

    fn input_bounds_for_display_line(
        &self,
        anchor_offset: usize,
        line: usize,
        col: usize,
        text_top_y: f32,
        display_text: &str,
        buffer_to_display: &[usize],
        wrapped_line: &WrappedLine,
        font_size: f32,
        line_height: f32,
        indent_px: f32,
        wrap_boundary_upstream_xs: &HashMap<usize, f32>,
        content_x: f32,
    ) -> Bounds<Pixels> {
        let disp_col = helpers::buffer_to_display_offset(buffer_to_display, col);
        let affinity = self
            .wrap_boundary_affinity
            .and_then(|current| (current.offset == anchor_offset).then_some(current.kind));
        let pos = Self::visual_position_for_wrapped_display(
            wrapped_line,
            display_text,
            disp_col,
            line_height,
            indent_px,
            wrap_boundary_upstream_xs,
            affinity,
        )
        .unwrap_or_default();
        let bounds = input_bounds_for_wrapped_position(
            wrapped_line,
            pos,
            font_size,
            line_height,
            content_x,
            text_top_y,
        );
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "bounds_for_range normal line={} col={} anchor_utf8={} -> x={} y={} h={}",
                line,
                col,
                anchor_offset,
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.height)
            ));
        }
        bounds
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if !self.active_tab_supports_text_buffer() {
            actual_range.take();
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        let clamped_end = range.end.min(self.buffer.len());
        let clamped_start = range.start.min(clamped_end);
        let text = &self.buffer.text()[clamped_start..clamped_end];
        actual_range.replace(self.range_to_utf16(&(clamped_start..clamped_end)));
        Some(text.to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if !self.active_tab_supports_text_buffer() {
            return None;
        }
        self.reconcile_front_matter_fold_cursor();
        self.schedule_fold_ime_cancellation(cx);
        self.sync_front_matter_group_focus();
        if self.selected_handle.is_some() {
            ime_debug_log("selected_text_range -> None (table handle selected)");
            return None;
        }
        let selection = UTF16Selection {
            range: self.range_to_utf16(&self.cursor.selection_range()),
            reversed: self.cursor.offset < self.cursor.anchor,
        };
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "selected_text_range -> {:?}, cursor=({},{}) marked={:?}",
                selection.range, self.cursor.anchor, self.cursor.offset, self.marked_range
            ));
        }
        Some(selection)
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if !self.active_tab_supports_text_buffer() {
            return None;
        }
        if self.selected_handle.is_some() {
            ime_debug_log("marked_text_range -> None (table handle selected)");
            return None;
        }
        let marked = self
            .marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range));
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "marked_text_range -> {:?} (utf8={:?})",
                marked, self.marked_range
            ));
        }
        marked
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab_supports_text_buffer() {
            return;
        }
        if !self.accepts_fold_ime_callback(true, window, cx) {
            return;
        }
        self.clear_marked_ranges();
        self.finish_ime_composition();
        if self.table_source.is_some() {
            self.pause_blinking(cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        mut range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Range-less native callbacks must be checked before begin_content_edit
        // can unfold the document or checkpoint/typing can clear IME state.
        if !self.accepts_fold_ime_callback(true, window, cx) {
            return;
        }
        if range_utf16
            .as_ref()
            .is_some_and(|range| self.ime_range_touches_folded_front_matter(range))
        {
            return;
        }
        let replaces_text = range_utf16.as_ref().is_some_and(|range| !range.is_empty())
            || self
                .marked_range
                .as_ref()
                .is_some_and(|range| !range.is_empty());
        if !new_text.is_empty() || replaces_text {
            if !self.begin_content_edit(cx) {
                return;
            }
            if !self.materialize_native_table_boundary(&mut range_utf16, cx) {
                return;
            }
        } else if self.active_tab_is_read_only() {
            return;
        }
        self.last_scroll_cursor_visual_x = None;
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        // Typing into the buffer dismisses any read-only phantom-row selection.
        self.clear_phantom_selection();
        if self.source_mode
            && self.active_file_kind().is_some_and(FileKind::is_markdown)
            && range_utf16.is_none()
            && self.marked_range.is_none()
            && self.extra_marked_ranges.is_empty()
            && self.ime_composition_snapshot.is_none()
            && !new_text.is_empty()
        {
            // Native replacement ranges and IME composition stay literal. Only
            // direct typing uses the same source planner as editing actions,
            // including every caret and exact (possibly reversed) selections.
            self.capture_typing_scroll_guard();
            self.pending_code_fence = None;
            self.checkpoint_for_typing(new_text);
            self.apply_source_action(source_edit::SourceAction::Insert(new_text));
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if !self.source_mode
            && !self.cursor.has_selection()
            && self.marked_range.is_none()
            && range_utf16.is_none()
            && !self.has_extra_cursors()
            && self.should_skip_auto_closing_delimiter(new_text)
        {
            self.cursor
                .move_to(self.cursor.offset + new_text.len(), &self.buffer);
            self.pause_blinking(cx);
            if !self.source_mode && self.wiki_link_start.is_some() {
                self.sync_wiki_link_menu(cx);
            }
            return;
        }
        if self.active_tab_is_markdown()
            && new_text == "/"
            && !self.cursor.has_selection()
            && !self.has_extra_cursors()
            && self.marked_range.is_none()
            && range_utf16.is_none()
        {
            self.capture_slash_menu_anchor_before_edit();
        }
        self.capture_typing_scroll_guard();
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_text_in_range start range_utf16={:?} new_text={:?} marked_before={:?} selection_before={:?}",
                range_utf16,
                new_text,
                self.marked_range,
                self.cursor.selection_range()
            ));
        }
        let had_ime_composition = self.ime_composition_snapshot.is_some();
        if !had_ime_composition {
            self.checkpoint_for_typing(new_text);
        }
        let native_range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range));
        if !self.source_mode
            && self.replace_table_cell_native_newlines(native_range, new_text, None, false, cx)
        {
            if had_ime_composition {
                self.finish_ime_composition();
            }
            return;
        }

        if self.has_extra_cursors()
            && (had_ime_composition
                || range_utf16.is_some()
                || self.marked_range.is_some()
                || !self.extra_marked_ranges.is_empty())
        {
            let range_spans_multiple_lines = self.apply_multi_cursor_ime_replace(
                range_utf16
                    .as_ref()
                    .map(|range_utf16| self.range_from_utf16(range_utf16)),
                new_text,
                None,
                false,
            );
            self.reparse();
            if !cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n')
            {
                self.scroll_to_cursor();
            }
            self.pause_blinking(cx);
            if had_ime_composition {
                self.finish_ime_composition();
            }
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "replace_text_in_range multi-cursor end cursor={} marked_after={:?} extra_marked_count={}",
                    self.cursor.offset,
                    self.marked_range,
                    self.extra_marked_ranges.len()
                ));
            }
            return;
        }

        if !self.source_mode
            && self.cursor.has_selection()
            && self.marked_range.is_none()
            && range_utf16.is_none()
        {
            if let Some((prefix, suffix)) = input_ime::paired_delimiter(new_text) {
                self.apply_wrap(prefix, suffix);
                self.pause_blinking(cx);
                return;
            }
            if !self.source_mode {
                let in_code = self.offset_in_code_context(self.cursor.selection_range().start);
                let wrap = match new_text {
                    "*" if !in_code => Some(("**", "**")),
                    "~" if !in_code => Some(("~~", "~~")),
                    "+" if !in_code => Some(("++", "++")),
                    "`" => Some(("`", "`")),
                    _ => None,
                };
                if let Some((prefix, suffix)) = wrap {
                    self.apply_wrap(prefix, suffix);
                    self.pause_blinking(cx);
                    return;
                }
            }
        }

        if self.has_extra_cursors() && self.marked_range.is_none() && range_utf16.is_none() {
            self.multi_cursor_insert(new_text);
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_text_in_range resolved range_utf8={:?}",
                range
            ));
        }
        let preserve_preview_scroll = self
            .decorations
            .get(cursor_line_before)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .is_some_and(|preview| {
                preview.source_range.start <= self.cursor.offset
                    && self.cursor.offset <= preview.source_range.end
            });
        if preserve_preview_scroll {
            self.pending_preview_reveal = Some(PendingPreviewReveal {
                scroll_offset: self.scroll_offset,
            });
        }
        let (range_start_line, _) = self.buffer.offset_to_line_col(range.start);
        let (range_end_line, _) = self.buffer.offset_to_line_col(range.end);
        let range_spans_multiple_lines = range_start_line != range_end_line;
        let touched_ordered_prefix = if self.active_tab_is_markdown() {
            self.ordered_list_prefix_touched_by_range(&range)
        } else {
            None
        };
        let markdown_syntax_edit = self.active_tab_is_markdown()
            && (new_text.contains(['=', '*', '_', '~', '`', '+', '[', ']', '(', ')'])
                || self
                    .buffer
                    .text()
                    .get(range.clone())
                    .is_some_and(|text| text.contains(['=', '*', '_', '~', '`', '+'])));

        if !self.source_mode
            && range.is_empty()
            && self.marked_range.is_none()
            && range_utf16.is_none()
        {
            if let Some((open, close)) =
                input_ime::paired_delimiter_at(self.buffer.text(), range.start, new_text)
                && !(matches!(new_text, "*" | "_" | "~" | "+" | "=")
                    && self.offset_in_code_context(range.start))
            {
                let mut paired_text = String::with_capacity(open.len() + close.len());
                paired_text.push_str(open);
                paired_text.push_str(close);
                self.queue_non_markdown_line_display_transfer(&range, &paired_text);
                self.buffer.replace(range.clone(), &paired_text);
                self.cursor.move_to(range.start + open.len(), &self.buffer);
                if markdown_syntax_edit {
                    self.invalidate_line_display_info_for_line(range_start_line);
                }
                self.reparse();
                if !preserve_preview_scroll
                    && (!cursor_had_inline_headroom || self.cursor_line() != cursor_line_before)
                {
                    self.scroll_to_cursor();
                }
                self.pause_blinking(cx);
                return;
            }
        }

        let key_colon_space = range.is_empty()
            && new_text == ":"
            && self.marked_range.is_none()
            && range_utf16.is_none()
            && self.active_tab_is_markdown()
            && super::front_matter::key_colon_needs_space(
                &self.buffer,
                &self.decorations,
                range.start,
            );
        let inserted = if key_colon_space { ": " } else { new_text };
        self.queue_non_markdown_line_display_transfer(&range, inserted);
        self.buffer.replace(range.clone(), inserted);
        let new_offset = range.start + inserted.len();
        self.cursor.move_to(new_offset, &self.buffer);
        if self.active_tab_is_markdown()
            && !range_spans_multiple_lines
            && !new_text.contains('\n')
            && markdown::newly_typed_fence_is_pending(
                self.buffer.text(),
                range_start_line,
                &self.decorations,
            )
        {
            self.pending_code_fence = Some((
                range_start_line,
                self.buffer.line_content(range_start_line).to_string(),
            ));
        } else {
            self.pending_code_fence = None;
        }
        self.clear_marked_ranges();
        if markdown_syntax_edit {
            self.invalidate_line_display_info_for_line(range_start_line);
        }
        self.reparse();
        if let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        }
        if !preserve_preview_scroll
            && (!cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n'))
        {
            self.scroll_to_cursor();
        }
        if !self.source_mode {
            if self.slash_menu_start.is_some() {
                self.update_slash_filter(cx);
            } else if self.wiki_link_start.is_some() {
                self.sync_wiki_link_menu(cx);
            } else if self.table_source.is_some() {
            } else if new_text == "[" && new_offset >= 2 {
                let text = self.buffer.text();
                if text.get(new_offset - 2..new_offset) == Some("[[") {
                    let closer = input_ime::missing_wiki_link_closer(text, new_offset);
                    if !closer.is_empty() {
                        self.buffer.replace(new_offset..new_offset, closer);
                        self.reparse();
                    }
                    self.cursor.move_to(new_offset, &self.buffer);
                    self.ensure_wiki_link_menu_for_cursor(cx);
                }
            } else if self.maybe_open_wiki_link_menu_after_edit(cx) {
            } else if new_text == "/" && self.active_tab_is_markdown() && !self.embedded_editor {
                let (line, col) = self.buffer.offset_to_line_col(new_offset);
                let line_content = self.buffer.line_content(line);
                if input_ime::slash_menu_trigger(
                    line_content,
                    col,
                    self.offset_in_code_context(new_offset),
                ) {
                    self.open_slash_menu(cx);
                }
            }
        }
        self.pause_blinking(cx);
        if had_ime_composition {
            self.finish_ime_composition();
        }
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_text_in_range end cursor={} marked_after={:?}",
                self.cursor.offset, self.marked_range
            ));
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        mut range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accepts_fold_ime_callback(false, window, cx) {
            return;
        }
        if range_utf16
            .as_ref()
            .is_some_and(|range| self.ime_range_touches_folded_front_matter(range))
        {
            return;
        }
        let replaces_text = range_utf16.as_ref().is_some_and(|range| !range.is_empty())
            || self
                .marked_range
                .as_ref()
                .is_some_and(|range| !range.is_empty());
        if !new_text.is_empty() || replaces_text {
            if !self.begin_content_edit(cx) {
                return;
            }
            if !self.materialize_native_table_boundary(&mut range_utf16, cx) {
                return;
            }
        } else if self.active_tab_is_read_only() {
            return;
        }
        self.last_scroll_cursor_visual_x = None;
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        self.capture_typing_scroll_guard();
        self.clear_phantom_selection();
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_and_mark_text_in_range start range_utf16={:?} new_text={:?} new_selected_range_utf16={:?} marked_before={:?} selection_before={:?}",
                range_utf16,
                new_text,
                new_selected_range_utf16,
                self.marked_range,
                self.cursor.selection_range()
            ));
        }
        self.begin_ime_composition();
        let native_range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range));
        if !self.source_mode
            && self.replace_table_cell_native_newlines(
                native_range,
                new_text,
                new_selected_range_utf16.as_ref(),
                true,
                cx,
            )
        {
            return;
        }

        if self.has_extra_cursors() {
            let range_spans_multiple_lines = self.apply_multi_cursor_ime_replace(
                range_utf16
                    .as_ref()
                    .map(|range_utf16| self.range_from_utf16(range_utf16)),
                new_text,
                new_selected_range_utf16.as_ref(),
                true,
            );
            self.reparse();
            if !cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n')
            {
                self.scroll_to_cursor();
            }
            self.pause_blinking(cx);
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "replace_and_mark_text_in_range multi-cursor end selection=({},{}) marked_after={:?} extra_marked_count={}",
                    self.cursor.anchor,
                    self.cursor.offset,
                    self.marked_range,
                    self.extra_marked_ranges.len()
                ));
            }
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_and_mark_text_in_range resolved range_utf8={:?}",
                range
            ));
        }
        let preserve_preview_scroll = self
            .decorations
            .get(cursor_line_before)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .is_some_and(|preview| {
                preview.source_range.start <= self.cursor.offset
                    && self.cursor.offset <= preview.source_range.end
            });
        if preserve_preview_scroll {
            self.pending_preview_reveal = Some(PendingPreviewReveal {
                scroll_offset: self.scroll_offset,
            });
        }
        let (range_start_line, _) = self.buffer.offset_to_line_col(range.start);
        let (range_end_line, _) = self.buffer.offset_to_line_col(range.end);
        let range_spans_multiple_lines = range_start_line != range_end_line;
        let touched_ordered_prefix = if self.active_tab_is_markdown() {
            self.ordered_list_prefix_touched_by_range(&range)
        } else {
            None
        };

        self.queue_non_markdown_line_display_transfer(&range, new_text);
        self.buffer.replace(range.clone(), new_text);

        let mark_start = range.start;
        let mark_end = range.start + new_text.len();
        self.extra_marked_ranges.clear();
        if !new_text.is_empty() {
            self.marked_range = Some(mark_start..mark_end);
        } else {
            self.marked_range = None;
        }

        let (selection_start, selection_end) = new_selected_range_utf16
            .as_ref()
            .map(|range| {
                let start = mark_start + Self::utf8_offset_in_text_for_utf16(new_text, range.start);
                let end = mark_start + Self::utf8_offset_in_text_for_utf16(new_text, range.end);
                (start, end)
            })
            .unwrap_or((mark_end, mark_end));
        self.cursor.anchor = self.buffer.clamp_offset(selection_start);
        self.cursor.offset = self.buffer.clamp_offset(selection_end);
        self.reparse();
        if let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        }
        if !preserve_preview_scroll
            && (!cursor_had_inline_headroom
                || self.cursor_line() != cursor_line_before
                || range_spans_multiple_lines
                || new_text.contains('\n'))
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "replace_and_mark_text_in_range end selection=({},{}) marked_after={:?}",
                self.cursor.anchor, self.cursor.offset, self.marked_range
            ));
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if !self.active_tab_supports_text_buffer() {
            return None;
        }
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "bounds_for_range start range_utf16={:?}",
                range_utf16
            ));
        }
        let queried_range = self.range_from_utf16(&range_utf16);
        let anchor_offset = if self.marked_range.as_ref().is_some_and(|marked| {
            queried_range.start >= marked.start && queried_range.end <= marked.end
        }) {
            self.buffer.clamp_offset(self.cursor.offset)
        } else {
            queried_range.start
        };
        let editor_bounds = if self.last_viewport_width > 0.0 && self.last_viewport_height > 0.0 {
            Bounds::new(
                self.content_origin,
                size(px(self.last_viewport_width), px(self.last_viewport_height)),
            )
        } else {
            bounds
        };
        self.refresh_front_matter_input_geometry(window, editor_bounds);
        let (line, col) = self.buffer.offset_to_line_col(anchor_offset);
        if self.front_matter_presentation_line_is_hidden(line) {
            return None;
        }
        let anchor_rows = self
            .line_display_info
            .get(line)
            .and_then(|info| info.as_ref())
            .map(|info| info.visual_line_count);
        if stable_input_bounds_are_usable(
            self.layout_dirty,
            self.front_matter_group_geometry_dirty,
            anchor_rows,
        ) && let Some(bounds) = self.last_stable_input_bounds
        {
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "bounds_for_range using cached stable bounds during layout_dirty anchor_utf8={} -> x={} y={} w={} h={}",
                    anchor_offset,
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height)
                ));
            }
            return Some(bounds);
        }
        if stable_input_bounds_are_usable(
            self.layout_dirty,
            self.front_matter_group_geometry_dirty,
            anchor_rows,
        ) && let Some(bounds) = self.pending_input_bounds
        {
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "bounds_for_range using pending input bounds during layout_dirty anchor_utf8={} -> x={} y={} w={} h={}",
                    anchor_offset,
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height)
                ));
            }
            return Some(bounds);
        }
        let origin_y: f32 = editor_bounds.origin.y.into();
        let text_top_y = origin_y
            + self.current_front_matter_input_top(window, line, editor_bounds)
            - self.scroll_offset;

        if let Some(table_info) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
            && uses_table_ime_geometry(self.active_tab_is_diff(), table_info)
        {
            let bounds = self.table_row_bounds_for_offset(
                window,
                anchor_offset,
                self.wrap_boundary_affinity_for_offset(anchor_offset),
            )?;
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "bounds_for_range table line={} col={} queried_utf8={:?} anchor_utf8={} -> x={} y={} h={}",
                    line,
                    col,
                    queried_range,
                    anchor_offset,
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.height)
                ));
            }
            self.pending_input_bounds = None;
            self.last_stable_input_bounds = Some(bounds);
            return Some(bounds);
        }

        let cached_line_bounds = |this: &Self| {
            this.line_display_info
                .get(line)
                .and_then(|info| info.as_deref())
                .filter(|info| {
                    cached_input_geometry_is_visible(
                        info.visual_line_count,
                        info.line_height,
                        this.front_matter_group_geometry_dirty,
                    )
                })
                .map(|info| {
                    this.input_bounds_for_display_line(
                        anchor_offset,
                        line,
                        col,
                        text_top_y,
                        &info.display_text,
                        &info.buffer_to_display,
                        &info.wrapped_line,
                        info.font_size,
                        info.line_height,
                        info.indent_px,
                        &info.wrap_boundary_upstream_xs,
                        info.content_x,
                    )
                })
        };
        let inline_line_bounds = |this: &Self, window: &mut Window| -> Option<Bounds<Pixels>> {
            this.inline_line_display_info(window, line, editor_bounds)
                .map(|info: InlineLineDisplayInfo| {
                    this.input_bounds_for_display_line(
                        anchor_offset,
                        line,
                        col,
                        text_top_y,
                        &info.display_text,
                        &info.buffer_to_display,
                        &info.wrapped_line,
                        info.font_size,
                        info.line_height,
                        info.indent_px,
                        &info.wrap_boundary_upstream_xs,
                        info.content_x,
                    )
                })
        };
        let line_bounds = if self.layout_dirty || self.front_matter_group_geometry_dirty {
            inline_line_bounds(self, window).or_else(|| cached_line_bounds(self))
        } else {
            cached_line_bounds(self).or_else(|| inline_line_bounds(self, window))
        };

        if let Some(bounds) = line_bounds {
            self.pending_input_bounds = None;
            self.last_stable_input_bounds = Some(bounds);
            Some(bounds)
        } else {
            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            let line_height = self.line_height_for_block(block_type);
            let x = self.last_content_left + col as f32 * self.base_font_size() * 0.6;
            let bounds = Bounds::new(
                point(px(x), px(text_top_y)),
                size(px(self.base_font_size() * 0.6), px(line_height)),
            );
            self.pending_input_bounds = None;
            self.last_stable_input_bounds = Some(bounds);
            Some(bounds)
        }
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if let Some((target_point, target_offset)) = self.last_mouse_down_target.take() {
            let dx = (f32::from(target_point.x) - f32::from(point.x)).abs();
            let dy = (f32::from(target_point.y) - f32::from(point.y)).abs();
            let utf16_offset = self.offset_to_utf16(target_offset);
            if ime_debug_enabled() {
                ime_debug_log(format!(
                    "character_index_for_point consumed_latch point=({}, {}) target=({}, {}) dx={} dy={} -> utf8={} utf16={}",
                    f32::from(point.x),
                    f32::from(point.y),
                    f32::from(target_point.x),
                    f32::from(target_point.y),
                    dx,
                    dy,
                    target_offset,
                    utf16_offset
                ));
            }
            return Some(utf16_offset);
        }

        self.refresh_front_matter_input_geometry(window, self.front_matter_input_viewport_bounds());
        let table_offset = self
            .table_click_areas
            .iter()
            .find(|(bounds, ..)| bounds.contains(&point))
            .and_then(|(bounds, table, row, column)| {
                self.table_cell_hit_for_position(window, *table, *row, *column, bounds, point)
            })
            .map(|(offset, _)| offset);
        let utf8_offset = table_offset.unwrap_or_else(|| self.offset_for_position(point));
        let utf16_offset = self.offset_to_utf16(utf8_offset);
        if ime_debug_enabled() {
            ime_debug_log(format!(
                "character_index_for_point point=({}, {}) -> utf8={} utf16={}",
                f32::from(point.x),
                f32::from(point.y),
                utf8_offset,
                utf16_offset
            ));
        }
        Some(utf16_offset)
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
