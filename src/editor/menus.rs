use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, Pixels, Point, SharedString, div,
    prelude::*, px, svg,
};

use crate::markdown::{BlockType, InlineKind, LineDecoration, raw_wiki_link_ranges};

use super::shortcut::{ShortcutRenderStyle, render_shortcut};
use super::*;

const ADD_FILE_TO_CHAT_LABEL: &str = "添加文件到聊天";
const ADD_SELECTION_TO_CHAT_LABEL: &str = "添加选区到聊天";
const TERMINAL_SELECT_ALL_LABEL: &str = "全选";

fn current_raw_wiki_link_range(
    decoration: Option<&LineDecoration>,
    line_content: &str,
    cursor_col: usize,
    selection_start_col: usize,
    selection_end_col: usize,
) -> Option<Range<usize>> {
    raw_wiki_link_ranges(line_content, decoration)
        .into_iter()
        .find(|range| {
            let cursor_in_range = cursor_col >= range.start && cursor_col <= range.end;
            let selection_in_range =
                selection_start_col >= range.start && selection_end_col <= range.end;
            cursor_in_range || selection_in_range
        })
}

fn menu_icon_column_metrics(icon_size_px: f32, check_size_px: f32, gap_px: f32) -> (f32, f32) {
    (check_size_px.max(icon_size_px), gap_px)
}

fn image_markdown_ranges(line_content: &str) -> Option<(Range<usize>, Range<usize>, Range<usize>)> {
    let full_start = line_content.find("![")?;
    let desc_start = full_start + 2;
    let desc_end = desc_start + line_content[desc_start..].find("](")?;
    let url_start = desc_end + 2;
    let url_end = url_start + line_content[url_start..].find(')')?;
    Some((
        full_start..url_end + 1,
        desc_start..desc_end,
        url_start..url_end,
    ))
}

#[derive(Clone)]
pub(super) struct ImageMenuTarget {
    pub(super) line: usize,
    pub(super) full_range: Range<usize>,
    pub(super) desc_range: Range<usize>,
    pub(super) url_range: Range<usize>,
}

