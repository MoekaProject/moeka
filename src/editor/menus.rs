use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, Pixels, Point, SharedString, div,
    prelude::*, px, svg,
};

use crate::markdown::{BlockType, InlineKind};

use super::shortcut::{ShortcutRenderStyle, render_shortcut};
use super::*;

#[derive(Clone)]
struct ImageMenuTarget {
    line: usize,
    full_range: Range<usize>,
    desc_range: Range<usize>,
    url_range: Range<usize>,
}

impl EditorView {
    fn current_heading_action(&self) -> Option<FormatMenuAction> {
        match self.decorations.get(self.cursor_line())?.block_type {
            BlockType::Heading(1) => Some(FormatMenuAction::Heading1),
            BlockType::Heading(2) => Some(FormatMenuAction::Heading2),
            BlockType::Heading(3) => Some(FormatMenuAction::Heading3),
            BlockType::Heading(4) => Some(FormatMenuAction::Heading4),
            BlockType::Heading(5) => Some(FormatMenuAction::Heading5),
            BlockType::Heading(6) => Some(FormatMenuAction::Heading6),
            _ => None,
        }
    }

    fn current_image_menu_target(&self) -> Option<ImageMenuTarget> {
        let line = self.cursor_line();
        let decoration = self.decorations.get(line)?;
        let span = decoration
            .inline_spans
            .iter()
            .find(|span| span.kind == InlineKind::Image)?;
        let url_range = decoration.syntax_ranges.iter().find(|range| {
            range.range.start == span.range.end && range.range.end > range.range.start + 1
        })?;
        let line_start = self.buffer.line_start(line);
        let line_content = self.buffer.line_content(line);
        let full_start = line_start + span.range.start.saturating_sub(2);
        let full_end = line_start + url_range.range.end.min(line_content.len());
        let selection = self.cursor.selection_range();
        let cursor_offset = self.cursor.offset;
        let selected_image = selection.start == full_start && selection.end == full_end;
        let cursor_in_image = cursor_offset >= full_start && cursor_offset <= full_end;
        if !selected_image && !cursor_in_image {
            return None;
        }

        Some(ImageMenuTarget {
            line,
            full_range: full_start..full_end,
            desc_range: line_start + span.range.start..line_start + span.range.end,
            url_range: line_start + url_range.range.start + 2
                ..line_start + url_range.range.end.saturating_sub(1),
        })
    }

    fn set_selection_range(&mut self, range: Range<usize>) {
        self.cursor.anchor = range.start.min(self.buffer.len());
        self.cursor.offset = range.end.min(self.buffer.len());
        self.scroll_to_cursor();
    }

    fn copy_text_to_clipboard(&mut self, text: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        cx.notify();
    }

    fn path_copy_text(&self, path: &Path, relative: bool) -> Option<String> {
        if relative {
            self.relative_path_for_path(path)
        } else {
            Some(path.display().to_string())
        }
    }

