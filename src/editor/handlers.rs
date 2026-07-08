use gpui::{Context, KeyDownEvent, Window};

use crate::actions::*;
use crate::markdown::{self, BlockType};

use super::*;

impl EditorView {
    // ── Action handlers ─────────────────────────────────────────────

    fn finish_horizontal_cursor_move(&mut self, cx: &mut Context<Self>) {
        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        self.pause_blinking(cx);
    }

    fn apply_history_snapshot(
        &mut self,
        cx: &mut Context<Self>,
        take_snapshot: impl FnOnce(
            &mut history_store::TabHistory,
            &EditorSnapshot,
        ) -> Option<EditorSnapshot>,
    ) {
        let current = self.current_snapshot();
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let Some(snapshot) = take_snapshot(&mut self.tabs[index].history, &current) else {
            return;
        };

        self.restore_snapshot(snapshot);
        self.reset_typing_checkpoint_grouping();
        self.tabs[index].snapshot = self.current_snapshot();
        self.sync_active_tab_dirty_state();
        session_store::persist_session(self);
        self.pause_blinking(cx);
    }

    pub(crate) fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_undo(cx);
            return;
        }
        self.apply_history_snapshot(cx, |history, current| history.take_undo(current));
    }

    pub(crate) fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() && self.whiteboard_escape() {
            cx.notify();
            return;
        }
        if self.preview_overlay.is_some() {
            self.close_preview_overlay();
            cx.notify();
            return;
        }
        if self.settings_font_picker.is_some() {
            self.close_font_picker(cx);
            return;
        }
        if self.quick_open_visible {
            self.close_quick_open(window, cx);
            return;
        }
        if self.language_picker_visible {
            self.close_language_picker(window, cx);
            return;
        }
        if self.workspace_search_visible {
            self.close_workspace_search(window, cx);
            return;
        }
        if self.dialog.is_some() {
            self.dismiss_dialog(cx);
            return;
        }
        if self.search_visible {
            self.dismiss_search(window, cx);
            return;
        }
        if self.sidebar_edit.is_some() {
            self.cancel_sidebar_edit(cx);
            return;
        }
        if self.context_menu.is_some() {
            self.close_context_menu(cx);
            return;
        }
        if self.clear_phantom_selection() {
            cx.notify();
            return;
        }
        if self.has_extra_cursors() {
            self.clear_extra_cursors();
            cx.notify();
            return;
        }
        if self.focused_sidebar_path.is_some() {
            self.focused_sidebar_path = None;
            cx.notify();
        }
    }

    pub(crate) fn on_preview_overlay_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview_overlay.is_none() {
            return;
        }

        if matches!(event.keystroke.key.as_ref(), "escape") {
            window.prevent_default();
            cx.stop_propagation();
            self.close_preview_overlay();
            cx.notify();
        }
    }

    pub(crate) fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_redo(cx);
            return;
        }
        self.apply_history_snapshot(cx, |history, current| history.take_redo(current));
    }

    pub(crate) fn on_backspace(
        &mut self,
        _: &Backspace,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_delete_selected(cx);
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let should_normalize_ordered_lists = self.active_tab_is_markdown();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        let selection = self.cursor.selection_range();
        let selection_spans_multiple_lines = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let (end_line, _) = self.buffer.offset_to_line_col(selection.end);
            start_line != end_line
        } else {
            false
        };
        let selection_line_span = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let end_probe = selection
                .end
                .saturating_sub(usize::from(selection.end > selection.start));
            let (end_line, _) = self
                .buffer
                .offset_to_line_col(end_probe.min(self.buffer.len()));
            Some((start_line, end_line))
        } else {
            None
        };
        let touched_ordered_prefix = if should_normalize_ordered_lists {
            if self.cursor.has_selection() {
                self.ordered_list_prefix_touched_by_range(&selection)
            } else {
                self.cursor
                    .offset
                    .checked_sub(1)
                    .and_then(|offset| self.ordered_list_prefix_touched_by_range(&(offset..offset)))
            }
        } else {
            None
        };
        let removes_line_break = if self.cursor.has_selection() {
            let selection =
                self.buffer.clamp_offset(selection.start)..self.buffer.clamp_offset(selection.end);
            self.buffer.text()[selection].contains('\n')
        } else {
            self.cursor.offset > 0
                && self.buffer.text().as_bytes().get(self.cursor.offset - 1) == Some(&b'\n')
        };

        // Multi-cursor backspace
        if self.has_extra_cursors() {
            self.multi_cursor_backspace();
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        if let Some((table_start_line, is_column, index)) = self.selected_handle.take() {
            if is_column && self.block_is_front_matter(table_start_line) {
                self.pause_blinking(cx);
                return;
            }
            if is_column {
                self.table_delete_column(table_start_line, index, cx);
            } else {
                self.table_delete_row(table_start_line, index, cx);
            }
            self.pause_blinking(cx);
            return;
        }

        if !self.cursor.has_selection() {
            let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
            let line_start = self.buffer.line_start(line);
            let line_content = self.buffer.line_content(line).to_string();
            let leading_ws =
                line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
            let trimmed = &line_content[leading_ws..];
            let prefix_len = match self.decorations.get(line).map(|d| d.block_type.clone()) {
                Some(BlockType::UnorderedList) => {
                    if trimmed.starts_with("- ")
                        || trimmed.starts_with("* ")
                        || trimmed.starts_with("+ ")
                    {
                        2
                    } else {
                        0
                    }
                }
                Some(BlockType::OrderedList) => markdown::ordered_list_prefix_len(trimmed),
                Some(BlockType::TaskList(_)) => task_prefix_len(trimmed),
                Some(BlockType::Blockquote) => {
                    if trimmed.starts_with("> ") {
                        2
                    } else if trimmed.starts_with('>') {
                        1
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            if prefix_len > 0 && col == leading_ws + prefix_len {
                let del_start = line_start + leading_ws;
                let del_end = line_start + leading_ws + prefix_len;
                self.queue_non_markdown_line_display_transfer(&(del_start..del_end), "");
                self.buffer.replace(del_start..del_end, "");
                self.cursor.move_to(del_start, &self.buffer);
                self.reparse();
                if should_normalize_ordered_lists {
                    self.normalize_all_ordered_lists();
                    self.reparse();
                }
                if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
                    self.scroll_to_cursor();
                }
                self.pause_blinking(cx);
                return;
            }

            if let Some(delete_range) = self.auto_paired_backspace_range() {
                self.queue_non_markdown_line_display_transfer(&delete_range, "");
                self.buffer.delete(delete_range.clone());
                self.cursor.move_to(delete_range.start, &self.buffer);
                self.reparse();
                if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
                    self.scroll_to_cursor();
                }
                self.pause_blinking(cx);
                if self.wiki_link_start.is_some() {
                    self.sync_wiki_link_menu(cx);
                }
                return;
            }
        }
        let delete_range = if self.cursor.has_selection() {
            Some(self.cursor.selection_range())
        } else if self.cursor.offset > 0 {
            let prev = self.buffer.prev_grapheme_boundary(self.cursor.offset);
            Some(prev..self.cursor.offset)
        } else {
            None
        };
        if let Some(delete_range) = delete_range.as_ref() {
            self.queue_non_markdown_line_display_transfer(delete_range, "");
        }
        self.cursor.backspace(&mut self.buffer);
        self.reparse();
        if should_normalize_ordered_lists
            && let Some((start_line, end_line)) = selection_line_span
            && (touched_ordered_prefix.is_some() || removes_line_break)
        {
            let _ = (start_line, end_line);
            if self.normalize_all_ordered_lists() {
                self.reparse();
            }
        } else if should_normalize_ordered_lists && let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        } else if should_normalize_ordered_lists && removes_line_break {
            if self.normalize_all_ordered_lists() {
                self.reparse();
            }
        }
        if !cursor_had_inline_headroom
            || (!removes_line_break
                && !selection_spans_multiple_lines
                && self.cursor_line() != cursor_line_before)
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);

        if let Some(start) = self.slash_menu_start {
            if self.cursor.offset <= start {
                self.slash_menu_start = None;
                self.slash_filter.clear();
                self.context_menu = None;
                cx.notify();
            } else {
                self.update_slash_filter(cx);
            }
        } else if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        } else {
            self.maybe_open_wiki_link_menu_after_edit(cx);
        }
    }

    pub(crate) fn on_delete(&mut self, _: &Delete, _window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_delete_selected(cx);
            return;
        }
        if self.focused_sidebar_path.is_some() {
            self.sidebar_delete_focused(cx);
            return;
        }
        if let Some((table_start_line, is_column, index)) = self.selected_handle.take() {
            if is_column && self.block_is_front_matter(table_start_line) {
                self.pause_blinking(cx);
                return;
            }
            if is_column {
                self.table_delete_column(table_start_line, index, cx);
            } else {
                self.table_delete_row(table_start_line, index, cx);
            }
            self.pause_blinking(cx);
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let should_normalize_ordered_lists = self.active_tab_is_markdown();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        let selection = self.cursor.selection_range();
        let selection_spans_multiple_lines = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let (end_line, _) = self.buffer.offset_to_line_col(selection.end);
            start_line != end_line
        } else {
            false
        };
        let selection_line_span = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let end_probe = selection
                .end
                .saturating_sub(usize::from(selection.end > selection.start));
            let (end_line, _) = self
                .buffer
                .offset_to_line_col(end_probe.min(self.buffer.len()));
            Some((start_line, end_line))
        } else {
            None
        };
        let touched_ordered_prefix = if should_normalize_ordered_lists {
            if self.cursor.has_selection() {
                self.ordered_list_prefix_touched_by_range(&selection)
            } else {
                self.ordered_list_prefix_touched_by_range(&(self.cursor.offset..self.cursor.offset))
            }
        } else {
            None
        };
        let removes_line_break = if self.cursor.has_selection() {
            let selection =
                self.buffer.clamp_offset(selection.start)..self.buffer.clamp_offset(selection.end);
            self.buffer.text()[selection].contains('\n')
        } else {
            self.buffer.text().as_bytes().get(self.cursor.offset) == Some(&b'\n')
        };
        if self.has_extra_cursors() {
            self.multi_cursor_delete();
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        let delete_range = if self.cursor.has_selection() {
            Some(self.cursor.selection_range())
        } else if self.cursor.offset < self.buffer.len() {
            let next = self.buffer.next_grapheme_boundary(self.cursor.offset);
            Some(self.cursor.offset..next)
        } else {
            None
        };
        if let Some(delete_range) = delete_range.as_ref() {
            self.queue_non_markdown_line_display_transfer(delete_range, "");
        }
        self.cursor.delete(&mut self.buffer);
        self.reparse();
        if should_normalize_ordered_lists
            && let Some((start_line, end_line)) = selection_line_span
            && (touched_ordered_prefix.is_some() || removes_line_break)
        {
            let _ = (start_line, end_line);
            if self.normalize_all_ordered_lists() {
                self.reparse();
            }
        } else if should_normalize_ordered_lists && let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        } else if should_normalize_ordered_lists && removes_line_break {
            if self.normalize_all_ordered_lists() {
                self.reparse();
            }
        }
        if !cursor_had_inline_headroom
            || (!removes_line_break
                && !selection_spans_multiple_lines
                && self.cursor_line() != cursor_line_before)
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        } else {
            self.maybe_open_wiki_link_menu_after_edit(cx);
        }
    }

    pub(crate) fn on_left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_nudge(-whiteboard_geometry::GRID, 0.0, cx);
            return;
        }
        self.clear_phantom_selection();
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_left(cx);
            return;
        }
        let had_selection = self.cursor.has_selection();
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_left();
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        let (line, _col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let cell_ranges = self
            .decorations
            .get(line)
            .and_then(|d| d.table_info.as_ref())
            .filter(|ti| !ti.is_separator)
            .map(|ti| ti.cell_ranges.clone());
        self.cursor.move_left(&self.buffer);
        if let Some(cell_ranges) = cell_ranges {
            let (_, new_col) = self.buffer.offset_to_line_col(self.cursor.offset);
            let mut jumped = false;
            for i in 0..cell_ranges.len().saturating_sub(1) {
                let gap_start = cell_ranges[i].end.saturating_add(1);
                let gap_end = cell_ranges[i + 1].start;
                if new_col >= gap_start && new_col < gap_end {
                    let line_start = self.buffer.line_start(line);
                    self.cursor
                        .move_to(line_start + cell_ranges[i].end, &self.buffer);
                    jumped = true;
                    break;
                }
            }
            if !jumped && !cell_ranges.is_empty() && new_col < cell_ranges[0].start {
                let line_start = self.buffer.line_start(line);
                self.cursor
                    .move_to(line_start + cell_ranges[0].start, &self.buffer);
            }
        }
        if !had_selection
            && self.cursor.offset != prev_offset
            && self.is_wrap_boundary_offset(self.cursor.offset)
        {
            self.set_wrap_boundary_affinity(
                self.cursor.offset,
                WrapBoundaryAffinityKind::Downstream,
            );
        }
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.finish_horizontal_cursor_move(cx);
        if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        }
    }

    pub(crate) fn on_right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_nudge(whiteboard_geometry::GRID, 0.0, cx);
            return;
        }
        self.clear_phantom_selection();
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_right(cx);
            return;
        }
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_right();
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        let (line, _col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let cell_ranges = self
            .decorations
            .get(line)
            .and_then(|d| d.table_info.as_ref())
            .filter(|ti| !ti.is_separator)
            .map(|ti| ti.cell_ranges.clone());
        self.cursor.move_right(&self.buffer);
        if let Some(cell_ranges) = cell_ranges {
            let (_, new_col) = self.buffer.offset_to_line_col(self.cursor.offset);
            for i in 0..cell_ranges.len().saturating_sub(1) {
                let gap_start = cell_ranges[i].end.saturating_add(1);
                let gap_end = cell_ranges[i + 1].start;
                if new_col >= gap_start && new_col < gap_end {
                    let line_start = self.buffer.line_start(line);
                    self.cursor
                        .move_to(line_start + cell_ranges[i + 1].start, &self.buffer);
                    break;
                }
            }
        }
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.finish_horizontal_cursor_move(cx);
        if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        }
    }

    pub(crate) fn on_up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_nudge(0.0, -whiteboard_geometry::GRID, cx);
            return;
        }
        self.clear_phantom_selection();
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_up(cx);
            return;
        }
        if self.wiki_link_start.is_some() {
            if self.menu_selected_index > 0 {
                self.menu_selected_index -= 1;
            }
            cx.notify();
            return;
        }
        if self.slash_menu_start.is_some() {
            if self.menu_selected_index > 0 {
                self.menu_selected_index -= 1;
            }
            cx.notify();
            return;
        }
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_move_up();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        if self.vertical_move_goal_x.is_none() {
            self.vertical_move_goal_x = self.cursor_visual_x(self.cursor.offset);
        }
        let goal_x = self.vertical_move_goal_x.unwrap_or_default();
        if let Some(offset) = self.offset_for_visual_up(self.cursor.offset) {
            self.cursor.move_to(offset, &self.buffer);
        } else {
            self.cursor.move_up(&self.buffer);
        }
        self.snap_cursor_to_content();
        self.update_wrap_boundary_affinity_for_goal_x(self.cursor.offset, goal_x);
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_nudge(0.0, whiteboard_geometry::GRID, cx);
            return;
        }
        self.clear_phantom_selection();
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_down(cx);
            return;
        }
        if self.wiki_link_start.is_some() {
            let max_index = self.wiki_link_result_count().saturating_sub(1);
            if self.menu_selected_index < max_index {
                self.menu_selected_index += 1;
            }
            cx.notify();
            return;
        }
        if self.slash_menu_start.is_some() {
            let max_index = self.slash_menu_matched_actions().len().saturating_sub(1);
            if self.menu_selected_index < max_index {
                self.menu_selected_index += 1;
            }
            cx.notify();
            return;
        }
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_move_down();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        if self.vertical_move_goal_x.is_none() {
            self.vertical_move_goal_x = self.cursor_visual_x(self.cursor.offset);
        }
        let goal_x = self.vertical_move_goal_x.unwrap_or_default();
        if let Some(offset) = self.offset_for_visual_down(self.cursor.offset) {
            self.cursor.move_to(offset, &self.buffer);
        } else {
            self.cursor.move_down(&self.buffer);
        }
        self.snap_cursor_to_content();
        self.update_wrap_boundary_affinity_for_goal_x(self.cursor.offset, goal_x);
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_left(
        &mut self,
        _: &SelectLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_select_left();
        } else {
            self.cursor.select_left(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_right(
        &mut self,
        _: &SelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_select_right();
        } else {
            self.cursor.select_right(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_select_up();
        } else {
            if self.vertical_move_goal_x.is_none() {
                self.vertical_move_goal_x = self.cursor_visual_x(self.cursor.offset);
            }
            let goal_x = self.vertical_move_goal_x.unwrap_or_default();
            if let Some(offset) = self.offset_for_visual_up(self.cursor.offset) {
                self.cursor.select_to(offset, &self.buffer);
            } else {
                self.cursor.select_up(&self.buffer);
            }
            self.update_wrap_boundary_affinity_for_goal_x(self.cursor.offset, goal_x);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_down(
        &mut self,
        _: &SelectDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_select_down();
        } else {
            if self.vertical_move_goal_x.is_none() {
                self.vertical_move_goal_x = self.cursor_visual_x(self.cursor.offset);
            }
            let goal_x = self.vertical_move_goal_x.unwrap_or_default();
            if let Some(offset) = self.offset_for_visual_down(self.cursor.offset) {
                self.cursor.select_to(offset, &self.buffer);
            } else {
                self.cursor.select_down(&self.buffer);
            }
            self.update_wrap_boundary_affinity_for_goal_x(self.cursor.offset, goal_x);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_select_all(cx);
            return;
        }
        self.cursor.select_all(&self.buffer);
        self.pause_blinking(cx);
    }

    pub(crate) fn on_home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_phantom_selection();
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_to_line_start();
        } else {
            self.cursor.move_to_line_start(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_phantom_selection();
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_to_line_end();
        } else {
            self.cursor.move_to_line_end(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_home(
        &mut self,
        _: &SelectHome,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_extra_cursors() {
            self.multi_cursor_select_to_line_start();
        } else {
            self.cursor.select_to_line_start(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_extra_cursors() {
            self.multi_cursor_select_to_line_end();
        } else {
            self.cursor.select_to_line_end(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            if self.sidebar_edit.is_some() {
                self.commit_sidebar_edit(cx);
            } else {
                self.sidebar_confirm_focused(cx);
            }
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        if self.wiki_link_start.is_some() {
            self.confirm_wiki_link_selection(cx);
            return;
        }
        if self.slash_menu_start.is_some() {
            self.execute_slash_menu_selected(cx);
            return;
        }
        if self.sidebar_edit.is_some() {
            self.commit_sidebar_edit(cx);
            return;
        }
        self.checkpoint();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();

        if self.has_extra_cursors() {
            self.multi_cursor_enter();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        let cursor_line = self.cursor_line();
        let line_start = self.buffer.line_start(cursor_line);
        let line_end = self.buffer.line_end(cursor_line);
        let line_content = self.buffer.line_content(cursor_line).to_string();
        let block_type = self
            .decorations
            .get(cursor_line)
            .map(|d| d.block_type.clone())
            .unwrap_or(BlockType::Normal);

        let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        let trimmed = &line_content[leading_ws..];
        let ws_str = " ".repeat(leading_ws);

        match block_type {
            BlockType::UnorderedList => {
                let marker_len = if trimmed.starts_with("- ")
                    || trimmed.starts_with("* ")
                    || trimmed.starts_with("+ ")
                {
                    2
                } else {
                    0
                };
                if marker_len > 0 && self.cursor.offset >= line_start + leading_ws + marker_len {
                    let text_after_marker = &line_content[leading_ws + marker_len..];
                    if text_after_marker.trim().is_empty() {
                        self.queue_non_markdown_line_display_transfer(
                            &(line_start..line_end),
                            "\n",
                        );
                        self.buffer.replace(line_start..line_end, "");
                        self.cursor.move_to(line_start, &self.buffer);
                        self.cursor.insert(&mut self.buffer, "\n");
                    } else {
                        let marker = &trimmed[..marker_len];
                        let continuation = format!("\n{}{}", ws_str, marker);
                        let insert_range = self.cursor.selection_range();
                        self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
                        self.cursor.insert(&mut self.buffer, &continuation);
                    }
                    self.reparse();
                    if !cursor_had_inline_headroom {
                        self.scroll_to_cursor();
                    }
                    self.pause_blinking(cx);
                    return;
                }
            }
            BlockType::TaskList(_) => {
                let prefix_len = task_prefix_len(trimmed);
                if prefix_len > 0 && self.cursor.offset >= line_start + leading_ws + prefix_len {
                    let text_after_marker = &line_content[leading_ws + prefix_len..];
                    if text_after_marker.trim().is_empty() {
                        self.queue_non_markdown_line_display_transfer(
                            &(line_start..line_end),
                            "\n",
                        );
                        self.buffer.replace(line_start..line_end, "");
                        self.cursor.move_to(line_start, &self.buffer);
                        self.cursor.insert(&mut self.buffer, "\n");
                    } else {
                        let bullet = if trimmed.starts_with("- ") {
                            "- "
                        } else if trimmed.starts_with("* ") {
                            "* "
                        } else {
                            "+ "
                        };
                        let continuation = format!("\n{}{}[ ] ", ws_str, bullet);
                        let insert_range = self.cursor.selection_range();
                        self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
                        self.cursor.insert(&mut self.buffer, &continuation);
                    }
                    self.reparse();
                    if !cursor_had_inline_headroom {
                        self.scroll_to_cursor();
                    }
                    self.pause_blinking(cx);
                    return;
                }
            }
            BlockType::OrderedList => {
                let prefix_len = markdown::ordered_list_prefix_len(trimmed);
                if prefix_len > 0 && self.cursor.offset >= line_start + leading_ws + prefix_len {
                    let number: u32 = trimmed[..prefix_len.saturating_sub(2)].parse().unwrap_or(1);
                    let text_after_marker = &line_content[leading_ws + prefix_len..];
                    if text_after_marker.trim().is_empty() {
                        self.queue_non_markdown_line_display_transfer(
                            &(line_start..line_end),
                            "\n",
                        );
                        self.buffer.replace(line_start..line_end, "");
                        self.cursor.move_to(line_start, &self.buffer);
                        self.cursor.insert(&mut self.buffer, "\n");
                        self.reparse();
                        self.normalize_ordered_list_near_line(cursor_line, false);
                    } else {
                        let continuation = format!("\n{}{}. ", ws_str, number + 1);
                        let insert_range = self.cursor.selection_range();
                        self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
                        self.cursor.insert(&mut self.buffer, &continuation);
                        self.renumber_ordered_list_after(cursor_line + 2, number + 2, leading_ws);
                        self.reparse();
                    }
                    if !cursor_had_inline_headroom {
                        self.scroll_to_cursor();
                    }
                    self.pause_blinking(cx);
                    return;
                }
            }
            _ => {}
        }

        let insert_range = self.cursor.selection_range();
        self.queue_non_markdown_line_display_transfer(&insert_range, "\n");
        self.cursor.insert(&mut self.buffer, "\n");
        self.reparse();
        if !cursor_had_inline_headroom && self.cursor_line() != cursor_line_before {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let (line_indices, has_selection, has_list_line) =
            self.collect_line_indices_for_block_indent();
        let indent_text = self.current_indent_text();

        if has_list_line {
            self.indent_selected_list_lines();
        } else if has_selection {
            if self.has_extra_cursors() {
                self.indent_line_indices(&line_indices, &indent_text);
            } else {
                self.indent_current_lines(&indent_text);
            }
        } else {
            if self.has_extra_cursors() {
                self.multi_cursor_insert(&indent_text);
                self.reparse();
                self.scroll_to_cursor();
            } else {
                let insert_range = self.cursor.selection_range();
                self.queue_non_markdown_line_display_transfer(&insert_range, &indent_text);
                self.cursor.insert(&mut self.buffer, &indent_text);
                self.reparse();
                self.scroll_to_cursor();
            }
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_back_tab(&mut self, _: &BackTab, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let (line_indices, has_selection, has_list_line) =
            self.collect_line_indices_for_block_indent();
        let unindent_width = self.current_unindent_width();

        if has_list_line {
            self.unindent_selected_list_lines();
        } else if has_selection {
            if self.has_extra_cursors() {
                self.unindent_line_indices(&line_indices, unindent_width);
            } else {
                self.unindent_current_lines(unindent_width);
            }
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_copy(cx);
            return;
        }
        self.copy_selection_to_clipboard(cx);
    }

    pub(crate) fn on_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_cut(cx);
            return;
        }
        self.cut_selection_to_clipboard(cx);
    }

    pub(crate) fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_is_whiteboard() {
            self.whiteboard_paste(cx);
            return;
        }
        self.paste_from_clipboard(cx);
    }

    pub(crate) fn on_show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    pub(crate) fn on_move_word_left(
        &mut self,
        _: &MoveWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_phantom_selection();
        if self.has_extra_cursors() {
            self.multi_cursor_move_word_left();
        } else {
            self.cursor.move_word_left(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_phantom_selection();
        if self.has_extra_cursors() {
            self.multi_cursor_move_word_right();
        } else {
            self.cursor.move_word_right(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        if self.has_extra_cursors() {
            self.multi_cursor_delete_word_left();
        } else {
            self.cursor.delete_word_left(&mut self.buffer);
        }
        self.reparse();
        if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        if self.has_extra_cursors() {
            self.multi_cursor_delete_word_right();
        } else {
            self.cursor.delete_word_right(&mut self.buffer);
        }
        self.reparse();
        if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    // ── Format actions ──────────────────────────────────────────────

    pub(crate) fn on_toggle_bold(
        &mut self,
        _: &ToggleBold,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_wrap("**", "**");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_italic(
        &mut self,
        _: &ToggleItalic,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_wrap("*", "*");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_strikethrough(
        &mut self,
        _: &ToggleStrikethrough,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_wrap("~~", "~~");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_inline_code(
        &mut self,
        _: &ToggleInlineCode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_wrap("`", "`");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_link(
        &mut self,
        _: &ToggleLink,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.insert_link_or_image("[", "](https://)", "title");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_image(
        &mut self,
        _: &ToggleImage,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.insert_link_or_image("![", "](image.png)", "alt");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_quote(
        &mut self,
        _: &ToggleQuote,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("> ");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_todo(
        &mut self,
        _: &ToggleTodo,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("- [ ] ");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_unordered_list(
        &mut self,
        _: &ToggleUnorderedList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("- ");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_ordered_list(
        &mut self,
        _: &ToggleOrderedList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("1. ");
        self.normalize_ordered_lists_near_all_cursors(true);
        self.reparse();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_code_block(
        &mut self,
        _: &InsertCodeBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.wrap_current_lines_as_code_block();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_horizontal_rule(
        &mut self,
        _: &InsertHorizontalRule,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.toggle_horizontal_rule_at_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_heading1(
        &mut self,
        _: &InsertHeading1,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_heading(1);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading2(
        &mut self,
        _: &InsertHeading2,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_heading(2);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading3(
        &mut self,
        _: &InsertHeading3,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_heading(3);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading4(
        &mut self,
        _: &InsertHeading4,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_heading(4);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading5(
        &mut self,
        _: &InsertHeading5,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_heading(5);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading6(
        &mut self,
        _: &InsertHeading6,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() {
            return;
        }
        self.checkpoint();
        self.apply_heading(6);
        self.pause_blinking(cx);
    }

    pub(crate) fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(self.ui_scale + self.styles.editor.editor_scale_step, cx);
    }
    pub(crate) fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(self.ui_scale - self.styles.editor.editor_scale_step, cx);
    }
    pub(crate) fn on_zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(1.0, cx);
    }

    pub(crate) fn on_toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_sidebar_visibility(cx);
    }

    pub(crate) fn on_toggle_outline(
        &mut self,
        _: &ToggleOutline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_outline_visibility(cx);
    }

    pub(crate) fn on_toggle_info_panel(
        &mut self,
        _: &ToggleInfoPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_info_panel(cx);
    }

    pub(crate) fn on_toggle_git_history(
        &mut self,
        _: &ToggleGitHistory,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_git_history_panel(cx);
    }

    pub(crate) fn on_toggle_theme(
        &mut self,
        _: &ToggleTheme,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_theme(cx);
    }

    pub(crate) fn on_toggle_theme_debugger(
        &mut self,
        _: &ToggleThemeDebugger,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_theme_debugger_visibility(cx);
    }

    pub(crate) fn on_toggle_editor_left_align(
        &mut self,
        _: &ToggleEditorLeftAlign,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_editor_alignment(cx);
    }

    pub(crate) fn on_select_line(
        &mut self,
        _: &SelectLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let start = self.buffer.line_start(line);
        let end = self.buffer.line_end(line);
        let select_end = if end < self.buffer.len() {
            end + 1
        } else {
            end
        };
        self.cursor.anchor = start;
        self.cursor.offset = select_end;
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_line_below(
        &mut self,
        _: &InsertLineBelow,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let end = self.buffer.line_end(line);
        self.buffer.insert(end, "\n");
        self.cursor.offset = end + 1;
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_line_above(
        &mut self,
        _: &InsertLineAbove,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let start = self.buffer.line_start(line);
        self.buffer.insert(start, "\n");
        self.cursor.offset = start;
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_duplicate_line(
        &mut self,
        _: &DuplicateLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let start = self.buffer.line_start(line);
        let end = self.buffer.line_end(line);
        let line_text = self.buffer.text()[start..end].to_string();
        self.buffer.insert(end, &format!("\n{}", line_text));
        self.cursor.offset = end + 1 + line_text.len();
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_delete_line(
        &mut self,
        _: &DeleteLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        self.checkpoint();
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let start = self.buffer.line_start(line);
        let end = self.buffer.line_end(line);
        let delete_end = if end < self.buffer.len() {
            end + 1
        } else {
            end
        };
        let delete_start = if start == delete_end && start > 0 {
            start - 1
        } else {
            start
        };
        self.queue_non_markdown_line_display_transfer(&(delete_start..delete_end), "");
        self.buffer.delete(delete_start..delete_end);
        self.cursor.offset = delete_start.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        if self.normalize_all_ordered_lists() {
            self.reparse();
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_move_line_up(
        &mut self,
        _: &MoveLineUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
        if line == 0 {
            return;
        }
        self.checkpoint();
        let start = self.buffer.line_start(line);
        let end = self.buffer.line_end(line);
        let line_text = self.buffer.text()[start..end].to_string();
        let delete_end = if end < self.buffer.len() {
            end + 1
        } else {
            end
        };
        self.buffer.delete(start..delete_end);
        let prev_start = self.buffer.line_start(line - 1);
        let insert_text = format!("{}\n", line_text);
        self.buffer.insert(prev_start, &insert_text);
        let new_offset = prev_start + col.min(line_text.len());
        self.cursor.offset = new_offset;
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_move_line_down(
        &mut self,
        _: &MoveLineDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if self.active_tab_is_read_only() {
            return;
        }
        let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let line_count = self.buffer.line_count();
        if line >= line_count.saturating_sub(1) {
            return;
        }
        self.checkpoint();
        let start = self.buffer.line_start(line);
        let end = self.buffer.line_end(line);
        let line_text = self.buffer.text()[start..end].to_string();
        let delete_end = if end < self.buffer.len() {
            end + 1
        } else {
            end
        };
        self.buffer.delete(start..delete_end);
        let next_end = self.buffer.line_end(line);
        let insert_text = format!("\n{}", line_text);
        self.buffer.insert(next_end, &insert_text);
        let new_offset = next_end + 1 + col.min(line_text.len());
        self.cursor.offset = new_offset.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn format_markdown_tables_for_save(&self, text: &str) -> String {
        // `text` is the current buffer text (see `normalize_text_for_save`), so
        // `self.decorations` is already in sync with it. Reusing the cached
        // decorations avoids a full-document `parse_decorations` on every save,
        // which is a multi-hundred-millisecond UI freeze on large files.
        let decorations = &self.decorations;
        let mut lines = text
            .split('\n')
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let mut table_ranges = Vec::new();

        for (line_index, decoration) in decorations.iter().enumerate() {
            let Some(table_info) = decoration.table_info.as_ref() else {
                continue;
            };
            if table_info.is_front_matter
                || !table_info.is_header
                || table_info.table_start_line != line_index
            {
                continue;
            }
            table_ranges.push((table_info.table_start_line, table_info.table_end_line));
        }

        table_ranges.sort_unstable();
        table_ranges.dedup();

        for (start_line, end_line) in table_ranges.into_iter().rev() {
            if start_line >= end_line || end_line > lines.len() {
                continue;
            }
            let formatted = crate::table_ops::serialize_table(
                &crate::table_ops::parse_table_block(&lines[start_line..end_line]),
            );
            lines.splice(start_line..end_line, formatted);
        }

        lines.join("\n")
    }

    fn normalize_text_for_save(&mut self) -> String {
        let original_text = self.buffer.text().to_string();
        let mut text = if self.active_tab_is_markdown() {
            self.format_markdown_tables_for_save(&original_text)
        } else {
            original_text.clone()
        };
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }

        if text != original_text {
            self.buffer = Buffer::from_text(&text);
            self.cursor.offset = self.cursor.offset.min(self.buffer.len());
            self.cursor.anchor = self.cursor.anchor.min(self.buffer.len());
            for cursor in &mut self.extra_cursors {
                cursor.offset = cursor.offset.min(self.buffer.len());
                cursor.anchor = cursor.anchor.min(self.buffer.len());
            }
            self.reparse();
        }
        text
    }

    pub(crate) fn on_save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab_supports_persistent_save() {
            return;
        }
        let Some(index) = self.active_tab_index() else {
            return;
        };
        if self.tabs[index]
            .file_kind
            .as_ref()
            .is_some_and(FileKind::is_whiteboard)
        {
            self.save_active_whiteboard(cx);
            return;
        }
        let text = self.normalize_text_for_save();
        if let Some(ref path) = self.tabs[index].path.clone() {
            if std::fs::write(path, &text).is_ok() {
                self.tabs[index].is_dirty = false;
                let tab_id = self.tabs[index].id.clone();
                self.clear_temp_file_for_tab(&tab_id);
                self.tabs[index].snapshot = self.current_snapshot();
                let snapshot = self.tabs[index].snapshot.clone();
                let source_path = self.tabs[index].path.clone();
                let _ = history_store::persist_tab_history(
                    &tab_id,
                    source_path.as_deref(),
                    &mut self.tabs[index].history,
                    &snapshot,
                );
                session_store::persist_session(self);
                cx.notify();
            }
        } else {
            // No file path — open native save dialog in background
            let default_name = format!("{}.md", self.tabs[index].title);
            let title = self.t(crate::i18n::TextKey::SaveFileDialog).to_string();
            cx.spawn(async move |this, cx| {
                let maybe_path = cx
                    .background_executor()
                    .spawn(async move { helpers::save_file_blocking(&default_name, &title) })
                    .await;
                if let Some(path) = maybe_path
                    && std::fs::write(&path, &text).is_ok()
                {
                    this.update(cx, |editor, cx| {
                        if let Some(idx) = editor.active_tab_index() {
                            let tab_id = editor.tabs[idx].id.clone();
                            editor.tabs[idx].path = Some(path.clone());
                            editor.tabs[idx].title = EditorView::title_for_path(&path);
                            editor.tabs[idx].is_dirty = false;
                            editor.tabs[idx].snapshot = editor.current_snapshot();
                            let snapshot = editor.tabs[idx].snapshot.clone();
                            let source_path = editor.tabs[idx].path.clone();
                            editor.clear_temp_file_for_tab(&tab_id);
                            let _ = history_store::persist_tab_history(
                                &tab_id,
                                source_path.as_deref(),
                                &mut editor.tabs[idx].history,
                                &snapshot,
                            );
                            editor.current_file_path = Some(path);
                            session_store::persist_session(editor);
                            cx.notify();
                        }
                    })
                    .ok();
                }
            })
            .detach();
        }
    }

    fn save_active_whiteboard(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let Some(json) = self.tabs[index]
            .whiteboard
            .as_ref()
            .map(|state| state.document.to_json_string())
        else {
            return;
        };
        if let Some(path) = self.tabs[index].path.clone() {
            if std::fs::write(&path, &json).is_ok() {
                self.tabs[index].is_dirty = false;
                session_store::persist_session(self);
                cx.notify();
            }
        } else {
            let default_name = format!("{}.{}", self.tabs[index].title, WHITEBOARD_EXTENSION);
            let title = self.t(crate::i18n::TextKey::SaveFileDialog).to_string();
            cx.spawn(async move |this, cx| {
                let maybe_path = cx
                    .background_executor()
                    .spawn(async move { helpers::save_file_blocking(&default_name, &title) })
                    .await;
                if let Some(path) = maybe_path
                    && std::fs::write(&path, &json).is_ok()
                {
                    this.update(cx, |editor, cx| {
                        if let Some(idx) = editor.active_tab_index() {
                            editor.tabs[idx].path = Some(path.clone());
                            editor.tabs[idx].title = EditorView::title_for_path(&path);
                            editor.tabs[idx].is_dirty = false;
                            editor.current_file_path = Some(path);
                            session_store::persist_session(editor);
                            cx.notify();
                        }
                    })
                    .ok();
                }
            })
            .detach();
        }
    }

    pub(crate) fn on_new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        self.create_scratch_tab(cx);
    }

    pub(crate) fn on_new_whiteboard(
        &mut self,
        _: &NewWhiteboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Keep references alive while creation is temporarily disabled.
        let _ = self.t(crate::i18n::TextKey::NewWhiteboard);
        if false {
            let parent = self.sidebar_root.clone();
            self.create_new_whiteboard(parent, cx);
        }
    }

    pub(crate) fn on_close_active_tab(
        &mut self,
        _: &CloseActiveTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_active_tab(cx);
    }

    pub(crate) fn on_reopen_closed_tab(
        &mut self,
        _: &ReopenClosedTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reopen_closed_tab(cx);
    }

    pub(crate) fn on_activate_previous_tab(
        &mut self,
        _: &ActivatePreviousTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_previous_tab(cx);
    }

    pub(crate) fn on_activate_next_tab(
        &mut self,
        _: &ActivateNextTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_next_tab(cx);
    }

    // ── Multi-cursor actions ────────────────────────────────────────

    pub(crate) fn on_add_cursor_above(
        &mut self,
        _: &AddCursorAbove,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursors = self.all_cursors_snapshot();
        let mut added = false;
        for cursor in cursors {
            let (line, col) = self.buffer.offset_to_line_col(cursor.offset);
            if line == 0 {
                continue;
            }
            let new_offset = self.buffer.line_col_to_offset(line - 1, col);
            self.add_cursor_at(new_offset, new_offset);
            added = true;
        }
        if added {
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn on_add_cursor_below(
        &mut self,
        _: &AddCursorBelow,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursors = self.all_cursors_snapshot();
        let mut added = false;
        for cursor in cursors {
            let (line, col) = self.buffer.offset_to_line_col(cursor.offset);
            if line + 1 >= self.buffer.line_count() {
                continue;
            }
            let new_offset = self.buffer.line_col_to_offset(line + 1, col);
            self.add_cursor_at(new_offset, new_offset);
            added = true;
        }
        if added {
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn on_select_next_occurrence(
        &mut self,
        _: &SelectNextOccurrence,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search_text = if self.cursor.has_selection() {
            let range = self.cursor.selection_range();
            self.buffer.text()[range.clone()].to_string()
        } else {
            // Select the word under cursor first
            let offset = self.cursor.offset;
            self.select_word_at_offset(offset);
            if !self.cursor.has_selection() {
                return;
            }
            let range = self.cursor.selection_range();
            self.buffer.text()[range.clone()].to_string()
        };
        if search_text.is_empty() {
            return;
        }

        // Find the next occurrence after the last cursor
        let all = self.all_cursor_ranges();
        let last_end = all.iter().map(|(o, a)| (*o).max(*a)).max().unwrap_or(0);
        let text = self.buffer.text();
        let found = text[last_end..].find(&search_text).map(|i| last_end + i);
        let found = found.or_else(|| text[..last_end].find(&search_text));
        if let Some(start) = found {
            let end = start + search_text.len();
            // Don't add if any cursor already selects this exact range
            let already = self.all_cursor_ranges().iter().any(|(o, a)| {
                let s = (*o).min(*a);
                let e = (*o).max(*a);
                s == start && e == end
            });
            if !already {
                self.add_cursor_at(end, start);
            }
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_all_occurrences(
        &mut self,
        _: &SelectAllOccurrences,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search_text = if self.cursor.has_selection() {
            let range = self.cursor.selection_range();
            self.buffer.text()[range.clone()].to_string()
        } else {
            let offset = self.cursor.offset;
            self.select_word_at_offset(offset);
            if !self.cursor.has_selection() {
                return;
            }
            let range = self.cursor.selection_range();
            self.buffer.text()[range.clone()].to_string()
        };
        if search_text.is_empty() {
            return;
        }

        let text = self.buffer.text().to_string();
        let mut start = 0;
        let mut first = true;
        self.extra_cursors.clear();
        while let Some(pos) = text[start..].find(&search_text) {
            let abs_start = start + pos;
            let abs_end = abs_start + search_text.len();
            if first {
                self.cursor.anchor = abs_start;
                self.cursor.offset = abs_end;
                first = false;
            } else {
                self.extra_cursors.push(Cursor {
                    offset: abs_end,
                    anchor: abs_start,
                });
            }
            start = abs_end;
        }
        self.pause_blinking(cx);
    }
}