impl EditorView {
    pub(crate) fn open_starred_sidebar_context_menu(
        &mut self,
        path: PathBuf,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::SidebarStarred { path },
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    fn current_wiki_link_target(&self) -> Option<Range<usize>> {
        let line = self.cursor_line();
        let decoration = self.decorations.get(line)?;
        let line_start = self.buffer.line_start(line);
        let selection = self.cursor.selection_range();
        let cursor_offset = self.cursor.offset;

        decoration
            .wiki_links
            .iter()
            .find_map(|wiki_link| {
                let full_start = line_start + wiki_link.full_range.start;
                let full_end = line_start + wiki_link.full_range.end;
                let cursor_in_link = cursor_offset >= full_start && cursor_offset <= full_end;
                let selection_in_link = selection.start >= full_start && selection.end <= full_end;
                (cursor_in_link || selection_in_link).then_some(full_start..full_end)
            })
            .or_else(|| {
                let line_content = self.buffer.line_content(line);
                let cursor_col = cursor_offset
                    .saturating_sub(line_start)
                    .min(line_content.len());
                let selection_start_col = selection
                    .start
                    .saturating_sub(line_start)
                    .min(line_content.len());
                let selection_end_col = selection
                    .end
                    .saturating_sub(line_start)
                    .min(line_content.len());
                current_raw_wiki_link_range(
                    Some(decoration),
                    line_content,
                    cursor_col,
                    selection_start_col,
                    selection_end_col,
                )
                .map(|range| line_start + range.start..line_start + range.end)
            })
    }

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

    pub(super) fn image_menu_target_on_line(&self, line: usize) -> Option<ImageMenuTarget> {
        let decoration = self.decorations.get(line)?;
        decoration
            .inline_spans
            .iter()
            .find(|span| span.kind == InlineKind::Image)?;
        let line_start = self.buffer.line_start(line);
        let line_content = self.buffer.line_content(line);
        let (full_range, desc_range, url_range) = image_markdown_ranges(line_content)?;

        Some(ImageMenuTarget {
            line,
            full_range: line_start + full_range.start..line_start + full_range.end,
            desc_range: line_start + desc_range.start..line_start + desc_range.end,
            url_range: line_start + url_range.start..line_start + url_range.end,
        })
    }

    fn current_image_menu_target(&self) -> Option<ImageMenuTarget> {
        let line = self.cursor_line();
        let target = self.image_menu_target_on_line(line)?;
        let selection = self.cursor.selection_range();
        let cursor_offset = self.cursor.offset;
        let selected_image =
            selection.start == target.full_range.start && selection.end == target.full_range.end;
        let cursor_in_image =
            cursor_offset >= target.full_range.start && cursor_offset <= target.full_range.end;
        if !selected_image && !cursor_in_image {
            return None;
        }

        Some(target)
    }

    pub(super) fn set_selection_range(&mut self, range: Range<usize>) {
        self.cursor.anchor = range.start.min(self.buffer.len());
        self.cursor.offset = range.end.min(self.buffer.len());
        self.scroll_to_cursor();
    }

    fn copy_text_to_clipboard(&mut self, text: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        cx.notify();
    }

    fn context_menu_icon_path(&self, label: &str) -> Option<&'static str> {
        let delete_prefix = format!("{} (", self.t(crate::i18n::TextKey::Delete));
        match label {
            current if current == self.t(crate::i18n::TextKey::NewFile) => {
                Some(MENU_NEW_FILE_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::MenuNewFolder) => {
                Some(MENU_NEW_FOLDER_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::Rename) => {
                Some(MENU_RENAME_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::Cut) => Some(MENU_CLIP_SVG_PATH),
            current if current == self.t(crate::i18n::TextKey::Copy) => Some(MENU_PASTE_SVG_PATH),
            current if current == self.t(crate::i18n::TextKey::Paste) => Some(MENU_PASTE_SVG_PATH),
            current if current == self.t(crate::i18n::TextKey::CollapseAllDirectories) => {
                Some(MENU_FOLD_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::ExpandAllDirectories) => {
                Some(MENU_EXPAND_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::RevealInFinder) => {
                Some(MENU_LOCATE_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::Star) => Some(MENU_STAR_SVG_PATH),
            current if current == self.t(crate::i18n::TextKey::Unstar) => {
                Some(MENU_UNSTAR_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::PinTab) => Some(MENU_PIN_SVG_PATH),
            current if current == self.t(crate::i18n::TextKey::UnpinTab) => {
                Some(MENU_UNPIN_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::RevealInFileTree) => {
                Some(MENU_LOCATE_TREE_SVG_PATH)
            }
            ADD_FILE_TO_CHAT_LABEL => Some(MENU_AT_SVG_PATH),
            ADD_SELECTION_TO_CHAT_LABEL => Some(CHAT_SVG_PATH),
            _ if label == self.t(crate::i18n::TextKey::Delete)
                || label.starts_with(&delete_prefix) =>
            {
                Some(MENU_DELETE_SVG_PATH)
            }
            _ => None,
        }
    }

    fn format_action_label(&self, action: FormatMenuAction) -> String {
        match action {
            FormatMenuAction::Cut => self.t(crate::i18n::TextKey::Cut).to_string(),
            FormatMenuAction::Copy => self.t(crate::i18n::TextKey::Copy).to_string(),
            FormatMenuAction::Paste => self.t(crate::i18n::TextKey::Paste).to_string(),
            FormatMenuAction::Heading1 => crate::i18n::heading_level_label(self.ui_language, 1),
            FormatMenuAction::Heading2 => crate::i18n::heading_level_label(self.ui_language, 2),
            FormatMenuAction::Heading3 => crate::i18n::heading_level_label(self.ui_language, 3),
            FormatMenuAction::Heading4 => crate::i18n::heading_level_label(self.ui_language, 4),
            FormatMenuAction::Heading5 => crate::i18n::heading_level_label(self.ui_language, 5),
            FormatMenuAction::Heading6 => crate::i18n::heading_level_label(self.ui_language, 6),
            FormatMenuAction::Paragraph => self.t(crate::i18n::TextKey::Paragraph).to_string(),
            FormatMenuAction::Bold => self.t(crate::i18n::TextKey::Bold).to_string(),
            FormatMenuAction::Italic => self.t(crate::i18n::TextKey::Italic).to_string(),
            FormatMenuAction::Strikethrough => {
                self.t(crate::i18n::TextKey::Strikethrough).to_string()
            }
            FormatMenuAction::InlineCode => self.t(crate::i18n::TextKey::InlineCode).to_string(),
            FormatMenuAction::CodeBlock => self.t(crate::i18n::TextKey::CodeBlock).to_string(),
            FormatMenuAction::HorizontalRule => {
                self.t(crate::i18n::TextKey::HorizontalRule).to_string()
            }
            FormatMenuAction::Quote => self.t(crate::i18n::TextKey::Quote).to_string(),
            FormatMenuAction::UnorderedList => {
                self.t(crate::i18n::TextKey::UnorderedList).to_string()
            }
            FormatMenuAction::OrderedList => self.t(crate::i18n::TextKey::OrderedList).to_string(),
            FormatMenuAction::Todo => self.t(crate::i18n::TextKey::Todo).to_string(),
            FormatMenuAction::Link => self.t(crate::i18n::TextKey::Link).to_string(),
            FormatMenuAction::WikiLink => self.t(crate::i18n::TextKey::WikiLink).to_string(),
            FormatMenuAction::Image => self.t(crate::i18n::TextKey::Image).to_string(),
            FormatMenuAction::Table => self.t(crate::i18n::TextKey::Table).to_string(),
            FormatMenuAction::EditImageDescription => {
                self.t(crate::i18n::TextKey::EditDescription).to_string()
            }
            FormatMenuAction::EditImageUrl => self.t(crate::i18n::TextKey::EditUrl).to_string(),
            FormatMenuAction::DeleteImage => self.t(crate::i18n::TextKey::Delete).to_string(),
            FormatMenuAction::SelectImageFile => {
                self.t(crate::i18n::TextKey::SelectImage).to_string()
            }
        }
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
        let query_end = self
            .wiki_link_query_end(start)
            .unwrap_or(self.cursor.offset);
        let query_start = start + 2;
        self.wiki_link_query = if query_start <= query_end && query_end <= self.buffer.len() {
            self.buffer.text()[query_start..query_end].to_string()
        } else {
            String::new()
        };
        self.wiki_link_search_results.clear();
        self.menu_selected_index = 0;
        let pos = self.wiki_link_menu_position(start);
        self.context_menu = Some(ContextMenu {
            position: pos,
            kind: ContextMenuKind::WikiLink,
        });
        if self.wiki_link_query.trim().is_empty() {
            self.run_wiki_link_recent_search(cx);
        } else {
            self.run_wiki_link_search(cx);
        }
        cx.notify();
    }

    pub(crate) fn ensure_wiki_link_menu_for_cursor(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.current_wiki_link_target() else {
            return false;
        };

        if self.wiki_link_start != Some(target.start)
            || !matches!(
                self.context_menu.as_ref().map(|menu| &menu.kind),
                Some(ContextMenuKind::WikiLink)
            )
        {
            self.open_wiki_link_menu(target.start, cx);
        }
        self.sync_wiki_link_menu(cx);
        true
    }

    pub(crate) fn maybe_open_wiki_link_menu_after_edit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.current_wiki_link_target() else {
            return false;
        };

        let query_start = target.start + 2;
        let query_end = target.end.saturating_sub(2);
        if self.cursor.offset < query_start || self.cursor.offset > query_end {
            return false;
        }

        if self.wiki_link_start != Some(target.start)
            || !matches!(
                self.context_menu.as_ref().map(|menu| &menu.kind),
                Some(ContextMenuKind::WikiLink)
            )
        {
            self.open_wiki_link_menu(target.start, cx);
            return true;
        }

        self.sync_wiki_link_menu(cx);
        true
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
            let query_start = start + 2;
            let query_end = self
                .wiki_link_query_end(start)
                .unwrap_or(self.cursor.offset)
                .min(self.buffer.len());
            let end = self.cursor.offset.clamp(query_start, query_end);
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

    pub(crate) fn open_terminal_context_menu(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Terminal,
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
        let full_end = query_end + 2;
        let query_start = start + 2;
        if self.cursor.offset < start || self.cursor.offset > full_end {
            self.close_context_menu(cx);
            return;
        }
        if self.cursor.offset < query_start {
            self.menu_selected_index = 0;
            let pos = self.wiki_link_menu_position(start);
            if let Some(menu) = &mut self.context_menu {
                menu.position = pos;
            }
            cx.notify();
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
                self.wrap_current_lines_as_code_block();
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
                self.normalize_ordered_list_near_line(self.cursor_line(), true);
                self.reparse();
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
                self.ensure_wiki_link_menu_for_cursor(cx);
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
        let all_items = [
            (
                self.format_action_label(FormatMenuAction::Todo),
                "todo task checkbox",
                FormatMenuAction::Todo,
            ),
            (
                self.format_action_label(FormatMenuAction::CodeBlock),
                "code block",
                FormatMenuAction::CodeBlock,
            ),
            (
                self.format_action_label(FormatMenuAction::Table),
                "table grid 2x2",
                FormatMenuAction::Table,
            ),
            (
                self.format_action_label(FormatMenuAction::Link),
                "link url",
                FormatMenuAction::Link,
            ),
            (
                self.format_action_label(FormatMenuAction::WikiLink),
                "wiki link backlink bidirectional",
                FormatMenuAction::WikiLink,
            ),
            (
                self.format_action_label(FormatMenuAction::Image),
                "image photo",
                FormatMenuAction::Image,
            ),
        ];
        for (label, ft, action) in all_items {
            if matches_filter(&label, ft) {
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
        if let Some(icon_path) = self.context_menu_icon_path(label) {
            return self.render_context_menu_item_with_icon(
                label,
                icon_path,
                is_destructive,
                cx,
                handler,
            );
        }

        let ms = &self.styles.context_menu;
        let wss = &self.styles.workspace_switcher;
        let text_color = if is_destructive {
            ms.menu_item_danger_text_color
        } else {
            ms.menu_item_text_color
        };
        let hover_bg = ms.menu_item_hover_bg;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        let (icon_col_w, gap) = menu_icon_column_metrics(
            ms.menu_format_icon_size_px,
            wss.ws_menu_item_check_size_px,
            wss.ws_menu_item_action_icon_gap_px,
        );
        div()
            .id(item_id)
            .w_full()
            .h(px(ms.menu_item_height_px))
            .px(px(ms.menu_item_padding_x_px))
            .flex()
            .items_center()
            .gap(px(gap))
            .text_size(px(ms.menu_item_font_size_px))
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
                    .justify_center(),
            )
            .child(label.to_string())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
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
        let (icon_col_w, gap) = menu_icon_column_metrics(
            ms.menu_format_icon_size_px,
            wss.ws_menu_item_check_size_px,
            wss.ws_menu_item_action_icon_gap_px,
        );
        let icon_size = ms.menu_format_icon_size_px;
        div()
            .id(item_id)
            .w_full()
            .h(px(ms.menu_item_height_px))
            .px(px(ms.menu_item_padding_x_px))
            .flex()
            .items_center()
            .gap(px(gap))
            .text_size(px(ms.menu_item_font_size_px))
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
                    cx.stop_propagation();
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
                    cx.stop_propagation();
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
        let icon_path = icon_path.or_else(|| self.context_menu_icon_path(label));
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
            icon_path.map(|path| SharedString::from(path.to_string()));
        let shortcut_style = ShortcutRenderStyle {
            font_size_px: self.scaled(ms.menu_shortcut_font_size_px),
            text_color: ms.menu_shortcut_text_color,
            gap_px: self.scaled(ms.menu_shortcut_gap_px),
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
                    cx.stop_propagation();
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
                    self.t(crate::i18n::TextKey::NewFile),
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
                    self.t(crate::i18n::TextKey::MenuNewFolder),
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
                    self.t(crate::i18n::TextKey::CollapseAllDirectories),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.collapse_all_dirs(cx);
                    },
                ));
            }
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::ExpandAllDirectories),
                false,
                cx,
                move |this, _, cx| {
                    this.expand_all_dirs(cx);
                },
            ));
            items.push(self.render_context_menu_separator());
            if let Some(copy_path_text) = self.sidebar_paths_copy_text(&path, false) {
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::CopyPath),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.copy_text_to_clipboard(copy_path_text.clone(), cx);
                    },
                ));
            }
            if let Some(copy_relative_path_text) = self.sidebar_paths_copy_text(&path, true) {
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::CopyRelativePath),
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
                    self.t(crate::i18n::TextKey::RevealInFinder),
                    false,
                    cx,
                    move |this, _, _cx| {
                        this.reveal_in_finder(&p);
                    },
                ));
            }
            items.push(self.render_context_menu_separator());
            {
                let chat_path = path.clone();
                items.push(self.render_context_menu_item(
                    ADD_FILE_TO_CHAT_LABEL,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.add_path_to_chat(&chat_path, cx);
                    },
                ));
            }
            items.push(self.render_context_menu_separator());
            {
                let p = path.clone();
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::CloseFolder),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_workspace(&p, cx);
                    },
                ));
            }
            return items;
        }

        {
            let pd = parent_dir.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::NewFile),
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
                self.t(crate::i18n::TextKey::MenuNewFolder),
                false,
                cx,
                move |this, window, cx| {
                    this.start_create_dir(pd.clone(), window, cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        if !is_dir {
            let starred_path = path.clone();
            let is_starred = self.app_settings.is_starred_file(&starred_path);
            let label = if is_starred {
                self.t(crate::i18n::TextKey::Unstar)
            } else {
                self.t(crate::i18n::TextKey::Star)
            };
            items.push(
                self.render_context_menu_item(label, false, cx, move |this, _, cx| {
                    let normalized = Self::normalize_sidebar_path(&starred_path);
                    if is_starred {
                        this.app_settings.remove_starred_file(&normalized);
                    } else {
                        this.app_settings.add_starred_file(normalized);
                    }
                    this.app_settings.save();
                    this.context_menu = None;
                    cx.notify();
                }),
            );
            items.push(self.render_context_menu_separator());
        }
        {
            let chat_path = path.clone();
            items.push(self.render_context_menu_item(
                ADD_FILE_TO_CHAT_LABEL,
                false,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.add_path_to_chat(&chat_path, cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        // Rename: only for single selection
        if self.selected_sidebar_paths.len() <= 1 {
            let p = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Rename),
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
                crate::i18n::delete_items_label(self.ui_language, count)
            } else {
                self.t(crate::i18n::TextKey::Delete).to_string()
            };
            items.push(self.render_context_menu_item(
                &delete_label,
                true,
                cx,
                move |this, _, cx| {
                    if selected.len() == 1 {
                        this.delete_path(selected[0].clone(), cx);
                    } else {
                        this.delete_paths_to_trash(&selected, cx);
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
            let cut_path = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Cut),
                false,
                cx,
                move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: vec![Self::normalize_sidebar_path(&cut_path)],
                        is_cut: true,
                    });
                    this.context_menu = None;
                    cx.notify();
                },
            ));
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Copy),
                false,
                cx,
                move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: paths.clone(),
                        is_cut: false,
                    });
                    this.context_menu = None;
                    cx.notify();
                },
            ));
        }
        if has_clipboard {
            let pd = parent_dir.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Paste),
                false,
                cx,
                move |this, _, cx| {
                    this.paste_into_dir(pd.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        if let Some(copy_path_text) = self.sidebar_paths_copy_text(&path, false) {
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::CopyPath),
                false,
                cx,
                move |this, _, cx| {
                    this.copy_text_to_clipboard(copy_path_text.clone(), cx);
                },
            ));
        }
        if let Some(copy_relative_path_text) = self.sidebar_paths_copy_text(&path, true) {
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::CopyRelativePath),
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
                self.t(crate::i18n::TextKey::CollapseAllDirectories),
                false,
                cx,
                move |this, _, cx| {
                    this.collapse_all_dirs(cx);
                },
            ));
        }
        {
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::ExpandAllDirectories),
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
                self.t(crate::i18n::TextKey::RevealInFinder),
                false,
                cx,
                move |this, _, _cx| {
                    this.reveal_in_finder(&p);
                },
            ));
        }

        items
    }

    fn render_starred_sidebar_context_menu(
        &self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut items: Vec<AnyElement> = Vec::new();

        {
            let starred_path = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Unstar),
                false,
                cx,
                move |this, _, cx| {
                    let normalized = Self::normalize_sidebar_path(&starred_path);
                    if this.app_settings.remove_starred_file(&normalized) {
                        this.app_settings.save();
                    }
                    this.context_menu = None;
                    cx.notify();
                },
            ));
        }
        items.push(self.render_context_menu_separator());

        {
            let rename_path = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Rename),
                false,
                cx,
                move |this, window, cx| {
                    this.start_starred_rename(rename_path.clone(), window, cx);
                },
            ));
        }
        {
            let delete_path = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Delete),
                true,
                cx,
                move |this, _, cx| {
                    this.delete_path(delete_path.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());

        if let Some(copy_path_text) = self.path_copy_text(&path, false) {
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::CopyPath),
                false,
                cx,
                move |this, _, cx| {
                    this.copy_text_to_clipboard(copy_path_text.clone(), cx);
                },
            ));
        }
        if let Some(copy_relative_path_text) = self.path_copy_text(&path, true) {
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::CopyRelativePath),
                false,
                cx,
                move |this, _, cx| {
                    this.copy_text_to_clipboard(copy_relative_path_text.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());

        {
            let reveal_path = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::RevealInFinder),
                false,
                cx,
                move |this, _, _cx| {
                    this.reveal_in_finder(&reveal_path);
                },
            ));
        }

        items
    }

    fn render_heading_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let active_actions = self.active_format_actions();
        let heading_items = [
            (Some(HEADING_1_SVG_PATH), FormatMenuAction::Heading1),
            (Some(HEADING_2_SVG_PATH), FormatMenuAction::Heading2),
            (Some(HEADING_3_SVG_PATH), FormatMenuAction::Heading3),
            (Some(HEADING_4_SVG_PATH), FormatMenuAction::Heading4),
            (Some(HEADING_5_SVG_PATH), FormatMenuAction::Heading5),
            (Some(HEADING_6_SVG_PATH), FormatMenuAction::Heading6),
            (None, FormatMenuAction::Paragraph),
        ];

        heading_items
            .iter()
            .map(|&(icon, action)| {
                let is_active = if action == FormatMenuAction::Paragraph {
                    self.current_heading_action().is_none()
                } else {
                    active_actions.contains(&action)
                };
                let label = self.format_action_label(action);
                self.render_format_context_menu_item(
                    &label,
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
        let mut items: Vec<AnyElement> = [
            (None, FormatMenuAction::EditImageDescription, false),
            (None, FormatMenuAction::EditImageUrl, false),
            (None, FormatMenuAction::DeleteImage, true),
            (None, FormatMenuAction::SelectImageFile, false),
        ]
        .into_iter()
        .map(|(icon, action, _is_destructive)| {
            let label = self.format_action_label(action);
            self.render_format_context_menu_item(
                &label,
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
        .collect();

        if let Some(target) = self.current_image_menu_target()
            && let Some(raw_image_path) = self
                .decorations
                .get(target.line)
                .and_then(|decoration| decoration.image_path.as_ref())
            && !helpers::is_remote_image_url(raw_image_path)
        {
            let reveal_path = if let Some(stripped) = raw_image_path.strip_prefix("file://") {
                PathBuf::from(stripped)
            } else {
                let image_path = PathBuf::from(raw_image_path);
                if image_path.is_absolute() {
                    image_path
                } else if let Some(file_dir) = self
                    .current_file_path
                    .as_ref()
                    .and_then(|path| path.parent().map(PathBuf::from))
                {
                    file_dir.join(image_path)
                } else {
                    self.sidebar_root.join(image_path)
                }
            };

            if reveal_path.exists() {
                let label = self.t(crate::i18n::TextKey::RevealInFinder).to_string();
                items.push(self.render_format_context_menu_item(
                    &label,
                    None,
                    None,
                    false,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.context_menu = None;
                        this.reveal_in_finder(&reveal_path);
                        cx.notify();
                    },
                ));
            }
        }

        items
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
            let clipboard_items = [
                (
                    self.format_action_label(FormatMenuAction::Cut),
                    "cut",
                    Some("⌘X"),
                    FormatMenuAction::Cut,
                ),
                (
                    self.format_action_label(FormatMenuAction::Copy),
                    "copy",
                    Some("⌘C"),
                    FormatMenuAction::Copy,
                ),
                (
                    self.format_action_label(FormatMenuAction::Paste),
                    "paste",
                    Some("⌘V"),
                    FormatMenuAction::Paste,
                ),
            ];
            let mut clip_matched = Vec::new();
            for (label, ft, shortcut, action) in clipboard_items {
                if matches_filter(&label, ft) {
                    clip_matched.push(self.render_format_context_menu_item(
                        &label,
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

            if let Some(current_path) = self.current_file_path.clone() {
                if !items.is_empty() {
                    items.push(self.render_context_menu_separator());
                }
                let chat_path = current_path.clone();
                items.push(self.render_format_context_menu_item(
                    ADD_FILE_TO_CHAT_LABEL,
                    Some(MENU_AT_SVG_PATH),
                    None,
                    false,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.add_path_to_chat(&chat_path, cx);
                    },
                ));

                let selection = self.cursor.selection_range();
                if !selection.is_empty() {
                    let selection_path = current_path.clone();
                    let selection_range = selection.clone();
                    items.push(self.render_format_context_menu_item(
                        ADD_SELECTION_TO_CHAT_LABEL,
                        Some(CHAT_SVG_PATH),
                        None,
                        false,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.add_selection_to_chat(
                                &selection_path,
                                selection_range.clone(),
                                cx,
                            );
                        },
                    ));
                }
            }
        }

        // (label, filter_text, icon_path, shortcut, action)
        let all_menu_items = [
            (
                self.t(crate::i18n::TextKey::Heading).to_string(),
                "heading title h1",
                Some(HEADING_1_SVG_PATH),
                None,
                FormatMenuAction::Heading1,
            ),
            (
                self.format_action_label(FormatMenuAction::Bold),
                "bold",
                Some(FMT_BOLD_SVG),
                Some("⌘B"),
                FormatMenuAction::Bold,
            ),
            (
                self.format_action_label(FormatMenuAction::Italic),
                "italic",
                Some(FMT_ITALIC_SVG),
                Some("⌘I"),
                FormatMenuAction::Italic,
            ),
            (
                self.format_action_label(FormatMenuAction::Strikethrough),
                "strikethrough",
                Some(FMT_STRIKETHROUGH_SVG),
                Some("⌘⇧S"),
                FormatMenuAction::Strikethrough,
            ),
            (
                self.format_action_label(FormatMenuAction::InlineCode),
                "inline code",
                Some(FMT_INLINE_CODE_SVG),
                Some("⌘E"),
                FormatMenuAction::InlineCode,
            ),
            (
                self.format_action_label(FormatMenuAction::CodeBlock),
                "code block",
                Some(FMT_CODE_SVG),
                Some("⌘⌥C"),
                FormatMenuAction::CodeBlock,
            ),
            (
                self.format_action_label(FormatMenuAction::HorizontalRule),
                "horizontal rule divider",
                Some(FMT_SPLIT_SVG),
                Some("---"),
                FormatMenuAction::HorizontalRule,
            ),
            (
                self.format_action_label(FormatMenuAction::Quote),
                "quote blockquote",
                Some(FMT_QUOTE_SVG),
                Some("⌘⌥Q"),
                FormatMenuAction::Quote,
            ),
            (
                self.format_action_label(FormatMenuAction::UnorderedList),
                "unordered list bullet",
                Some(FMT_LIST_SVG),
                Some("⌘⌥U"),
                FormatMenuAction::UnorderedList,
            ),
            (
                self.format_action_label(FormatMenuAction::OrderedList),
                "ordered list numbered",
                Some(FMT_ORDER_LIST_SVG),
                Some("⌘⌥O"),
                FormatMenuAction::OrderedList,
            ),
            (
                self.format_action_label(FormatMenuAction::Todo),
                "todo task checkbox",
                Some(FMT_TODO_SVG),
                Some("⌘⌥T"),
                FormatMenuAction::Todo,
            ),
            (
                self.format_action_label(FormatMenuAction::Link),
                "link url",
                Some(FMT_LINK_SVG),
                Some("⌘K"),
                FormatMenuAction::Link,
            ),
            (
                self.format_action_label(FormatMenuAction::WikiLink),
                "wiki link backlink bidirectional",
                Some(FMT_WIKI_LINK_SVG),
                Some("[["),
                FormatMenuAction::WikiLink,
            ),
            (
                self.format_action_label(FormatMenuAction::Image),
                "image photo",
                Some(FMT_IMAGE_SVG),
                Some("⌘⇧I"),
                FormatMenuAction::Image,
            ),
            (
                self.format_action_label(FormatMenuAction::Table),
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
                if let Some((label, ft, icon, shortcut, action)) = all_menu_items
                    .iter()
                    .find(|(_, _, _, _, a)| *a == slash_action)
                    .cloned()
                {
                    if matches_filter(&label, ft) {
                        let is_selected = actionable_index == selected_index;
                        let is_active = active_actions.contains(&action);
                        items.push(self.render_format_context_menu_item(
                            &label,
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

            for (idx, (label, ft, _, _, _)) in all_menu_items.iter().enumerate() {
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
                    let (label, ft, icon, shortcut, action) = all_menu_items[idx].clone();
                    if matches_filter(&label, ft) {
                        let is_selected = false;
                        let is_active = active_actions.contains(&action);
                        items.push(self.render_format_context_menu_item(
                            &label,
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
            let label = self.t(crate::i18n::TextKey::Searching).to_string();
            items.push(self.render_context_menu_item(&label, false, cx, |_this, _, _cx| {}));
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
                self.t(crate::i18n::TextKey::NoMatchingReferenceFiles)
            } else {
                self.t(crate::i18n::TextKey::NoReferenceFiles)
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
            ContextMenuKind::SidebarStarred { path } => (
                self.render_starred_sidebar_context_menu(path.clone(), cx),
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
                        self.t(mode.label_key()),
                        self.sidebar_sort_mode == mode,
                        cx,
                        move |this, _, cx| {
                            this.sidebar_sort_mode = mode;
                            this.app_settings.set_sidebar_sort_mode(mode);
                            this.app_settings.save();
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
                        self.t(crate::i18n::TextKey::NewFile),
                        MENU_NEW_FILE_SVG_PATH,
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
                        self.t(crate::i18n::TextKey::MenuNewFolder),
                        MENU_NEW_FOLDER_SVG_PATH,
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
                        self.t(crate::i18n::TextKey::Close),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.request_close_tab(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::CloseOthers),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.close_other_tabs(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::CloseLeft),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.close_left_tabs(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::CloseRight),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.close_right_tabs(&id, cx);
                        },
                    ));
                }
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::CloseSaved),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.close_saved_tabs(cx);
                    },
                ));
                items.push(self.render_context_menu_separator());
                {
                    let id = tab_id.clone();
                    let label = if is_pinned {
                        self.t(crate::i18n::TextKey::UnpinTab)
                    } else {
                        self.t(crate::i18n::TextKey::PinTab)
                    };
                    items.push(self.render_context_menu_item(
                        label,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.toggle_pin_tab(&id, cx);
                        },
                    ));
                }
                // Copy path if tab has a file
                if let Some(tab) = self.tabs.iter().find(|t| t.id == *tab_id) {
                    if let Some(path) = &tab.path {
                        let chat_path = path.clone();
                        items.push(self.render_context_menu_item(
                            ADD_FILE_TO_CHAT_LABEL,
                            false,
                            cx,
                            move |this, _, cx| {
                                this.close_context_menu(cx);
                                this.add_path_to_chat(&chat_path, cx);
                            },
                        ));
                        items.push(self.render_context_menu_separator());
                        let reveal_path = path.clone();
                        items.push(self.render_context_menu_item(
                            self.t(crate::i18n::TextKey::RevealInFileTree),
                            false,
                            cx,
                            move |this, _, cx| {
                                this.close_context_menu(cx);
                                this.reveal_path_in_sidebar(reveal_path.clone(), cx);
                            },
                        ));
                        {
                            let reveal_path = path.clone();
                            items.push(self.render_context_menu_item(
                                self.t(crate::i18n::TextKey::RevealInFinder),
                                false,
                                cx,
                                move |this, _, cx| {
                                    this.close_context_menu(cx);
                                    this.reveal_in_finder(&reveal_path);
                                },
                            ));
                        }
                        items.push(self.render_context_menu_separator());
                        if let Some(relative_path_text) = self.path_copy_text(path, true) {
                            items.push(self.render_context_menu_item(
                                self.t(crate::i18n::TextKey::CopyRelativePath),
                                false,
                                cx,
                                move |this, _, cx| {
                                    this.close_context_menu(cx);
                                    this.copy_text_to_clipboard(relative_path_text.clone(), cx);
                                },
                            ));
                        }
                        let path_text = path.display().to_string();
                        items.push(self.render_context_menu_item(
                            self.t(crate::i18n::TextKey::CopyPath),
                            false,
                            cx,
                            move |this, _, cx| {
                                this.close_context_menu(cx);
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
            ContextMenuKind::Terminal => {
                let mut items = Vec::new();
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::Copy),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        let _ = this.copy_chat_terminal_selection(cx);
                    },
                ));
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::Paste),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.paste_chat_terminal_from_clipboard(cx);
                    },
                ));
                items.push(self.render_context_menu_item(
                    TERMINAL_SELECT_ALL_LABEL,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        let _ = this.select_all_chat_terminal(cx);
                    },
                ));
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_sidebar_width_px),
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
                                    cx.stop_propagation();
                                    this.switch_workspace(ws_switch.clone(), cx);
                                }),
                            )
                            .into_any_element(),
                    );
                }
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::OpenFolder),
                    "assets/icons/filetree/all.svg",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.pick_workspace_folder(cx);
                    },
                ));
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::AppSettingsMenu),
                    "assets/icons/setting.svg",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
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
                        self.t(crate::i18n::TextKey::AddRowAbove),
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
                        self.t(crate::i18n::TextKey::AddRowBelow),
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
                            self.t(crate::i18n::TextKey::AddColumnLeft),
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
                            self.t(crate::i18n::TextKey::AddColumnRight),
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
                        self.t(crate::i18n::TextKey::DeleteRow),
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
                            self.t(crate::i18n::TextKey::DeleteColumn),
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
                            self.t(crate::i18n::TextKey::DeleteFrontMatter)
                        } else {
                            self.t(crate::i18n::TextKey::DeleteTable)
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