    fn sidebar_context_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = if self.selected_sidebar_paths.contains(path) {
            self.selected_sidebar_paths.iter().cloned().collect()
        } else {
            vec![path.to_path_buf()]
        };
        paths.sort();
        paths
    }

    fn sidebar_paths_copy_text(&self, path: &Path, relative: bool) -> Option<String> {
        let mut lines = Vec::new();
        for item_path in self.sidebar_context_paths(path) {
            lines.push(self.path_copy_text(&item_path, relative)?);
        }
        Some(lines.join("\n"))
    }

    fn replace_image_url_on_line_with_text(
        &mut self,
        line: usize,
        new_url: &str,
        cx: &mut Context<Self>,
    ) {
        let line_content = self.buffer.line_content(line).to_string();
        let line_start = self.buffer.line_start(line);
        if let Some(bracket_pos) = line_content.find("](") {
            let url_start = bracket_pos + 2;
            if let Some(close_pos) = line_content[url_start..].find(')') {
                let url_end = url_start + close_pos;
                self.checkpoint();
                self.buffer
                    .replace(line_start + url_start..line_start + url_end, new_url);
                self.reparse();
                self.pause_blinking(cx);
                self.scroll_to_cursor();
            }
        }
    }

    fn replace_image_from_picker(
        &mut self,
        line: usize,
        source_path: &Path,
        document_path: &Path,
        cx: &mut Context<Self>,
    ) {
        let Some(document_dir) = document_path.parent() else {
            return;
        };
        let Some(file_name) = source_path.file_name() else {
            return;
        };

        let destination_path = document_dir.join(file_name);
        if source_path != destination_path {
            if let Err(error) = std::fs::copy(source_path, &destination_path) {
                eprintln!("failed to copy image into document directory: {error}");
                return;
            }
        }

        let relative_path = file_name.to_string_lossy().to_string();
        self.replace_image_url_on_line_with_text(line, &relative_path, cx);
    }

    fn pick_image_for_current_markdown_image(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.current_image_menu_target() else {
            return;
        };
        let Some(document_path) = self.current_file_path.clone() else {
            eprintln!("select image requires a saved markdown file");
            return;
        };

        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_image_file_blocking() })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    editor.replace_image_from_picker(target.line, &path, &document_path, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn clear_wiki_link_menu_state(&mut self) {
        self.wiki_link_start = None;
        self.wiki_link_query.clear();
        self.wiki_link_recent_task = None;
        self.wiki_link_search_task = None;
        self.wiki_link_search_results.clear();
    }

    fn wiki_link_query_end(&self, start: usize) -> Option<usize> {
        let text = self.buffer.text();
        if text.get(start..start + 2) != Some("[[") {
            return None;
        }
        let suffix = text.get(start + 2..)?;
        suffix.find("]]").map(|index| start + 2 + index)
    }

    pub(crate) fn wiki_link_result_count(&self) -> usize {
        if self.wiki_link_query.trim().is_empty() {
            self.wiki_link_recent_results.len()
        } else {
            self.wiki_link_search_results.len()
        }
    }

    fn selected_wiki_link_relative_path(&self) -> Option<String> {
        let path = if self.wiki_link_query.trim().is_empty() {
            self.wiki_link_recent_results
                .get(self.menu_selected_index)
                .map(|candidate| candidate.path.clone())
        } else {
            self.wiki_link_search_results
                .get(self.menu_selected_index)
                .map(|result| result.path.clone())
        }?;

        Some(
            path.strip_prefix(&self.sidebar_root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/"),
        )
    }

    /// Detect which format action is "active" at the cursor position.
    /// Uses the markdown parser's decoration data for accurate detection.
    /// Inline syntax has priority over block-level syntax.
    /// Returns at most one active action.
    fn active_format_actions(&self) -> Vec<FormatMenuAction> {
        let cursor_line = self.cursor_line();
        let decoration = match self.decorations.get(cursor_line) {
            Some(d) => d,
            None => return Vec::new(),
        };

        let line_start = self.buffer.line_start(cursor_line);
        let cursor_col = self.cursor.offset.saturating_sub(line_start);

        // Check inline spans (including markers) — pick the smallest (innermost) match
        let mut best_inline: Option<(usize, FormatMenuAction)> = None;
        for span in &decoration.inline_spans {
            let marker_len = match &span.kind {
                InlineKind::Bold => 2,
                InlineKind::Italic => 1,
                InlineKind::Strikethrough => 2,
                InlineKind::InlineCode => 1,
                InlineKind::Link => 0,
                InlineKind::Image => 0,
            };
            let full_start = span.range.start.saturating_sub(marker_len);
            let full_end = span.range.end + marker_len;
            if cursor_col >= full_start && cursor_col <= full_end {
                let action = match &span.kind {
                    InlineKind::Bold => Some(FormatMenuAction::Bold),
                    InlineKind::Italic => Some(FormatMenuAction::Italic),
                    InlineKind::Strikethrough => Some(FormatMenuAction::Strikethrough),
                    InlineKind::InlineCode => Some(FormatMenuAction::InlineCode),
                    InlineKind::Link => Some(FormatMenuAction::Link),
                    InlineKind::Image => Some(FormatMenuAction::Image),
                };
                if let Some(action) = action {
                    let span_len = full_end - full_start;
                    if best_inline
                        .as_ref()
                        .is_none_or(|(prev_len, _)| span_len < *prev_len)
                    {
                        best_inline = Some((span_len, action));
                    }
                }
            }
        }

        if let Some((_, action)) = best_inline {
            return vec![action];
        }

        // Fallback to block-level syntax
        let block_action = match &decoration.block_type {
            BlockType::Heading(1) => Some(FormatMenuAction::Heading1),
            BlockType::Heading(2) => Some(FormatMenuAction::Heading2),
            BlockType::Heading(3) => Some(FormatMenuAction::Heading3),
            BlockType::Heading(4) => Some(FormatMenuAction::Heading4),
            BlockType::Heading(5) => Some(FormatMenuAction::Heading5),
            BlockType::Heading(6) => Some(FormatMenuAction::Heading6),
            BlockType::Blockquote => Some(FormatMenuAction::Quote),
            BlockType::UnorderedList => Some(FormatMenuAction::UnorderedList),
            BlockType::OrderedList => Some(FormatMenuAction::OrderedList),
            BlockType::TaskList(_) => Some(FormatMenuAction::Todo),
            BlockType::CodeBlock | BlockType::CodeBlockFence => Some(FormatMenuAction::CodeBlock),
            BlockType::HorizontalRule => Some(FormatMenuAction::HorizontalRule),
            _ => None,
        };

        block_action.into_iter().collect()
    }
    pub(crate) fn open_sidebar_context_menu(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Sidebar { path, is_dir },
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    pub(crate) fn open_format_context_menu(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Format,
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    fn screen_position_for_offset(&self, offset: usize) -> Point<Pixels> {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let origin_y: f32 = self.content_origin.y.into();
        let y = origin_y + self.line_y_offset(line) - self.scroll_offset;
        let line_height = self.visual_height_for_line(line);
        let x = if let Some(Some(info)) = self.line_display_info.get(line) {
            let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
            let pos = info
                .wrapped_line
                .position_for_index(disp_col, gpui::px(info.line_height))
                .unwrap_or_default();
            info.content_x + f32::from(pos.x)
        } else {
            self.last_content_left + col as f32 * self.base_font_size() * 0.6
        };
        gpui::point(px(x), px(y + line_height))
    }

    pub(crate) fn cursor_screen_position(&self) -> Point<Pixels> {
        self.screen_position_for_offset(self.cursor.offset)
    }

    fn wiki_link_menu_position(&self, start: usize) -> Point<Pixels> {
        let start_position = self.screen_position_for_offset(start);
        let cursor_position = self.cursor_screen_position();
        gpui::point(start_position.x, cursor_position.y)
    }

    pub(crate) fn open_slash_menu(&mut self, cx: &mut Context<Self>) {
        self.slash_menu_start = Some(self.cursor.offset - 1);
        self.slash_filter = String::new();
        self.menu_selected_index = 0;
        self.heading_submenu_open = false;
        let pos = self.cursor_screen_position();
        self.open_format_context_menu(pos, cx);
    }

    pub(crate) fn open_wiki_link_menu(&mut self, start: usize, cx: &mut Context<Self>) {
        self.wiki_link_start = Some(start);
        self.wiki_link_query.clear();
        self.wiki_link_search_results.clear();
        self.menu_selected_index = 0;
        let pos = self.wiki_link_menu_position(start);
        self.context_menu = Some(ContextMenu {
            position: pos,
            kind: ContextMenuKind::WikiLink,
        });
        self.run_wiki_link_recent_search(cx);
        cx.notify();
    }

    pub(crate) fn update_slash_filter(&mut self, cx: &mut Context<Self>) {
        if let Some(start) = self.slash_menu_start {
            self.menu_selected_index = 0;
            let end = self.cursor.offset;
            if end > start + 1 {
                let text = self.buffer.text();
                let filter_start = start + 1;
                if filter_start <= end && end <= text.len() {
                    self.slash_filter = text[filter_start..end].to_string();
                }
            } else {
                self.slash_filter.clear();
            }
            let pos = self.cursor_screen_position();
            if let Some(menu) = &mut self.context_menu {
                menu.position = pos;
            }
            cx.notify();
        }
    }

    pub(crate) fn update_wiki_link_query(&mut self, cx: &mut Context<Self>) {
        if let Some(start) = self.wiki_link_start {
            self.menu_selected_index = 0;
            let end = self.cursor.offset;
            let query_start = start + 2;
            if query_start <= end && end <= self.buffer.len() {
                let text = self.buffer.text();
                self.wiki_link_query = text[query_start..end].to_string();
            } else {
                self.wiki_link_query.clear();
            }

            let pos = self.wiki_link_menu_position(start);
            if let Some(menu) = &mut self.context_menu {
                menu.position = pos;
            }

            if self.wiki_link_query.trim().is_empty() {
                self.run_wiki_link_recent_search(cx);
            } else {
                self.run_wiki_link_search(cx);
            }
            cx.notify();
        }
    }

    pub(crate) fn run_wiki_link_recent_search(&mut self, cx: &mut Context<Self>) {
        self.wiki_link_recent_task = None;
        self.wiki_link_search_task = None;
        self.wiki_link_search_results.clear();

        let root = self.sidebar_root.clone();
        if !root.exists() {
            self.wiki_link_recent_results.clear();
            cx.notify();
            return;
        }

        let task = cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    crate::editor::quick_open::collect_wiki_link_recent_files(&root, 10)
                })
                .await;

            this.update(cx, |this, cx| {
                this.wiki_link_recent_results = results;
                this.wiki_link_recent_task = None;
                cx.notify();
            })
            .ok();
        });
        self.wiki_link_recent_task = Some(task);
    }

    pub(crate) fn run_wiki_link_search(&mut self, cx: &mut Context<Self>) {
        self.wiki_link_search_task = None;

        let root = self.sidebar_root.clone();
        let query = self.wiki_link_query.clone();
        if !root.exists() || query.trim().is_empty() {
            self.wiki_link_search_results.clear();
            cx.notify();
            return;
        }

        let task = cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    crate::editor::quick_open::collect_wiki_link_search_results(&root, &query, 10)
                })
                .await;

            this.update(cx, |this, cx| {
                this.wiki_link_search_results = results;
                this.wiki_link_search_task = None;
                cx.notify();
            })
            .ok();
        });
        self.wiki_link_search_task = Some(task);
    }

    pub(crate) fn open_tab_context_menu(
        &mut self,
        tab_id: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Tab { tab_id },
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    pub(crate) fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            if let Some(start) = self.slash_menu_start.take() {
                let end = self.cursor.offset;
                if end > start {
                    self.buffer.replace(start..end, "");
                    self.cursor.move_to(start, &self.buffer);
                    self.reparse();
                }
                self.slash_filter.clear();
            }
            self.clear_wiki_link_menu_state();
            cx.notify();
        }
    }

    pub(crate) fn sync_wiki_link_menu(&mut self, cx: &mut Context<Self>) {
        let Some(start) = self.wiki_link_start else {
            return;
        };
        let Some(query_end) = self.wiki_link_query_end(start) else {
            self.close_context_menu(cx);
            return;
        };
        let query_start = start + 2;
        if self.cursor.offset < query_start || self.cursor.offset > query_end {
            self.close_context_menu(cx);
            return;
        }
        self.update_wiki_link_query(cx);
    }

    pub(crate) fn confirm_wiki_link_selection(&mut self, cx: &mut Context<Self>) {
        let Some(start) = self.wiki_link_start else {
            return;
        };
        let Some(query_end) = self.wiki_link_query_end(start) else {
            self.close_context_menu(cx);
            return;
        };
        let Some(relative_path) = self.selected_wiki_link_relative_path() else {
            return;
        };

        let replace_start = start + 2;
        self.buffer
            .replace(replace_start..query_end, &relative_path);
        self.cursor
            .move_to(replace_start + relative_path.len(), &self.buffer);
        self.reparse();
        self.close_context_menu(cx);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn execute_format_menu_action(
        &mut self,
        action: FormatMenuAction,
        cx: &mut Context<Self>,
    ) {
        if let Some(start) = self.slash_menu_start.take() {
            let end = self.cursor.offset;
            if end > start {
                self.buffer.replace(start..end, "");
                self.cursor.move_to(start, &self.buffer);
                self.reparse();
            }
            self.slash_filter.clear();
        }
        self.context_menu = None;
        match action {
            FormatMenuAction::Cut => self.cut_selection_to_clipboard(cx),
            FormatMenuAction::Copy => self.copy_selection_to_clipboard(cx),
            FormatMenuAction::Paste => self.paste_from_clipboard(cx),
            FormatMenuAction::Heading1 => {
                self.checkpoint();
                self.apply_heading(1);
                self.pause_blinking(cx);
            }
            FormatMenuAction::Heading2 => {
                self.checkpoint();
                self.apply_heading(2);
                self.pause_blinking(cx);
            }
            FormatMenuAction::Heading3 => {
                self.checkpoint();
                self.apply_heading(3);
                self.pause_blinking(cx);
            }
            FormatMenuAction::Heading4 => {
                self.checkpoint();
                self.apply_heading(4);
                self.pause_blinking(cx);
            }
            FormatMenuAction::Heading5 => {
                self.checkpoint();
                self.apply_heading(5);
                self.pause_blinking(cx);
            }
            FormatMenuAction::Heading6 => {
                self.checkpoint();
                self.apply_heading(6);
                self.pause_blinking(cx);
            }
            FormatMenuAction::Paragraph => {
                if let Some(current_heading) = self.current_heading_action() {
                    self.checkpoint();
                    match current_heading {
                        FormatMenuAction::Heading1 => self.apply_heading(1),
                        FormatMenuAction::Heading2 => self.apply_heading(2),
                        FormatMenuAction::Heading3 => self.apply_heading(3),
                        FormatMenuAction::Heading4 => self.apply_heading(4),
                        FormatMenuAction::Heading5 => self.apply_heading(5),
                        FormatMenuAction::Heading6 => self.apply_heading(6),
                        _ => {}
                    }
                    self.pause_blinking(cx);
                }
            }
            FormatMenuAction::Bold => {
                self.checkpoint();
                self.apply_wrap("**", "**");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Italic => {
                self.checkpoint();
                self.apply_wrap("*", "*");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Strikethrough => {
                self.checkpoint();
                self.apply_wrap("~~", "~~");
                self.pause_blinking(cx);
            }
            FormatMenuAction::InlineCode => {
                self.checkpoint();
                self.apply_wrap("`", "`");
                self.pause_blinking(cx);
            }
            FormatMenuAction::CodeBlock => {
                self.checkpoint();
                self.apply_wrap("```\n", "\n```");
                self.pause_blinking(cx);
            }
            FormatMenuAction::HorizontalRule => {
                self.checkpoint();
                self.toggle_horizontal_rule_at_cursor();
                self.pause_blinking(cx);
            }
            FormatMenuAction::Quote => {
                self.checkpoint();
                self.toggle_current_line_prefix("> ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::UnorderedList => {
                self.checkpoint();
                self.toggle_current_line_prefix("- ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::OrderedList => {
                self.checkpoint();
                self.toggle_current_line_prefix("1. ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Todo => {
                self.checkpoint();
                self.toggle_current_line_prefix("- [ ] ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Link => {
                self.checkpoint();
                self.insert_link_or_image("[", "](https://)", "title");
                self.pause_blinking(cx);
            }
            FormatMenuAction::WikiLink => {
                self.checkpoint();
                let range = self.cursor.selection_range();
                let selected_text = self.buffer.text()[range.clone()].to_string();
                let replacement = format!("[[{}]]", selected_text);
                self.buffer.replace(range.clone(), &replacement);
                let cursor_offset = range.start + 2 + selected_text.len();
                self.cursor.move_to(cursor_offset, &self.buffer);
                self.reparse();
                self.open_wiki_link_menu(range.start, cx);
                if !selected_text.is_empty() {
                    self.update_wiki_link_query(cx);
                }
                self.scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }
            FormatMenuAction::Image => {
                self.checkpoint();
                self.insert_link_or_image("![", "](image.png)", "alt");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Table => {
                self.checkpoint();
                let range = self.cursor.selection_range();
                let snippet =
                    "| 列1 | 列2 |\n| --- | --- |\n| 单元格1 | 单元格2 |\n| 单元格3 | 单元格4 |";
                self.buffer.replace(range.clone(), snippet);
                let select_start = range.start + 2;
                let select_end = select_start + "列1".len();
                self.cursor.anchor = select_start;
                self.cursor.offset = select_end;
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
            }
            FormatMenuAction::EditImageDescription => {
                if let Some(target) = self.current_image_menu_target() {
                    self.set_selection_range(target.desc_range);
                    self.pause_blinking(cx);
                }
            }
            FormatMenuAction::EditImageUrl => {
                if let Some(target) = self.current_image_menu_target() {
                    self.set_selection_range(target.url_range);
                    self.pause_blinking(cx);
                }
            }
            FormatMenuAction::DeleteImage => {
                if let Some(target) = self.current_image_menu_target() {
                    self.checkpoint();
                    self.buffer.replace(target.full_range.clone(), "");
                    self.cursor.move_to(target.full_range.start, &self.buffer);
                    self.reparse();
                    self.scroll_to_cursor();
                    self.pause_blinking(cx);
                }
            }
            FormatMenuAction::SelectImageFile => {
                self.pick_image_for_current_markdown_image(cx);
            }
        }
        self.close_context_menu(cx);
    }

    pub(crate) fn slash_menu_matched_actions(&self) -> Vec<FormatMenuAction> {
        let filter = self.slash_filter.to_lowercase();
        let matches_filter = |label: &str, filter_text: &str| -> bool {
            if filter.is_empty() {
                return true;
            }
            label.to_lowercase().contains(&filter) || filter_text.to_lowercase().contains(&filter)
        };

        let mut actions = Vec::new();
        let all_items: &[(&str, &str, FormatMenuAction)] = &[
            ("待办事项", "todo task checkbox", FormatMenuAction::Todo),
            ("代码块", "code block", FormatMenuAction::CodeBlock),
            ("表格", "table grid 2x2", FormatMenuAction::Table),
            ("链接", "link url", FormatMenuAction::Link),
            (
                "双向链接",
                "wiki link backlink bidirectional",
                FormatMenuAction::WikiLink,
            ),
            ("图片", "image photo", FormatMenuAction::Image),
        ];
        for &(label, ft, action) in all_items {
            if matches_filter(label, ft) {
                actions.push(action);
            }
        }
        actions
    }

    pub(crate) fn execute_slash_menu_selected(&mut self, cx: &mut Context<Self>) {
        let actions = self.slash_menu_matched_actions();
        let index = self
            .menu_selected_index
            .min(actions.len().saturating_sub(1));
        if let Some(&action) = actions.get(index) {
            self.execute_format_menu_action(action, cx);
        }
    }

    // ── Context menu rendering ──────────────────────────────────────

    fn render_context_menu_item(
        &self,
        label: &str,
        is_destructive: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let text_color = if is_destructive {
            ms.menu_item_danger_text_color
        } else {
            ms.menu_item_text_color
        };
        let hover_bg = ms.menu_item_hover_bg;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        div()
            .id(item_id)
            .w_full()
            .h(px(ms.menu_item_height_px))
            .px(px(ms.menu_item_padding_x_px))
            .flex()
            .items_center()
            .text_size(px(ms.menu_item_font_size_px))
            .text_color(text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(label.to_string())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_context_menu_item_with_icon(
        &self,
        label: &str,
        icon_path: &'static str,
        is_destructive: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let wss = &self.styles.workspace_switcher;
        let text_color = if is_destructive {
            ms.menu_item_danger_text_color
        } else {
            ms.menu_item_text_color
        };
        let hover_bg = ms.menu_item_hover_bg;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        let icon_size = self.scaled(wss.ws_menu_item_action_icon_size_px);
        let check_size = self.scaled(wss.ws_menu_item_check_size_px);
        let icon_col_w = check_size.max(icon_size);
        div()
            .id(item_id)
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .px(px(self.scaled(ms.menu_item_padding_x_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(wss.ws_menu_item_action_icon_gap_px)))
            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
            .text_color(text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .w(px(icon_col_w))
                    .h(px(icon_col_w))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(icon_size))
                            .text_color(wss.ws_menu_item_action_icon_color),
                    ),
            )
            .child(label.to_string())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_context_menu_choice_item(
        &self,
        label: &str,
        is_active: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let wss = &self.styles.workspace_switcher;
        let hover_bg = ms.menu_item_hover_bg;
        let icon_size = self.scaled(wss.ws_menu_item_check_size_px);
        let gap = self.scaled(wss.ws_menu_item_action_icon_gap_px);

        div()
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .px(px(self.scaled(ms.menu_item_padding_x_px)))
            .flex()
            .items_center()
            .gap(px(gap))
            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
            .text_color(ms.menu_item_text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .w(px(icon_size))
                    .h(px(icon_size))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .when(is_active, |this| {
                        this.child(
                            svg()
                                .path(CHECK_SVG_PATH)
                                .size(px(icon_size))
                                .text_color(wss.ws_menu_item_check_color),
                        )
                    }),
            )
            .child(label.to_string())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_format_context_menu_item(
        &self,
        label: &str,
        icon_path: Option<&str>,
        shortcut: Option<&str>,
        is_selected: bool,
        is_active: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let bg = if is_selected {
            ms.menu_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let hover_bg = ms.menu_item_hover_bg;
        let text_color = if is_active {
            ms.menu_format_active_text_color
        } else {
            ms.menu_item_text_color
        };
        let icon_color = if is_active {
            ms.menu_format_active_text_color
        } else {
            ms.menu_format_icon_color
        };
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        let icon_gap = self.scaled(ms.menu_format_icon_gap_px);
        let icon_shared: Option<SharedString> =
            icon_path.map(|p| SharedString::from(p.to_string()));
        let shortcut_style = ShortcutRenderStyle {
            font_size_px: self.scaled(ms.menu_shortcut_font_size_px),
            text_color: ms.menu_shortcut_text_color,
            gap_px: self.scaled(ms.menu_shortcut_gap_px),
            key_min_width_px: self.scaled(ms.menu_shortcut_key_min_width_px),
            key_height_px: self.scaled(ms.menu_shortcut_key_height_px),
            key_padding_x_px: self.scaled(ms.menu_shortcut_key_padding_x_px),
            key_radius_px: self.scaled(ms.menu_shortcut_key_radius_px),
            key_bg: ms.menu_shortcut_key_bg,
        };
        div()
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .bg(bg)
            .px(px(self.scaled(ms.menu_item_padding_x_px)))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
            .text_color(text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(icon_gap))
                    .child(
                        div()
                            .w(px(icon_size))
                            .h(px(icon_size))
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_none()
                            .when_some(icon_shared, |this, path| {
                                this.child(
                                    svg().path(path).size(px(icon_size)).text_color(icon_color),
                                )
                            }),
                    )
                    .child(label.to_string()),
            )
            .when_some(shortcut, |this, shortcut| {
                this.child(div().child(render_shortcut(shortcut, shortcut_style)))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_context_menu_separator(&self) -> AnyElement {
        let ms = &self.styles.context_menu;
        div()
            .w_full()
            .h(px(ms.menu_separator_height_px))
            .my(px(ms.menu_separator_margin_y_px))
            .bg(ms.menu_separator_color)
            .into_any_element()
    }

    fn render_sidebar_context_menu(
        &self,
        path: PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let has_clipboard = self.file_clipboard.is_some();
        let is_root = is_dir && path == self.sidebar_root;
        let parent_dir = if is_dir {
            path.clone()
        } else {
            path.parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| path.clone())
        };

        let mut items: Vec<AnyElement> = Vec::new();

        if is_root {
            {
                let pd = parent_dir.clone();
                items.push(self.render_context_menu_item(
                    "新建文件",
                    false,
                    cx,
                    move |this, window, cx| {
                        this.start_create_file(pd.clone(), window, cx);
                    },
                ));
            }
            {
                let pd = parent_dir.clone();
                items.push(self.render_context_menu_item(
                    "新建目录",
                    false,
                    cx,
                    move |this, window, cx| {
                        this.start_create_dir(pd.clone(), window, cx);
                    },
                ));
            }
            items.push(self.render_context_menu_separator());
            if !self.expanded_dirs.is_empty() {
                items.push(self.render_context_menu_item(
                    "折叠所有目录",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.collapse_all_dirs(cx);
                    },
                ));
            }
            items.push(self.render_context_menu_item(
                "展开所有目录",
                false,
                cx,
                move |this, _, cx| {
                    this.expand_all_dirs(cx);
                },
            ));
            items.push(self.render_context_menu_separator());
            if let Some(copy_path_text) = self.sidebar_paths_copy_text(&path, false) {
                items.push(self.render_context_menu_item(
                    "复制路径",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.copy_text_to_clipboard(copy_path_text.clone(), cx);
                    },
                ));
            }
            if let Some(copy_relative_path_text) = self.sidebar_paths_copy_text(&path, true) {
                items.push(self.render_context_menu_item(
                    "复制相对路径",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.copy_text_to_clipboard(copy_relative_path_text.clone(), cx);
                    },
                ));
            }
            items.push(self.render_context_menu_separator());
            {
                let p = path.clone();
                items.push(self.render_context_menu_item(
                    "在访达中显示",
                    false,
                    cx,
                    move |this, _, _cx| {
                        this.reveal_in_finder(&p);
                    },
                ));
            }
            return items;
        }

        {
            let pd = parent_dir.clone();
            items.push(self.render_context_menu_item(
                "新建文件",
                false,
                cx,
                move |this, window, cx| {
                    this.start_create_file(pd.clone(), window, cx);
                },
            ));
        }
        {
            let pd = parent_dir.clone();
            items.push(self.render_context_menu_item(
                "新建目录",
                false,
                cx,
                move |this, window, cx| {
                    this.start_create_dir(pd.clone(), window, cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        // Rename: only for single selection
        if self.selected_sidebar_paths.len() <= 1 {
            let p = path.clone();
            items.push(self.render_context_menu_item(
                "重命名",
                false,
                cx,
                move |this, window, cx| {
                    this.start_rename(p.clone(), window, cx);
                },
            ));
        }
        {
            let selected: Vec<std::path::PathBuf> =
                self.selected_sidebar_paths.iter().cloned().collect();
            let count = selected.len();
            let delete_label = if count > 1 {
                format!("删除 ({count} 项)")
            } else {
                "删除".to_string()
            };
            items.push(self.render_context_menu_item(
                &delete_label,
                true,
                cx,
                move |this, _, cx| {
                    if selected.len() == 1 {
                        this.delete_path(selected[0].clone(), cx);
                    } else {
                        // Delete all selected paths
                        for p in &selected {
                            let result = if p.is_dir() {
                                std::fs::remove_dir_all(p)
                            } else {
                                std::fs::remove_file(p)
                            };
                            if let Err(e) = result {
                                eprintln!("Delete failed: {e}");
                            }
                        }
                        this.tabs.retain(|tab| {
                            tab.path
                                .as_ref()
                                .is_none_or(|tp| !selected.iter().any(|sp| tp.starts_with(sp)))
                        });
                        if this.tabs.is_empty() {
                            this.create_scratch_tab(cx);
                            return;
                        }
                        let active_still_exists = this
                            .active_tab_id
                            .as_ref()
                            .is_some_and(|id| this.tabs.iter().any(|t| &t.id == id));
                        if !active_still_exists {
                            this.activate_tab_by_index(0, cx);
                        }
                        this.selected_sidebar_paths.clear();
                        this.context_menu = None;
                        cx.notify();
                    }
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        {
            let selected: Vec<std::path::PathBuf> =
                self.selected_sidebar_paths.iter().cloned().collect();
            let paths = if selected.is_empty() {
                vec![path.clone()]
            } else {
                selected
            };
            let cut_paths = paths.clone();
            items.push(
                self.render_context_menu_item("剪切", false, cx, move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: cut_paths.clone(),
                        is_cut: true,
                    });
                    this.context_menu = None;
                    cx.notify();
                }),
            );
            items.push(
                self.render_context_menu_item("复制", false, cx, move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: paths.clone(),
                        is_cut: false,
                    });
                    this.context_menu = None;
                    cx.notify();
                }),
            );
        }
        if has_clipboard {
            let pd = parent_dir.clone();
            items.push(
                self.render_context_menu_item("粘贴", false, cx, move |this, _, cx| {
                    this.paste_into_dir(pd.clone(), cx);
                }),
            );
        }
        items.push(self.render_context_menu_separator());
        if let Some(copy_path_text) = self.sidebar_paths_copy_text(&path, false) {
            items.push(self.render_context_menu_item(
                "复制路径",
                false,
                cx,
                move |this, _, cx| {
                    this.copy_text_to_clipboard(copy_path_text.clone(), cx);
                },
            ));
        }
        if let Some(copy_relative_path_text) = self.sidebar_paths_copy_text(&path, true) {
            items.push(self.render_context_menu_item(
                "复制相对路径",
                false,
                cx,
                move |this, _, cx| {
                    this.copy_text_to_clipboard(copy_relative_path_text.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        // Collapse / Expand all
        if !self.expanded_dirs.is_empty() {
            items.push(self.render_context_menu_item(
                "折叠所有目录",
                false,
                cx,
                move |this, _, cx| {
                    this.collapse_all_dirs(cx);
                },
            ));
        }
        {
            items.push(self.render_context_menu_item(
                "展开所有目录",
                false,
                cx,
                move |this, _, cx| {
                    this.expand_all_dirs(cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        {
            let p = path.clone();
            items.push(self.render_context_menu_item(
                "在访达中显示",
                false,
                cx,
                move |this, _, _cx| {
                    this.reveal_in_finder(&p);
                },
            ));
        }

        items
    }

    fn render_heading_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let active_actions = self.active_format_actions();
        let heading_items: &[(&str, Option<&str>, FormatMenuAction)] = &[
            (
                "标题1",
                Some(HEADING_1_SVG_PATH),
                FormatMenuAction::Heading1,
            ),
            (
                "标题2",
                Some(HEADING_2_SVG_PATH),
                FormatMenuAction::Heading2,
            ),
            (
                "标题3",
                Some(HEADING_3_SVG_PATH),
                FormatMenuAction::Heading3,
            ),
            (
                "标题4",
                Some(HEADING_4_SVG_PATH),
                FormatMenuAction::Heading4,
            ),
            (
                "标题5",
                Some(HEADING_5_SVG_PATH),
                FormatMenuAction::Heading5,
            ),
            (
                "标题6",
                Some(HEADING_6_SVG_PATH),
                FormatMenuAction::Heading6,
            ),
            ("正文", None, FormatMenuAction::Paragraph),
        ];

        heading_items
            .iter()
            .map(|&(label, icon, action)| {
                let is_active = if action == FormatMenuAction::Paragraph {
                    self.current_heading_action().is_none()
                } else {
                    active_actions.contains(&action)
                };
                self.render_format_context_menu_item(
                    label,
                    icon,
                    None,
                    false,
                    is_active,
                    cx,
                    move |this, _, cx| {
                        this.execute_format_menu_action(action, cx);
                    },
                )
            })
            .collect()
    }

    fn render_image_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        [
            (
                "修改描述",
                None,
                FormatMenuAction::EditImageDescription,
                false,
            ),
            ("修改 URL", None, FormatMenuAction::EditImageUrl, false),
            ("删除", None, FormatMenuAction::DeleteImage, true),
            (
                "选择图片...",
                None,
                FormatMenuAction::SelectImageFile,
                false,
            ),
        ]
        .into_iter()
        .map(|(label, icon, action, _is_destructive)| {
            self.render_format_context_menu_item(
                label,
                icon,
                None,
                false,
                false,
                cx,
                move |this, _, cx| {
                    this.execute_format_menu_action(action, cx);
                },
            )
        })
        .collect()
    }

    fn render_format_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.current_image_menu_target().is_some() {
            return self.render_image_context_menu(cx);
        }

        if self.current_heading_action().is_some() {
            return self.render_heading_context_menu(cx);
        }

        let mut items: Vec<AnyElement> = Vec::new();
        let is_slash_menu = self.slash_menu_start.is_some();
        let filter = self.slash_filter.to_lowercase();
        let selected_index = self.menu_selected_index;
        let mut actionable_index: usize = 0;
        let active_actions = self.active_format_actions();

        let matches_filter = |label: &str, filter_text: &str| -> bool {
            if filter.is_empty() {
                return true;
            }
            label.to_lowercase().contains(&filter) || filter_text.to_lowercase().contains(&filter)
        };

        if !is_slash_menu {
            let clipboard_items: &[(&str, &str, Option<&str>, FormatMenuAction)] = &[
                ("剪切", "cut", Some("⌘X"), FormatMenuAction::Cut),
                ("复制", "copy", Some("⌘C"), FormatMenuAction::Copy),
                ("粘贴", "paste", Some("⌘V"), FormatMenuAction::Paste),
            ];
            let mut clip_matched = Vec::new();
            for &(label, ft, shortcut, action) in clipboard_items {
                if matches_filter(label, ft) {
                    clip_matched.push(self.render_format_context_menu_item(
                        label,
                        None,
                        shortcut,
                        false,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.execute_format_menu_action(action, cx);
                        },
                    ));
                }
            }
            if !clip_matched.is_empty() {
                items.extend(clip_matched);
            }
        }

        // (label, filter_text, icon_path, shortcut, action)
        let all_menu_items: &[(&str, &str, Option<&str>, Option<&str>, FormatMenuAction)] = &[
            (
                "标题",
                "heading title h1",
                Some(HEADING_1_SVG_PATH),
                None,
                FormatMenuAction::Heading1,
            ),
            (
                "粗体",
                "bold",
                Some(FMT_BOLD_SVG),
                Some("⌘B"),
                FormatMenuAction::Bold,
            ),
            (
                "斜体",
                "italic",
                Some(FMT_ITALIC_SVG),
                Some("⌘I"),
                FormatMenuAction::Italic,
            ),
            (
                "删除线",
                "strikethrough",
                Some(FMT_STRIKETHROUGH_SVG),
                Some("⌘⇧S"),
                FormatMenuAction::Strikethrough,
            ),
            (
                "行内代码",
                "inline code",
                Some(FMT_INLINE_CODE_SVG),
                Some("⌘E"),
                FormatMenuAction::InlineCode,
            ),
            (
                "代码块",
                "code block",
                Some(FMT_CODE_SVG),
                Some("⌘⌥C"),
                FormatMenuAction::CodeBlock,
            ),
            (
                "分割线",
                "horizontal rule divider",
                Some(FMT_SPLIT_SVG),
                Some("---"),
                FormatMenuAction::HorizontalRule,
            ),
            (
                "引用",
                "quote blockquote",
                Some(FMT_QUOTE_SVG),
                Some("⌘⌥Q"),
                FormatMenuAction::Quote,
            ),
            (
                "无序列表",
                "unordered list bullet",
                Some(FMT_LIST_SVG),
                Some("⌘⌥U"),
                FormatMenuAction::UnorderedList,
            ),
            (
                "有序列表",
                "ordered list numbered",
                Some(FMT_ORDER_LIST_SVG),
                Some("⌘⌥O"),
                FormatMenuAction::OrderedList,
            ),
            (
                "待办事项",
                "todo task checkbox",
                Some(FMT_TODO_SVG),
                Some("⌘⌥T"),
                FormatMenuAction::Todo,
            ),
            (
                "链接",
                "link url",
                Some(FMT_LINK_SVG),
                Some("⌘K"),
                FormatMenuAction::Link,
            ),
            (
                "双向链接",
                "wiki link backlink bidirectional",
                Some(FMT_WIKI_LINK_SVG),
                Some("[["),
                FormatMenuAction::WikiLink,
            ),
            (
                "图片",
                "image photo",
                Some(FMT_IMAGE_SVG),
                Some("⌘⇧I"),
                FormatMenuAction::Image,
            ),
            (
                "表格",
                "table grid 2x2",
                Some(FMT_TABLE_SVG),
                Some("2×2"),
                FormatMenuAction::Table,
            ),
        ];

        let slash_only_actions: &[FormatMenuAction] = &[
            FormatMenuAction::Todo,
            FormatMenuAction::CodeBlock,
            FormatMenuAction::Table,
            FormatMenuAction::Link,
            FormatMenuAction::WikiLink,
            FormatMenuAction::Image,
        ];

        if is_slash_menu {
            // Render slash menu items in the fixed order defined by slash_only_actions
            for &slash_action in slash_only_actions.iter() {
                // Find the matching item in all_menu_items
                if let Some(&(label, ft, icon, shortcut, action)) = all_menu_items
                    .iter()
                    .find(|&&(_, _, _, _, a)| a == slash_action)
                {
                    if matches_filter(label, ft) {
                        let is_selected = actionable_index == selected_index;
                        let is_active = active_actions.contains(&action);
                        items.push(self.render_format_context_menu_item(
                            label,
                            icon,
                            shortcut,
                            is_selected,
                            is_active,
                            cx,
                            move |this, _, cx| {
                                this.execute_format_menu_action(action, cx);
                            },
                        ));
                        actionable_index += 1;
                    }
                }
            }
        } else {
            let group_boundaries = [0usize, 5, 11, 15];
            let mut group_start_flags = [false; 3];

            for (idx, &(label, ft, _, _, _)) in all_menu_items.iter().enumerate() {
                if matches_filter(label, ft) {
                    for g in 0..3 {
                        if idx >= group_boundaries[g] && idx < group_boundaries[g + 1] {
                            group_start_flags[g] = true;
                        }
                    }
                }
            }

            let mut prev_group_had_items = !items.is_empty();
            for g in 0..3 {
                if !group_start_flags[g] {
                    continue;
                }
                if prev_group_had_items {
                    items.push(self.render_context_menu_separator());
                }
                for idx in group_boundaries[g]..group_boundaries[g + 1] {
                    let (label, ft, icon, shortcut, action) = all_menu_items[idx];
                    if matches_filter(label, ft) {
                        let is_selected = false;
                        let is_active = active_actions.contains(&action);
                        items.push(self.render_format_context_menu_item(
                            label,
                            icon,
                            shortcut,
                            is_selected,
                            is_active,
                            cx,
                            move |this, _, cx| {
                                this.execute_format_menu_action(action, cx);
                            },
                        ));
                    }
                }
                prev_group_had_items = true;
            }
        }

        items
    }

    fn render_wiki_link_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut items: Vec<AnyElement> = Vec::new();
        let ms = &self.styles.context_menu;

        let has_query = !self.wiki_link_query.trim().is_empty();
        let show_loading = if has_query {
            self.wiki_link_search_task.is_some()
                && self.wiki_link_search_results.is_empty()
                && self.wiki_link_recent_results.is_empty()
        } else {
            self.wiki_link_recent_task.is_some() && self.wiki_link_recent_results.is_empty()
        };
        if show_loading {
            items.push(self.render_context_menu_item(
                "正在搜索文件...",
                false,
                cx,
                |_this, _, _cx| {},
            ));
            return items;
        }

        let search_items = if has_query {
            if self.wiki_link_search_results.is_empty() && self.wiki_link_search_task.is_some() {
                self.wiki_link_recent_results
                    .iter()
                    .map(|candidate| {
                        (
                            candidate.path.clone(),
                            candidate.file_name.clone(),
                            Vec::new(),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                self.wiki_link_search_results
                    .iter()
                    .map(|result| {
                        let path = &result.path;
                        (
                            path.clone(),
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string()),
                            result.file_name_match_indices.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            }
        } else {
            self.wiki_link_recent_results
                .iter()
                .map(|candidate| {
                    (
                        candidate.path.clone(),
                        candidate.file_name.clone(),
                        Vec::new(),
                    )
                })
                .collect::<Vec<_>>()
        };

        if search_items.is_empty() {
            let label = if has_query {
                "无匹配可引用文件"
            } else {
                "暂无可引用文件"
            };
            items.push(self.render_context_menu_item(label, false, cx, |_this, _, _cx| {}));
            return items;
        }

        for (index, (path, item, highlight_indices)) in search_items.into_iter().enumerate() {
            let is_selected = index == self.menu_selected_index;
            let hover_bg = ms.menu_item_hover_bg;
            items.push(
                div()
                    .id(SharedString::from(format!("wiki-link-item-{}", index)))
                    .w_full()
                    .h(px(self.scaled(ms.menu_item_height_px)))
                    .px(px(self.scaled(ms.menu_item_padding_x_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(self.styles.sidebar.sidebar_item_gap_px)))
                    .text_size(px(self.scaled(ms.menu_item_font_size_px)))
                    .text_color(ms.menu_item_text_color)
                    .bg(if is_selected {
                        ms.menu_item_selected_bg
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |style| style.bg(hover_bg))
                    .cursor(CursorStyle::Arrow)
                    .child(self.render_wiki_link_menu_indicator(&path, &item))
                    .child(self.render_wiki_link_menu_label(
                        &item,
                        &highlight_indices,
                        ms.menu_item_text_color,
                        self.styles.quick_open.quick_open_result_match_text_color,
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.menu_selected_index = index;
                            this.confirm_wiki_link_selection(cx);
                        }),
                    )
                    .into_any_element(),
            );
        }

        items
    }

    fn render_wiki_link_menu_label(
        &self,
        name: &str,
        highlight_indices: &[usize],
        color: gpui::Hsla,
        highlight_color: gpui::Hsla,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let (base, suffix) = split_file_name_for_display(name);
        let base_char_count = base.chars().count();
        let base_highlight_indices = highlight_indices
            .iter()
            .copied()
            .filter(|index| *index < base_char_count)
            .collect::<Vec<_>>();
        let suffix_highlight_indices = highlight_indices
            .iter()
            .filter_map(|index| index.checked_sub(base_char_count))
            .collect::<Vec<_>>();
        div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .flex()
            .gap(px(0.0))
            .items_center()
            .text_size(px(self.scaled(ss.sidebar_item_font_size_px)))
            .text_color(color)
            .child(div().whitespace_nowrap().child(
                crate::editor::quick_open::render_highlighted_text(
                    base,
                    &base_highlight_indices,
                    color,
                    highlight_color,
                    self.scaled(ss.sidebar_item_font_size_px),
                ),
            ))
            .when_some(suffix, |this, suffix| {
                this.child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .opacity(ss.sidebar_item_suffix_opacity)
                        .child(crate::editor::quick_open::render_highlighted_text(
                            suffix,
                            &suffix_highlight_indices,
                            color,
                            highlight_color,
                            self.scaled(ss.sidebar_item_font_size_px),
                        )),
                )
            })
            .into_any_element()
    }

    fn render_wiki_link_menu_indicator(&self, path: &std::path::Path, _name: &str) -> AnyElement {
        let ss = &self.styles.sidebar;

        if let Some(label) = file_kind_badge_text(path) {
            return div()
                .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
                .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
                .px(px(self.scaled(ss.sidebar_item_badge_padding_x_px)))
                .rounded(px(self.scaled(ss.sidebar_item_badge_radius_px)))
                .flex()
                .items_center()
                .justify_center()
                .bg(ss.sidebar_item_badge_bg)
                .text_size(px(self.scaled(ss.sidebar_item_badge_font_size_px)))
                .text_color(ss.sidebar_item_badge_text_color)
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(label)
                .into_any_element();
        }

        svg()
            .path(MARKDOWN_SVG_PATH)
            .size(px(self.scaled(ss.sidebar_item_icon_size_px + 4.0)))
            .text_color(ss.sidebar_item_icon_color)
            .into_any_element()
    }

    pub(crate) fn render_context_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = &self.context_menu else {
            return div().into_any_element();
        };
        let pos_x: f32 = menu.position.x.into();
        let pos_y: f32 = menu.position.y.into();

        let (items, width) = match &menu.kind {
            ContextMenuKind::Sidebar { path, is_dir } => (
                self.render_sidebar_context_menu(path.clone(), *is_dir, cx),
                self.scaled(self.styles.context_menu.menu_sidebar_width_px),
            ),
            ContextMenuKind::SidebarSort => {
                let mut items = Vec::new();
                for mode in [
                    SidebarSortMode::CreatedDesc,
                    SidebarSortMode::CreatedAsc,
                    SidebarSortMode::NameAsc,
                    SidebarSortMode::NameDesc,
                    SidebarSortMode::Type,
                ] {
                    items.push(self.render_context_menu_choice_item(
                        mode.label(),
                        self.sidebar_sort_mode == mode,
                        cx,
                        move |this, _, cx| {
                            this.sidebar_sort_mode = mode;
                            this.invalidate_sidebar_items_cache();
                            this.context_menu = None;
                            cx.notify();
                        },
                    ));
                }
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_sidebar_width_px),
                )
            }
            ContextMenuKind::SidebarCreate => {
                let mut items = Vec::new();
                {
                    let parent = self.sidebar_root.clone();
                    items.push(self.render_context_menu_item_with_icon(
                        "新建文件",
                        FILETREE_ADD_SVG_PATH,
                        false,
                        cx,
                        move |this, window, cx| {
                            this.start_create_file(parent.clone(), window, cx);
                        },
                    ));
                }
                {
                    let parent = self.sidebar_root.clone();
                    items.push(self.render_context_menu_item_with_icon(
                        "新建目录",
                        FOLDER_CLOSED_SVG_PATH,
                        false,
                        cx,
                        move |this, window, cx| {
                            this.start_create_dir(parent.clone(), window, cx);
                        },
                    ));
                }
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_sidebar_width_px),
                )
            }
            ContextMenuKind::Format => (
                self.render_format_context_menu(cx),
                self.scaled(self.styles.context_menu.menu_format_width_px),
            ),
            ContextMenuKind::WikiLink => (
                self.render_wiki_link_context_menu(cx),
                self.scaled(self.styles.context_menu.menu_sidebar_width_px),
            ),
            ContextMenuKind::Tab { tab_id } => {
                let mut items = Vec::new();
                let is_pinned = self
                    .tabs
                    .iter()
                    .find(|t| t.id == *tab_id)
                    .is_some_and(|t| t.is_pinned);
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.request_close_tab(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭其他",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_other_tabs(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭左侧",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_left_tabs(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭右侧",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_right_tabs(&id, cx);
                        },
                    ));
                }
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item(
                    "关闭已保存",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_saved_tabs(cx);
                    },
                ));
                items.push(self.render_context_menu_separator());
                {
                    let id = tab_id.clone();
                    let label = if is_pinned {
                        "取消固定"
                    } else {
                        "固定标签"
                    };
                    items.push(self.render_context_menu_item(
                        label,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.toggle_pin_tab(&id, cx);
                        },
                    ));
                }
                // Copy path if tab has a file
                if let Some(tab) = self.tabs.iter().find(|t| t.id == *tab_id) {
                    if let Some(path) = &tab.path {
                        let reveal_path = path.clone();
                        items.push(self.render_context_menu_item(
                            "定位到文件树",
                            false,
                            cx,
                            move |this, _, cx| {
                                this.reveal_path_in_sidebar(reveal_path.clone(), cx);
                            },
                        ));
                        {
                            let reveal_path = path.clone();
                            items.push(self.render_context_menu_item(
                                "在访达中显示",
                                false,
                                cx,
                                move |this, _, cx| {
                                    this.context_menu = None;
                                    this.reveal_in_finder(&reveal_path);
                                    cx.notify();
                                },
                            ));
                        }
                        items.push(self.render_context_menu_separator());
                        if let Some(relative_path_text) = self.path_copy_text(path, true) {
                            items.push(self.render_context_menu_item(
                                "复制相对路径",
                                false,
                                cx,
                                move |this, _, cx| {
                                    this.copy_text_to_clipboard(relative_path_text.clone(), cx);
                                },
                            ));
                        }
                        let path_text = path.display().to_string();
                        items.push(self.render_context_menu_item(
                            "复制路径",
                            false,
                            cx,
                            move |this, _, cx| {
                                this.copy_text_to_clipboard(path_text.clone(), cx);
                            },
                        ));
                    }
                }
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_tab_width_px),
                )
            }
            ContextMenuKind::Workspace => {
                let mut items = Vec::new();
                let wss = &self.styles.workspace_switcher;
                let current_root = self.sidebar_root.clone();
                let check_color = wss.ws_menu_item_check_color;
                let check_size = self.scaled(wss.ws_menu_item_check_size_px);
                let close_color = wss.ws_menu_item_close_color;
                let close_hover_color = wss.ws_menu_item_close_hover_color;
                let close_hover_bg = wss.ws_menu_item_close_hover_bg;
                let close_size = self.scaled(wss.ws_menu_item_close_size_px);
                let ms = &self.styles.context_menu;
                let row_hover_bg = ms.menu_item_hover_bg;
                let text_color = ms.menu_item_text_color;

                for workspace in self.workspaces.clone() {
                    let ws_name = workspace
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| workspace.display().to_string())
                        .to_uppercase();
                    let is_current = workspace == current_root;
                    let ws_switch = workspace.clone();
                    let ws_close = workspace.clone();
                    let item_id = SharedString::from(format!("ws-{}", ws_name));
                    let close_btn_id = SharedString::from(format!("ws-close-{}", ws_name));

                    items.push(
                        div()
                            .id(item_id)
                            .w_full()
                            .h(px(self.scaled(ms.menu_item_height_px)))
                            .px(px(self.scaled(ms.menu_item_padding_x_px)))
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(wss.ws_menu_item_action_icon_gap_px)))
                            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
                            .text_color(text_color)
                            .hover(move |s| s.bg(row_hover_bg))
                            .cursor(CursorStyle::Arrow)
                            .child(
                                div()
                                    .w(px(check_size))
                                    .h(px(check_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_none()
                                    .when(is_current, |this| {
                                        this.child(
                                            svg()
                                                .path(CHECK_SVG_PATH)
                                                .size(px(check_size))
                                                .text_color(check_color),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .overflow_hidden()
                                    .truncate()
                                    .child(ws_name),
                            )
                            .child(
                                div()
                                    .id(close_btn_id)
                                    .w(px(close_size))
                                    .h(px(close_size))
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .text_size(px(self.scaled(self.styles.ui_font_size_px - 3.0)))
                                    .text_color(close_color)
                                    .cursor(CursorStyle::Arrow)
                                    .hover(move |s| {
                                        s.text_color(close_hover_color).bg(close_hover_bg)
                                    })
                                    .child("✕")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, window, cx| {
                                            window.prevent_default();
                                            cx.stop_propagation();
                                            this.close_workspace(&ws_close, cx);
                                        }),
                                    ),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.switch_workspace(ws_switch.clone(), cx);
                                }),
                            )
                            .into_any_element(),
                    );
                }
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item_with_icon(
                    "打开目录 ...",
                    "assets/icons/filetree/all.svg",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.pick_workspace_folder(cx);
                    },
                ));
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item_with_icon(
                    "应用设置...",
                    "assets/icons/setting.svg",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.open_settings_tab(cx);
                    },
                ));
                (items, self.scaled(wss.ws_menu_width_px))
            }
            ContextMenuKind::Table {
                table_start_line,
                row,
                col,
            } => {
                let mut items = Vec::new();
                let tsl = *table_start_line;
                let r = *row;
                let c = *col;
                let is_front_matter = self
                    .decorations
                    .get(tsl)
                    .and_then(|dec| dec.table_info.as_ref())
                    .is_some_and(|ti| ti.is_front_matter);
                {
                    let tsl = tsl;
                    let r = r;
                    items.push(self.render_context_menu_item(
                        "上方新增行",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.table_insert_row(tsl, r, true, cx);
                        },
                    ));
                }
                {
                    let tsl = tsl;
                    let r = r;
                    items.push(self.render_context_menu_item(
                        "下方新增行",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.table_insert_row(tsl, r, false, cx);
                        },
                    ));
                }
                if !is_front_matter {
                    {
                        let tsl = tsl;
                        let c = c;
                        items.push(self.render_context_menu_item(
                            "左侧新增列",
                            false,
                            cx,
                            move |this, _, cx| {
                                this.table_insert_column(tsl, c, true, cx);
                            },
                        ));
                    }
                    {
                        let tsl = tsl;
                        let c = c;
                        items.push(self.render_context_menu_item(
                            "右侧新增列",
                            false,
                            cx,
                            move |this, _, cx| {
                                this.table_insert_column(tsl, c, false, cx);
                            },
                        ));
                    }
                }
                items.push(self.render_context_menu_separator());
                {
                    let tsl = tsl;
                    let r = r;
                    items.push(self.render_context_menu_item(
                        "删除行",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.table_delete_row(tsl, r, cx);
                        },
                    ));
                }
                if !is_front_matter {
                    {
                        let tsl = tsl;
                        let c = c;
                        items.push(self.render_context_menu_item(
                            "删除列",
                            false,
                            cx,
                            move |this, _, cx| {
                                this.table_delete_column(tsl, c, cx);
                            },
                        ));
                    }
                }
                items.push(self.render_context_menu_separator());
                {
                    let tsl = tsl;
                    items.push(self.render_context_menu_item(
                        if is_front_matter {
                            "删除 Front Matter"
                        } else {
                            "删除表格"
                        },
                        false,
                        cx,
                        move |this, _, cx| {
                            this.table_delete_table(tsl, cx);
                        },
                    ));
                }
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_format_width_px),
                )
            }
        };

        let ms = &self.styles.context_menu;

        // Estimate total menu height from items
        let item_count = items.len();
        let estimated_item_height = self.scaled(ms.menu_item_height_px);
        let menu_padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        let estimated_menu_height = item_count as f32 * estimated_item_height + menu_padding;

        // Use window bounds for positioning, adjusted for editor-root border
        let border_w = self.styles.window.window_border_width_px;
        let overlay_w = self.last_window_width - 2.0 * border_w;
        let overlay_h = self.last_window_height - 2.0 * border_w;
        // Convert window-coordinate mouse position to overlay-local coordinates
        let pos_x = (pos_x - border_w).clamp(0.0, overlay_w);
        let pos_y = (pos_y - border_w).clamp(0.0, overlay_h);
        let margin = self.scaled(ms.menu_max_height_margin_px).max(4.0);

        let space_right = overlay_w - pos_x - margin;
        let space_left = pos_x - margin;
        let space_below = overlay_h - pos_y - margin;
        let space_above = pos_y - margin;

        // Decide direction
        let open_right = space_right >= width || space_right >= space_left;
        let open_down = space_below >= estimated_menu_height || space_below >= space_above;

        let final_x = if open_right {
            pos_x
        } else {
            (pos_x - width).max(margin)
        };

        let max_menu_height = if open_down {
            (overlay_h - pos_y - margin).max(self.scaled(100.0))
        } else {
            (pos_y - margin).max(self.scaled(100.0))
        };

        // Use .top() for downward, .bottom() for upward to anchor the correct edge
        let mut menu_div = div()
            .id("context-menu")
            .absolute()
            .left(px(final_x))
            .w(px(width))
            .max_h(px(max_menu_height))
            .overflow_y_scroll()
            .py(px(self.scaled(ms.menu_padding_y_px)))
            .bg(ms.menu_bg)
            .border(px(ms.menu_border_width_px))
            .border_color(ms.menu_border_color)
            .rounded(px(self.scaled(ms.menu_radius_px)));
        if open_down {
            menu_div = menu_div.top(px(pos_y));
        } else {
            // Anchor bottom edge of menu at cursor y position
            menu_div = menu_div.bottom(px(overlay_h - pos_y));
        }
        if ms.menu_shadow {
            menu_div = menu_div.shadow_sm();
        }
        menu_div
            .children(items)
            .cursor(CursorStyle::Arrow)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event, _window, _cx| {}),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_this, _event, _window, _cx| {}),
            )
            .on_mouse_move(cx.listener(|_this, _event, _window, _cx| {}))
            .on_scroll_wheel(cx.listener(|_this, _event, _window, _cx| {}))
            .into_any_element()
    }
}
