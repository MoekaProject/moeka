use gpui::{Context, Window};

use crate::actions::*;
use crate::markdown::{self, BlockType};

use super::*;

impl EditorView {
    // ── Action handlers ─────────────────────────────────────────────

    pub(crate) fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn on_escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.settings_font_picker.is_some() {
            self.close_font_picker(cx);
            return;
        }
        if self.dialog.is_some() {
            self.dismiss_dialog(cx);
            return;
        }
        if self.search_visible {
            self.dismiss_search(cx);
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

    pub(crate) fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn on_backspace(
        &mut self,
        _: &Backspace,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();

        // Multi-cursor backspace
        if self.has_extra_cursors() {
            self.multi_cursor_backspace();
            self.reparse();
            self.scroll_to_cursor();
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
                self.buffer.replace(del_start..del_end, "");
                self.cursor.move_to(del_start, &self.buffer);
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }
        }
        self.cursor.backspace(&mut self.buffer);
        self.reparse();
        self.scroll_to_cursor();
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
        }
    }

    pub(crate) fn on_delete(&mut self, _: &Delete, _window: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            self.sidebar_delete_focused(cx);
            return;
        }
        self.checkpoint();
        if self.has_extra_cursors() {
            self.multi_cursor_delete();
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        self.cursor.delete(&mut self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_left(cx);
            return;
        }
        if self.has_extra_cursors() {
            self.multi_cursor_move_left();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
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
        self.maybe_auto_select_link_url(prev_offset, false);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_right(cx);
            return;
        }
        if self.has_extra_cursors() {
            self.multi_cursor_move_right();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
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
        self.maybe_auto_select_link_url(prev_offset, false);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_up(cx);
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
            self.multi_cursor_move_up();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        if let Some(offset) = self.offset_for_visual_up(self.cursor.offset) {
            self.cursor.move_to(offset, &self.buffer);
        } else {
            self.cursor.move_up(&self.buffer);
        }
        self.maybe_auto_select_link_url(prev_offset, false);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_down(cx);
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
            self.multi_cursor_move_down();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        if let Some(offset) = self.offset_for_visual_down(self.cursor.offset) {
            self.cursor.move_to(offset, &self.buffer);
        } else {
            self.cursor.move_down(&self.buffer);
        }
        self.maybe_auto_select_link_url(prev_offset, false);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_left(
        &mut self,
        _: &SelectLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_extra_cursors() {
            self.multi_cursor_select_left();
        } else {
            self.cursor.select_left(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_right(
        &mut self,
        _: &SelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_extra_cursors() {
            self.multi_cursor_select_right();
        } else {
            self.cursor.select_right(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_extra_cursors() {
            self.multi_cursor_select_up();
        } else if let Some(offset) = self.offset_for_visual_up(self.cursor.offset) {
            self.cursor.select_to(offset, &self.buffer);
        } else {
            self.cursor.select_up(&self.buffer);
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
            self.multi_cursor_select_down();
        } else if let Some(offset) = self.offset_for_visual_down(self.cursor.offset) {
            self.cursor.select_to(offset, &self.buffer);
        } else {
            self.cursor.select_down(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.select_all(&self.buffer);
        self.pause_blinking(cx);
    }

    pub(crate) fn on_home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_extra_cursors() {
            self.multi_cursor_move_to_line_start();
        } else {
            self.cursor.move_to_line_start(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_extra_cursors() {
            self.multi_cursor_move_to_line_end();
        } else {
            self.cursor.move_to_line_end(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
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
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_extra_cursors() {
            self.multi_cursor_select_to_line_end();
        } else {
            self.cursor.select_to_line_end(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
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
        if self.slash_menu_start.is_some() {
            self.execute_slash_menu_selected(cx);
            return;
        }
        if self.sidebar_edit.is_some() {
            self.commit_sidebar_edit(cx);
            return;
        }
        self.checkpoint();

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
                        self.buffer.replace(line_start..line_end, "");
                        self.cursor.move_to(line_start, &self.buffer);
                        self.cursor.insert(&mut self.buffer, "\n");
                    } else {
                        let marker = &trimmed[..marker_len];
                        let continuation = format!("\n{}{}", ws_str, marker);
                        self.cursor.insert(&mut self.buffer, &continuation);
                    }
                    self.reparse();
                    self.scroll_to_cursor();
                    self.pause_blinking(cx);
                    return;
                }
            }
            BlockType::TaskList(_) => {
                let prefix_len = task_prefix_len(trimmed);
                if prefix_len > 0 && self.cursor.offset >= line_start + leading_ws + prefix_len {
                    let text_after_marker = &line_content[leading_ws + prefix_len..];
                    if text_after_marker.trim().is_empty() {
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
                        self.cursor.insert(&mut self.buffer, &continuation);
                    }
                    self.reparse();
                    self.scroll_to_cursor();
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
                        self.buffer.replace(line_start..line_end, "");
                        self.cursor.move_to(line_start, &self.buffer);
                        self.cursor.insert(&mut self.buffer, "\n");
                    } else {
                        let continuation = format!("\n{}{}. ", ws_str, number + 1);
                        self.cursor.insert(&mut self.buffer, &continuation);
                        self.renumber_ordered_list_after(cursor_line + 2, number + 2, leading_ws);
                    }
                    self.reparse();
                    self.scroll_to_cursor();
                    self.pause_blinking(cx);
                    return;
                }
            }
            _ => {}
        }

        self.cursor.insert(&mut self.buffer, "\n");
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        let current_line = self.cursor_line();
        let should_indent_list = matches!(
            self.decorations.get(current_line).map(|d| &d.block_type),
            Some(BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_))
        ) || self.cursor.has_selection();

        if should_indent_list {
            self.indent_current_lines("  ");
        } else {
            self.cursor.insert(&mut self.buffer, "  ");
            self.reparse();
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection_to_clipboard(cx);
    }

    pub(crate) fn on_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        self.cut_selection_to_clipboard(cx);
    }

    pub(crate) fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
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
        self.cursor.move_word_left(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_word_right(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.cursor.delete_word_left(&mut self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    // ── Format actions ──────────────────────────────────────────────

    pub(crate) fn on_toggle_bold(
        &mut self,
        _: &ToggleBold,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.checkpoint();
        self.toggle_current_line_prefix("1. ");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_code_block(
        &mut self,
        _: &InsertCodeBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.apply_wrap("```\n", "\n```");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_horizontal_rule(
        &mut self,
        _: &InsertHorizontalRule,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.buffer.delete(delete_start..delete_end);
        self.cursor.offset = delete_start.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
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

    pub(crate) fn on_save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        self.tabs[index].snapshot = self.current_snapshot();
        let text = self.tabs[index].snapshot.text.clone();
        if let Some(ref path) = self.tabs[index].path.clone() {
            if std::fs::write(path, &text).is_ok() {
                self.tabs[index].is_dirty = false;
                cx.notify();
            }
        } else {
            // No file path — open native save dialog in background
            let default_name = format!("{}.md", self.tabs[index].title);
            cx.spawn(async move |this, cx| {
                let maybe_path = cx
                    .background_executor()
                    .spawn(async move { helpers::save_file_blocking(&default_name) })
                    .await;
                if let Some(path) = maybe_path {
                    if std::fs::write(&path, &text).is_ok() {
                        this.update(cx, |editor, cx| {
                            if let Some(idx) = editor.active_tab_index() {
                                editor.tabs[idx].path = Some(path.clone());
                                editor.tabs[idx].title = EditorView::title_for_path(&path);
                                editor.tabs[idx].is_dirty = false;
                                editor.current_file_path = Some(path);
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                }
            })
            .detach();
        }
    }

    pub(crate) fn on_new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        self.create_scratch_tab(cx);
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
        let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
        if line == 0 {
            return;
        }
        let new_offset = self.buffer.line_col_to_offset(line - 1, col);
        self.add_cursor_at(new_offset, new_offset);
        self.pause_blinking(cx);
    }

    pub(crate) fn on_add_cursor_below(
        &mut self,
        _: &AddCursorBelow,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
        if line + 1 >= self.buffer.line_count() {
            return;
        }
        let new_offset = self.buffer.line_col_to_offset(line + 1, col);
        self.add_cursor_at(new_offset, new_offset);
        self.pause_blinking(cx);
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
        let last_end = all
            .iter()
            .map(|(o, a)| (*o).max(*a))
            .max()
            .unwrap_or(0);
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
