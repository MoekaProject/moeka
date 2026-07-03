use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClipboardItem, Context, CursorStyle, FontWeight, Image, ImageFormat, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, SharedString, div, prelude::*, px, svg,
};

use crate::markdown::{BlockType, InlineKind, LineDecoration, raw_wiki_link_ranges};

use super::shortcut::{ShortcutRenderStyle, render_shortcut};
use super::*;

const ADD_FILE_TO_CHAT_LABEL: &str = "添加文件到聊天";
const ADD_SELECTION_TO_CHAT_LABEL: &str = "添加选区到聊天";
const COPY_PLAIN_TEXT_LABEL: &str = "复制纯文本";
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

#[derive(Clone, Copy)]
struct ContextMenuPanelLayout {
    left: f32,
    estimated_top: f32,
    max_height: f32,
    bottom_anchor: Option<f32>,
    overlay_width: f32,
    overlay_height: f32,
    margin: f32,
}

#[derive(Clone, Copy)]
enum FormatMenuHoverBehavior {
    None,
    ClearSubmenu,
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
    pub(crate) fn open_context_menu(
        &mut self,
        menu: ContextMenu,
        clear_sidebar_edit: bool,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(menu);
        self.context_submenu = None;
        self.context_submenu_aim = None;
        self.context_submenu_hovered = false;
        if clear_sidebar_edit {
            self.sidebar_edit = None;
        }
        self.context_menu_opacity = 0.0;
        self.animate_context_menu_opacity(1.0, false, cx);
        cx.notify();
    }

    fn animate_context_menu_opacity(
        &mut self,
        target: f32,
        close_when_done: bool,
        cx: &mut Context<Self>,
    ) {
        self.context_menu_opacity_target = target.clamp(0.0, 1.0);
        self.context_menu_opacity_epoch += 1;
        let epoch = self.context_menu_opacity_epoch;
        let start = self.context_menu_opacity.clamp(0.0, 1.0);
        let target = self.context_menu_opacity_target;
        let duration_ms = self.styles.context_menu.menu_opacity_animation_ms.max(1);

        self.context_menu_opacity_task = Some(cx.spawn(async move |this, cx| {
            let start_time = std::time::Instant::now();
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        if this.context_menu_opacity_epoch != epoch {
                            return true;
                        }

                        let progress = (start_time.elapsed().as_secs_f32()
                            / (duration_ms as f32 / 1000.0))
                            .clamp(0.0, 1.0);
                        let eased = super::ease_in_out_cubic(progress);
                        this.context_menu_opacity = start + (target - start) * eased;
                        if progress >= 1.0 {
                            this.context_menu_opacity = target;
                            this.context_menu_opacity_task = None;
                            if close_when_done {
                                this.finish_close_context_menu(cx);
                            }
                            cx.notify();
                            return true;
                        }

                        cx.notify();
                        false
                    })
                    .unwrap_or(true);

                if done {
                    return;
                }

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
        }));
    }

    fn finish_close_context_menu(&mut self, cx: &mut Context<Self>) {
        let had_menu = self.context_menu.take().is_some();
        let had_submenu = self.context_submenu.take().is_some();
        let had_submenu_aim = self.context_submenu_aim.take().is_some();
        let had_submenu_hovered = std::mem::replace(&mut self.context_submenu_hovered, false);
        if had_menu || had_submenu || had_submenu_aim || had_submenu_hovered {
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

    pub(crate) fn open_starred_sidebar_context_menu(
        &mut self,
        path: PathBuf,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::SidebarStarred { path },
            },
            true,
            cx,
        );
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

    fn copy_active_image_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current_file_path.clone() else {
            self.context_menu = None;
            cx.notify();
            return;
        };
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        let format = match ext.as_deref() {
            Some("png") => Some(ImageFormat::Png),
            Some("jpg" | "jpeg") => Some(ImageFormat::Jpeg),
            Some("webp") => Some(ImageFormat::Webp),
            Some("gif") => Some(ImageFormat::Gif),
            _ => None,
        };
        if let Some(format) = format
            && let Ok(bytes) = std::fs::read(&path)
        {
            self.context_menu = None;
            cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(format, bytes)));
            cx.notify();
            return;
        }
        if ext.as_deref() == Some("svg")
            && let Ok(svg) = std::fs::read_to_string(&path)
        {
            self.copy_text_to_clipboard(svg, cx);
            return;
        }
        self.copy_text_to_clipboard(path.to_string_lossy().into_owned(), cx);
    }

    fn context_menu_icon_path(&self, label: &str) -> Option<&'static str> {
        let delete_prefix = format!("{} (", self.t(crate::i18n::TextKey::Delete));
        let counted = |key: crate::i18n::TextKey| {
            let base = self.t(key);
            label == base || label.starts_with(&format!("{base} ("))
        };
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
            _ if counted(crate::i18n::TextKey::Cut) => Some(MENU_CLIP_SVG_PATH),
            _ if counted(crate::i18n::TextKey::Copy) => Some(MENU_COPY_SVG_PATH),
            _ if counted(crate::i18n::TextKey::Paste) => Some(MENU_PASTE_SVG_PATH),
            current if current == self.t(crate::i18n::TextKey::CollapseAllDirectories) => {
                Some(MENU_FOLD_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::ExpandAllDirectories) => {
                Some(MENU_EXPAND_SVG_PATH)
            }
            current if current == self.t(crate::i18n::TextKey::RevealInFinder) => {
                Some(MENU_LOCATE_SVG_PATH)
            }
            _ if counted(crate::i18n::TextKey::Star) => Some(MENU_STAR_SVG_PATH),
            _ if counted(crate::i18n::TextKey::Unstar) => Some(MENU_UNSTAR_SVG_PATH),
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
            FormatMenuAction::Underline => self.extra_format_action_label(action).to_string(),
            FormatMenuAction::InlineCode => self.t(crate::i18n::TextKey::InlineCode).to_string(),
            FormatMenuAction::Highlight => self.extra_format_action_label(action).to_string(),
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
            FormatMenuAction::DateTime => self.t(crate::i18n::TextKey::InsertDateTime).to_string(),
            FormatMenuAction::Superscript
            | FormatMenuAction::Subscript
            | FormatMenuAction::Footnote
            | FormatMenuAction::CalloutTip => self.extra_format_action_label(action).to_string(),
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

    fn extra_format_action_label(&self, action: FormatMenuAction) -> &'static str {
        match self.ui_language {
            UiLanguage::ChineseSimplified => match action {
                FormatMenuAction::Highlight => "高亮",
                FormatMenuAction::Underline => "下划线",
                FormatMenuAction::Superscript => "上标",
                FormatMenuAction::Subscript => "下标",
                FormatMenuAction::Footnote => "脚注",
                FormatMenuAction::CalloutTip => "提示块",
                _ => "",
            },
            UiLanguage::ChineseTraditional => match action {
                FormatMenuAction::Highlight => "高亮",
                FormatMenuAction::Underline => "底線",
                FormatMenuAction::Superscript => "上標",
                FormatMenuAction::Subscript => "下標",
                FormatMenuAction::Footnote => "腳註",
                FormatMenuAction::CalloutTip => "提示區塊",
                _ => "",
            },
            _ => match action {
                FormatMenuAction::Highlight => "Highlight",
                FormatMenuAction::Underline => "Underline",
                FormatMenuAction::Superscript => "Superscript",
                FormatMenuAction::Subscript => "Subscript",
                FormatMenuAction::Footnote => "Footnote",
                FormatMenuAction::CalloutTip => "Tip Callout",
                _ => "",
            },
        }
    }

    fn more_formats_menu_label(&self) -> &'static str {
        match self.ui_language {
            UiLanguage::ChineseSimplified | UiLanguage::ChineseTraditional => "更多格式",
            _ => "More Formats",
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

    fn sidebar_counted_label(&self, label: &str, count: usize) -> String {
        if count > 1 {
            crate::i18n::action_items_label(self.ui_language, label, count)
        } else {
            label.to_string()
        }
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
        let mut update_best_inline =
            |full_start: usize, full_end: usize, action: FormatMenuAction| {
                if cursor_col >= full_start && cursor_col <= full_end {
                    let span_len = full_end.saturating_sub(full_start);
                    if best_inline
                        .as_ref()
                        .is_none_or(|(prev_len, _)| span_len < *prev_len)
                    {
                        best_inline = Some((span_len, action));
                    }
                }
            };
        for span in &decoration.inline_spans {
            let marker_len = match &span.kind {
                InlineKind::Bold => 2,
                InlineKind::Italic => 1,
                InlineKind::Strikethrough => 2,
                InlineKind::Underline => 2,
                InlineKind::InlineCode => 1,
                InlineKind::Link => 0,
                InlineKind::Image => 0,
            };
            let full_start = span.range.start.saturating_sub(marker_len);
            let full_end = span.range.end + marker_len;
            let action = match &span.kind {
                InlineKind::Bold => Some(FormatMenuAction::Bold),
                InlineKind::Italic => Some(FormatMenuAction::Italic),
                InlineKind::Strikethrough => Some(FormatMenuAction::Strikethrough),
                InlineKind::Underline => Some(FormatMenuAction::Underline),
                InlineKind::InlineCode => Some(FormatMenuAction::InlineCode),
                InlineKind::Link => Some(FormatMenuAction::Link),
                InlineKind::Image => Some(FormatMenuAction::Image),
            };
            if let Some(action) = action {
                update_best_inline(full_start, full_end, action);
            }
        }

        for span in &decoration.highlight_spans {
            update_best_inline(
                span.start.saturating_sub(2),
                span.end.saturating_add(2),
                FormatMenuAction::Highlight,
            );
        }

        for span in &decoration.superscript_spans {
            update_best_inline(
                span.start.saturating_sub(1),
                span.end.saturating_add(1),
                FormatMenuAction::Superscript,
            );
        }

        for span in &decoration.subscript_spans {
            update_best_inline(
                span.start.saturating_sub(1),
                span.end.saturating_add(1),
                FormatMenuAction::Subscript,
            );
        }

        for footnote_reference in &decoration.footnote_references {
            update_best_inline(
                footnote_reference.full_range.start,
                footnote_reference.full_range.end,
                FormatMenuAction::Footnote,
            );
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
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::Sidebar { path, is_dir },
            },
            true,
            cx,
        );
    }

    pub(crate) fn open_format_context_menu(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::Format,
            },
            true,
            cx,
        );
    }

    fn screen_position_for_offset(&self, offset: usize) -> Point<Pixels> {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let origin_y: f32 = self.content_origin.y.into();
        // Top of the line's text content (after any block padding).
        let text_top = origin_y + self.text_y_offset_for_line(line) - self.scroll_offset;
        let (x, row_top_within, row_height) =
            if let Some(Some(info)) = self.line_display_info.get(line) {
                let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
                let pos = info
                    .wrapped_line
                    .position_for_index(disp_col, gpui::px(info.line_height))
                    .unwrap_or_default();
                (
                    info.content_x + f32::from(pos.x),
                    f32::from(pos.y),
                    info.line_height,
                )
            } else {
                (
                    self.last_content_left + col as f32 * self.base_font_size() * 0.6,
                    0.0,
                    self.visual_height_for_line(line),
                )
            };
        // Anchor to the cursor's specific visual row (handles wrapped lines and
        // block padding precisely) rather than the whole line block, so the menu
        // sits exactly below/above the syntax being edited.
        let row_top = text_top + row_top_within;
        gpui::point(px(x), px(row_top + row_height))
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
        self.open_context_menu(
            ContextMenu {
                position: pos,
                kind: ContextMenuKind::WikiLink,
            },
            true,
            cx,
        );
        if self.wiki_link_query.trim().is_empty() {
            self.run_wiki_link_recent_search(cx);
        } else {
            self.run_wiki_link_search(cx);
        }
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
        // Clear stale results immediately so the menu shows a "searching" state
        // instead of flashing the previous query's matches.
        self.wiki_link_search_results.clear();

        let root = self.sidebar_root.clone();
        let query = self.wiki_link_query.clone();
        if !root.exists() || query.trim().is_empty() {
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
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::Tab { tab_id },
            },
            true,
            cx,
        );
    }

    pub(crate) fn open_terminal_context_menu(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::Terminal,
            },
            true,
            cx,
        );
    }

    pub(crate) fn open_chat_tab_context_menu(
        &mut self,
        tab_id: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::ChatTab { tab_id },
            },
            true,
            cx,
        );
    }

    pub(crate) fn open_chat_tab_create_context_menu(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::ChatTabCreate,
            },
            true,
            cx,
        );
    }

    pub(crate) fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.is_none() {
            self.finish_close_context_menu(cx);
            self.context_menu_opacity = 0.0;
            self.context_menu_opacity_target = 0.0;
            return;
        }
        self.animate_context_menu_opacity(0.0, true, cx);
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
        if self.active_tab_is_read_only() {
            if action == FormatMenuAction::Copy {
                self.copy_selection_to_clipboard(cx);
            }
            self.close_context_menu(cx);
            return;
        }
        if let Some(start) = self.slash_menu_start.take() {
            let end = self.cursor.offset;
            if end > start {
                self.buffer.replace(start..end, "");
                self.cursor.move_to(start, &self.buffer);
                self.reparse();
            }
            self.slash_filter.clear();
        }
        self.close_context_menu(cx);
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
            FormatMenuAction::Underline => {
                self.checkpoint();
                self.apply_wrap("++", "++");
                self.pause_blinking(cx);
            }
            FormatMenuAction::InlineCode => {
                self.checkpoint();
                self.apply_wrap("`", "`");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Highlight => {
                self.checkpoint();
                self.apply_wrap("==", "==");
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
                self.normalize_ordered_lists_near_all_cursors(true);
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
            FormatMenuAction::DateTime => {
                self.checkpoint();
                let range = self.cursor.selection_range();
                let formatted = chrono::Local::now()
                    .format(self.app_settings.date_time_format())
                    .to_string();
                self.buffer.replace(range.clone(), &formatted);
                self.cursor
                    .move_to(range.start + formatted.len(), &self.buffer);
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
            }
            FormatMenuAction::Superscript => {
                self.checkpoint();
                self.apply_wrap("^", "^");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Subscript => {
                self.checkpoint();
                self.apply_wrap("~", "~");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Footnote => {
                self.checkpoint();
                let range = self.cursor.selection_range();
                if range.is_empty() {
                    let snippet = "[^note]";
                    self.buffer.replace(range.clone(), snippet);
                    let select_start = range.start + 2;
                    let select_end = select_start + "note".len();
                    self.cursor.anchor = select_start;
                    self.cursor.offset = select_end;
                } else {
                    let selected_text = self.buffer.text()[range.clone()].to_string();
                    let replacement = format!("[^{}]", selected_text);
                    self.buffer.replace(range.clone(), &replacement);
                    self.cursor
                        .move_to(range.start + replacement.len(), &self.buffer);
                }
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
            }
            FormatMenuAction::CalloutTip => {
                self.checkpoint();
                self.toggle_current_line_prefix("> [!TIP] ");
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
            (
                self.format_action_label(FormatMenuAction::DateTime),
                "date time now today timestamp calendar",
                FormatMenuAction::DateTime,
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
            ms.menu_item_primary_text_color
        };
        let hover_bg = ms.menu_item_hover_bg;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        let (icon_col_w, gap) = menu_icon_column_metrics(
            ms.menu_format_icon_size_px,
            wss.ws_menu_item_check_size_px,
            wss.ws_menu_item_action_icon_gap_px,
        );
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding_x = self.scaled(ms.menu_item_padding_x_px);
        let font_size = self.scaled(ms.menu_item_font_size_px);
        let icon_col_w = self.scaled(icon_col_w);
        let gap = self.scaled(gap);
        div()
            .id(item_id)
            .w_full()
            .h(px(item_height))
            .px(px(padding_x))
            .flex()
            .items_center()
            .gap(px(gap))
            .text_size(px(font_size))
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
                    this.close_context_menu(cx);
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
            ms.menu_item_primary_text_color
        };
        let hover_bg = ms.menu_item_hover_bg;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        let (icon_col_w, gap) = menu_icon_column_metrics(
            ms.menu_format_icon_size_px,
            wss.ws_menu_item_check_size_px,
            wss.ws_menu_item_action_icon_gap_px,
        );
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding_x = self.scaled(ms.menu_item_padding_x_px);
        let font_size = self.scaled(ms.menu_item_font_size_px);
        let icon_col_w = self.scaled(icon_col_w);
        let gap = self.scaled(gap);
        div()
            .id(item_id)
            .w_full()
            .h(px(item_height))
            .px(px(padding_x))
            .flex()
            .items_center()
            .gap(px(gap))
            .text_size(px(font_size))
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
                    this.close_context_menu(cx);
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_context_menu_item_with_icon_and_check(
        &self,
        label: &str,
        icon_path: &'static str,
        is_active: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let wss = &self.styles.workspace_switcher;
        let hover_bg = ms.menu_item_hover_bg;
        let text_color = ms.menu_item_primary_text_color;
        let check_size = self.scaled(wss.ws_menu_item_check_size_px);
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        let gap = self.scaled(wss.ws_menu_item_action_icon_gap_px);
        let icon_column_width = check_size.max(icon_size);

        div()
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .px(px(self.scaled(ms.menu_item_padding_x_px)))
            .flex()
            .items_center()
            .gap(px(gap))
            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
            .text_color(text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .w(px(check_size))
                    .h(px(check_size))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .when(is_active, |this| {
                        this.child(
                            svg()
                                .path(CHECK_SVG_PATH)
                                .size(px(check_size))
                                .text_color(wss.ws_menu_item_check_color),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(icon_column_width))
                    .h(px(icon_column_width))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(icon_size))
                            .text_color(ms.menu_format_icon_color),
                    ),
            )
            .child(label.to_string())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    this.close_context_menu(cx);
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
            .text_color(ms.menu_item_primary_text_color)
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
                    this.close_context_menu(cx);
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
        self.render_format_context_menu_item_with_hover_behavior(
            label,
            icon_path,
            shortcut,
            is_selected,
            is_active,
            FormatMenuHoverBehavior::None,
            cx,
            handler,
        )
    }

    fn render_format_context_menu_item_with_hover_behavior(
        &self,
        label: &str,
        icon_path: Option<&str>,
        shortcut: Option<&str>,
        is_selected: bool,
        is_active: bool,
        hover_behavior: FormatMenuHoverBehavior,
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
            ms.menu_item_primary_text_color
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
            font_family: ms.menu_shortcut_font_family,
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
            .when(
                !matches!(hover_behavior, FormatMenuHoverBehavior::None),
                |this| {
                    this.on_mouse_move(cx.listener(
                        move |this, event: &MouseMoveEvent, _window, cx| match hover_behavior {
                            FormatMenuHoverBehavior::None => {}
                            FormatMenuHoverBehavior::ClearSubmenu => {
                                if this.should_preserve_format_submenu(event.position) {
                                    return;
                                }
                                if this.context_submenu.take().is_some() {
                                    this.context_submenu_aim = None;
                                    this.context_submenu_hovered = false;
                                    cx.notify();
                                }
                            }
                        },
                    ))
                },
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_format_submenu_trigger_item(
        &self,
        label: &str,
        icon_path: Option<&'static str>,
        submenu_kind: ContextMenuKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let is_open = self.context_submenu.as_ref() == Some(&submenu_kind);
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        let icon_gap = self.scaled(ms.menu_format_icon_gap_px);
        let arrow_size = (icon_size - self.scaled(2.0)).max(self.scaled(10.0));
        let hover_bg = ms.menu_item_hover_bg;
        let text_color = ms.menu_item_primary_text_color;
        let bg = if is_open {
            ms.menu_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let icon_shared = icon_path.map(SharedString::from);

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
            .hover(move |style| style.bg(hover_bg))
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
                                    svg()
                                        .path(path)
                                        .size(px(icon_size))
                                        .text_color(ms.menu_format_icon_color),
                                )
                            }),
                    )
                    .child(label.to_string()),
            )
            .child(
                svg()
                    .path("assets/icons/right.svg")
                    .size(px(arrow_size))
                    .text_color(ms.menu_shortcut_text_color),
            )
            .on_mouse_move({
                let submenu_kind = submenu_kind.clone();
                cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                    this.context_submenu_aim = Some(ContextSubmenuAim {
                        origin: event.position,
                    });
                    this.context_submenu_hovered = false;
                    if this.context_submenu.as_ref() != Some(&submenu_kind) {
                        this.context_submenu = Some(submenu_kind.clone());
                        cx.notify();
                    }
                })
            })
            .on_mouse_down(MouseButton::Left, {
                let submenu_kind = submenu_kind.clone();
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.context_submenu_aim = Some(ContextSubmenuAim {
                        origin: event.position,
                    });
                    this.context_submenu_hovered = false;
                    this.context_submenu = Some(submenu_kind.clone());
                    cx.notify();
                })
            })
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
            if has_clipboard {
                let pd = parent_dir.clone();
                let paste_count = self
                    .file_clipboard
                    .as_ref()
                    .map(|clipboard| {
                        Self::filter_nested_paths(&clipboard.paths.iter().cloned().collect()).len()
                    })
                    .unwrap_or(0);
                let paste_label =
                    self.sidebar_counted_label(self.t(crate::i18n::TextKey::Paste), paste_count);
                items.push(self.render_context_menu_item(
                    &paste_label,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.paste_into_dir(pd.clone(), cx);
                    },
                ));
                items.push(self.render_context_menu_separator());
            }
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
                let chat_paths = self.sidebar_context_paths(&path);
                items.push(self.render_context_menu_item(
                    ADD_FILE_TO_CHAT_LABEL,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.add_paths_to_chat(&chat_paths, cx);
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

        let context_paths = self.sidebar_context_paths(&path);
        let context_paths = Self::filter_nested_paths(&context_paths.into_iter().collect());
        let context_count = context_paths.len();

        if !is_dir {
            let starred_paths: Vec<PathBuf> = context_paths
                .iter()
                .filter(|path| path.is_file())
                .cloned()
                .collect();
            let is_starred = starred_paths
                .iter()
                .all(|path| self.app_settings.is_starred_file(path));
            let label = if is_starred {
                self.t(crate::i18n::TextKey::Unstar)
            } else {
                self.t(crate::i18n::TextKey::Star)
            };
            let star_label = self.sidebar_counted_label(label, starred_paths.len());
            items.push(self.render_context_menu_item(
                &star_label,
                false,
                cx,
                move |this, _, cx| {
                    for starred_path in &starred_paths {
                        let normalized = Self::normalize_sidebar_path(starred_path);
                        if is_starred {
                            this.app_settings.remove_starred_file(&normalized);
                        } else {
                            this.app_settings.add_starred_file(normalized);
                            this.sidebar_starred_collapsed = false;
                        }
                    }
                    this.app_settings.save();
                    this.context_menu = None;
                    cx.notify();
                },
            ));
            items.push(self.render_context_menu_separator());
        }
        {
            let chat_paths = context_paths.clone();
            items.push(self.render_context_menu_item(
                ADD_FILE_TO_CHAT_LABEL,
                false,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.add_paths_to_chat(&chat_paths, cx);
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
            let delete_label = if context_count > 1 {
                crate::i18n::delete_items_label(self.ui_language, context_count)
            } else {
                self.t(crate::i18n::TextKey::Delete).to_string()
            };
            let delete_paths = context_paths.clone();
            items.push(self.render_context_menu_item(
                &delete_label,
                true,
                cx,
                move |this, _, cx| {
                    this.show_sidebar_delete_dialog(delete_paths.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        {
            let cut_paths = context_paths
                .iter()
                .map(|path| Self::normalize_sidebar_path(path))
                .collect::<Vec<_>>();
            let copy_paths = cut_paths.clone();
            let cut_label =
                self.sidebar_counted_label(self.t(crate::i18n::TextKey::Cut), context_count);
            let copy_label =
                self.sidebar_counted_label(self.t(crate::i18n::TextKey::Copy), context_count);
            items.push(
                self.render_context_menu_item(&cut_label, false, cx, move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: cut_paths.clone(),
                        is_cut: true,
                    });
                    this.context_menu = None;
                    cx.notify();
                }),
            );
            items.push(self.render_context_menu_item(
                &copy_label,
                false,
                cx,
                move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: copy_paths.clone(),
                        is_cut: false,
                    });
                    this.context_menu = None;
                    cx.notify();
                },
            ));
        }
        if has_clipboard {
            let pd = parent_dir.clone();
            let paste_count = self
                .file_clipboard
                .as_ref()
                .map(|clipboard| {
                    Self::filter_nested_paths(&clipboard.paths.iter().cloned().collect()).len()
                })
                .unwrap_or(0);
            let paste_label =
                self.sidebar_counted_label(self.t(crate::i18n::TextKey::Paste), paste_count);
            items.push(self.render_context_menu_item(
                &paste_label,
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
            let context_paths = self.sidebar_context_paths(&path);
            let context_count = context_paths.len();
            let unstar_label =
                self.sidebar_counted_label(self.t(crate::i18n::TextKey::Unstar), context_count);
            items.push(self.render_context_menu_item(
                &unstar_label,
                false,
                cx,
                move |this, _, cx| {
                    let mut changed = false;
                    for starred_path in &context_paths {
                        let normalized = Self::normalize_sidebar_path(starred_path);
                        changed |= this.app_settings.remove_starred_file(&normalized);
                    }
                    if changed {
                        this.app_settings.save();
                    }
                    this.context_menu = None;
                    cx.notify();
                },
            ));
        }
        items.push(self.render_context_menu_separator());

        if self.selected_sidebar_paths.len() <= 1 {
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
            let context_paths = self.sidebar_context_paths(&path);
            let context_count = context_paths.len();
            let delete_label = if context_count > 1 {
                crate::i18n::delete_items_label(self.ui_language, context_count)
            } else {
                self.t(crate::i18n::TextKey::Delete).to_string()
            };
            items.push(self.render_context_menu_item(
                &delete_label,
                true,
                cx,
                move |this, _, cx| {
                    this.show_sidebar_delete_dialog(context_paths.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());

        {
            let context_paths = self.sidebar_context_paths(&path);
            let context_count = context_paths.len();
            let cut_paths = context_paths
                .iter()
                .map(|path| Self::normalize_sidebar_path(path))
                .collect::<Vec<_>>();
            let copy_paths = cut_paths.clone();
            let cut_label =
                self.sidebar_counted_label(self.t(crate::i18n::TextKey::Cut), context_count);
            let copy_label =
                self.sidebar_counted_label(self.t(crate::i18n::TextKey::Copy), context_count);
            items.push(
                self.render_context_menu_item(&cut_label, false, cx, move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: cut_paths.clone(),
                        is_cut: true,
                    });
                    this.context_menu = None;
                    cx.notify();
                }),
            );
            items.push(self.render_context_menu_item(
                &copy_label,
                false,
                cx,
                move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: copy_paths.clone(),
                        is_cut: false,
                    });
                    this.context_menu = None;
                    cx.notify();
                },
            ));
        }
        if self.file_clipboard.is_some() {
            let parent_dir = path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.sidebar_root.clone());
            let paste_count = self
                .file_clipboard
                .as_ref()
                .map(|clipboard| {
                    Self::filter_nested_paths(&clipboard.paths.iter().cloned().collect()).len()
                })
                .unwrap_or(0);
            let paste_label =
                self.sidebar_counted_label(self.t(crate::i18n::TextKey::Paste), paste_count);
            items.push(self.render_context_menu_item(
                &paste_label,
                false,
                cx,
                move |this, _, cx| {
                    this.paste_into_dir(parent_dir.clone(), cx);
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

        {
            let reveal_paths = self.sidebar_context_paths(&path);
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::RevealInFinder),
                false,
                cx,
                move |this, _, _cx| {
                    this.reveal_paths_in_finder(&reveal_paths);
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
            for (label, shortcut, action) in [
                (
                    self.format_action_label(FormatMenuAction::Cut),
                    Some("⌘X"),
                    FormatMenuAction::Cut,
                ),
                (
                    self.format_action_label(FormatMenuAction::Copy),
                    Some("⌘C"),
                    FormatMenuAction::Copy,
                ),
                (
                    self.format_action_label(FormatMenuAction::Paste),
                    Some("⌘V"),
                    FormatMenuAction::Paste,
                ),
            ] {
                items.push(self.render_format_context_menu_item_with_hover_behavior(
                    &label,
                    None,
                    shortcut,
                    false,
                    false,
                    FormatMenuHoverBehavior::ClearSubmenu,
                    cx,
                    move |this, _, cx| {
                        this.execute_format_menu_action(action, cx);
                    },
                ));
            }

            // Text/code editors only expose Cut/Copy/Paste; markdown-specific
            // actions below do not apply to plain text or source files.
            if self.active_tab_is_non_markdown_text() {
                return items;
            }

            // Copy the document's text with all markdown syntax markers removed.
            // Operates on the selection when present (still excluding any syntax
            // characters inside it), otherwise on the whole document.
            items.push(self.render_format_context_menu_item_with_hover_behavior(
                COPY_PLAIN_TEXT_LABEL,
                Some(MENU_COPY_SVG_PATH),
                None,
                false,
                false,
                FormatMenuHoverBehavior::ClearSubmenu,
                cx,
                move |this, _, cx| {
                    this.copy_plain_text_to_clipboard(cx);
                },
            ));

            if let Some(current_path) = self.current_file_path.clone() {
                items.push(self.render_context_menu_separator());

                let chat_path = current_path.clone();
                items.push(self.render_format_context_menu_item_with_hover_behavior(
                    ADD_FILE_TO_CHAT_LABEL,
                    Some(MENU_AT_SVG_PATH),
                    None,
                    false,
                    false,
                    FormatMenuHoverBehavior::ClearSubmenu,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.add_path_to_chat(&chat_path, cx);
                    },
                ));

                let selection_path = current_path.clone();
                let selection_range = self.cursor.selection_range();
                items.push(self.render_format_context_menu_item_with_hover_behavior(
                    ADD_SELECTION_TO_CHAT_LABEL,
                    Some(CHAT_SVG_PATH),
                    None,
                    false,
                    false,
                    FormatMenuHoverBehavior::ClearSubmenu,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.add_selection_to_chat(&selection_path, selection_range.clone(), cx);
                    },
                ));
            }

            items.push(self.render_context_menu_separator());
            items.push(self.render_format_submenu_trigger_item(
                &self.t(crate::i18n::TextKey::Heading).to_string(),
                Some(HEADING_1_SVG_PATH),
                ContextMenuKind::FormatHeading,
                cx,
            ));

            for (label, icon, shortcut, action) in [
                (
                    self.format_action_label(FormatMenuAction::Bold),
                    Some(FMT_BOLD_SVG),
                    Some("⌘B"),
                    FormatMenuAction::Bold,
                ),
                (
                    self.format_action_label(FormatMenuAction::Italic),
                    Some(FMT_ITALIC_SVG),
                    Some("⌘I"),
                    FormatMenuAction::Italic,
                ),
                (
                    self.format_action_label(FormatMenuAction::Strikethrough),
                    Some(FMT_STRIKETHROUGH_SVG),
                    Some("⌘⇧S"),
                    FormatMenuAction::Strikethrough,
                ),
                (
                    self.format_action_label(FormatMenuAction::Underline),
                    Some(FMT_UNDERLINE_SVG),
                    Some("++"),
                    FormatMenuAction::Underline,
                ),
                (
                    self.format_action_label(FormatMenuAction::InlineCode),
                    Some(FMT_INLINE_CODE_SVG),
                    Some("⌘E"),
                    FormatMenuAction::InlineCode,
                ),
            ] {
                let is_active = active_actions.contains(&action);
                items.push(self.render_format_context_menu_item_with_hover_behavior(
                    &label,
                    icon,
                    shortcut,
                    false,
                    is_active,
                    FormatMenuHoverBehavior::ClearSubmenu,
                    cx,
                    move |this, _, cx| {
                        this.execute_format_menu_action(action, cx);
                    },
                ));
            }

            items.push(self.render_context_menu_separator());

            for (label, icon, shortcut, action) in [
                (
                    self.format_action_label(FormatMenuAction::Todo),
                    Some(FMT_TODO_SVG),
                    Some("⌘⌥T"),
                    FormatMenuAction::Todo,
                ),
                (
                    self.format_action_label(FormatMenuAction::Link),
                    Some(FMT_LINK_SVG),
                    Some("⌘K"),
                    FormatMenuAction::Link,
                ),
            ] {
                let is_active = active_actions.contains(&action);
                items.push(self.render_format_context_menu_item_with_hover_behavior(
                    &label,
                    icon,
                    shortcut,
                    false,
                    is_active,
                    FormatMenuHoverBehavior::ClearSubmenu,
                    cx,
                    move |this, _, cx| {
                        this.execute_format_menu_action(action, cx);
                    },
                ));
            }

            items.push(self.render_context_menu_separator());
            items.push(self.render_format_submenu_trigger_item(
                self.more_formats_menu_label(),
                None,
                ContextMenuKind::FormatMore,
                cx,
            ));
            return items;
        }

        let slash_items = [
            (
                self.format_action_label(FormatMenuAction::Todo),
                "todo task checkbox",
                Some(FMT_TODO_SVG),
                Some("⌘⌥T"),
                FormatMenuAction::Todo,
            ),
            (
                self.format_action_label(FormatMenuAction::CodeBlock),
                "code block",
                Some(FMT_CODE_SVG),
                Some("⌘⌥C"),
                FormatMenuAction::CodeBlock,
            ),
            (
                self.format_action_label(FormatMenuAction::Table),
                "table grid 2x2",
                Some(FMT_TABLE_SVG),
                Some("2×2"),
                FormatMenuAction::Table,
            ),
            (
                self.format_action_label(FormatMenuAction::Link),
                "link url",
                Some(FMT_LINK_SVG),
                Some("⌘K"),
                FormatMenuAction::Link,
            ),
            (
                self.format_action_label(FormatMenuAction::DateTime),
                "date time now today timestamp calendar",
                Some(FMT_DATE_TIME_SVG),
                Some("now"),
                FormatMenuAction::DateTime,
            ),
            (
                self.format_action_label(FormatMenuAction::CalloutTip),
                "tip callout alert admonition quote",
                Some(FMT_CALLOUT_TIP_SVG),
                Some("> [!TIP]"),
                FormatMenuAction::CalloutTip,
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
        ];

        for (label, filter_text, icon, shortcut, action) in slash_items {
            if matches_filter(&label, filter_text) {
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

        items
    }

    fn render_format_more_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let active_actions = self.active_format_actions();
        let mut items = Vec::new();

        let block_actions = [
            (
                self.format_action_label(FormatMenuAction::CodeBlock),
                Some(FMT_CODE_SVG),
                Some("⌘⌥C"),
                FormatMenuAction::CodeBlock,
            ),
            (
                self.format_action_label(FormatMenuAction::HorizontalRule),
                Some(FMT_SPLIT_SVG),
                Some("---"),
                FormatMenuAction::HorizontalRule,
            ),
            (
                self.format_action_label(FormatMenuAction::Quote),
                Some(FMT_QUOTE_SVG),
                Some("⌘⌥Q"),
                FormatMenuAction::Quote,
            ),
            (
                self.format_action_label(FormatMenuAction::UnorderedList),
                Some(FMT_LIST_SVG),
                Some("⌘⌥U"),
                FormatMenuAction::UnorderedList,
            ),
            (
                self.format_action_label(FormatMenuAction::OrderedList),
                Some(FMT_ORDER_LIST_SVG),
                Some("⌘⌥O"),
                FormatMenuAction::OrderedList,
            ),
        ];

        for (label, icon, shortcut, action) in block_actions {
            let is_active = active_actions.contains(&action);
            items.push(self.render_format_context_menu_item(
                &label,
                icon,
                shortcut,
                false,
                is_active,
                cx,
                move |this, _, cx| {
                    this.execute_format_menu_action(action, cx);
                },
            ));
        }

        items.push(self.render_context_menu_separator());

        for (label, icon, shortcut, action) in [
            (
                self.format_action_label(FormatMenuAction::WikiLink),
                Some(FMT_WIKI_LINK_SVG),
                Some("[["),
                FormatMenuAction::WikiLink,
            ),
            (
                self.format_action_label(FormatMenuAction::Image),
                Some(FMT_IMAGE_SVG),
                Some("⌘⇧I"),
                FormatMenuAction::Image,
            ),
            (
                self.format_action_label(FormatMenuAction::Table),
                Some(FMT_TABLE_SVG),
                Some("2×2"),
                FormatMenuAction::Table,
            ),
        ] {
            let is_active = active_actions.contains(&action);
            items.push(self.render_format_context_menu_item(
                &label,
                icon,
                shortcut,
                false,
                is_active,
                cx,
                move |this, _, cx| {
                    this.execute_format_menu_action(action, cx);
                },
            ));
        }

        items.push(self.render_context_menu_separator());

        for (label, icon, shortcut, action) in [
            (
                self.format_action_label(FormatMenuAction::Superscript),
                Some(FMT_SUPERSCRIPT_SVG),
                Some("^text^"),
                FormatMenuAction::Superscript,
            ),
            (
                self.format_action_label(FormatMenuAction::Subscript),
                Some(FMT_SUBSCRIPT_SVG),
                Some("~text~"),
                FormatMenuAction::Subscript,
            ),
            (
                self.format_action_label(FormatMenuAction::Footnote),
                Some(FMT_FOOTNOTE_SVG),
                Some("[^note]"),
                FormatMenuAction::Footnote,
            ),
            (
                self.format_action_label(FormatMenuAction::Underline),
                Some(FMT_UNDERLINE_SVG),
                Some("++text++"),
                FormatMenuAction::Underline,
            ),
            (
                self.format_action_label(FormatMenuAction::CalloutTip),
                Some(FMT_CALLOUT_TIP_SVG),
                Some("> [!TIP]"),
                FormatMenuAction::CalloutTip,
            ),
            (
                self.format_action_label(FormatMenuAction::Highlight),
                Some(FMT_HIGHLIGHT_SVG),
                Some("==text=="),
                FormatMenuAction::Highlight,
            ),
        ] {
            let is_active = active_actions.contains(&action);
            items.push(self.render_format_context_menu_item(
                &label,
                icon,
                shortcut,
                false,
                is_active,
                cx,
                move |this, _, cx| {
                    this.execute_format_menu_action(action, cx);
                },
            ));
        }

        items
    }

    fn render_wiki_link_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut items: Vec<AnyElement> = Vec::new();
        let ms = &self.styles.context_menu;

        let has_query = !self.wiki_link_query.trim().is_empty();
        let show_loading = if has_query {
            self.wiki_link_search_task.is_some() && self.wiki_link_search_results.is_empty()
        } else {
            self.wiki_link_recent_task.is_some() && self.wiki_link_recent_results.is_empty()
        };
        if show_loading {
            let label = self.t(crate::i18n::TextKey::Searching).to_string();
            items.push(self.render_context_menu_item(&label, false, cx, |_this, _, _cx| {}));
            return items;
        }

        let search_items = if has_query {
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
            let directory_path = self
                .relative_path_for_path(&path)
                .map(std::path::PathBuf::from)
                .and_then(|relative| {
                    relative
                        .parent()
                        .map(|parent| parent.to_string_lossy().into_owned())
                })
                .filter(|parent| !parent.is_empty() && parent != ".");
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
                    .text_color(ms.menu_item_primary_text_color)
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
                        ms.menu_item_primary_text_color,
                        self.styles.quick_open.quick_open_result_match_text_color,
                    ))
                    .when_some(directory_path, |this, directory_path| {
                        this.child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(self.scaled(ms.menu_item_font_size_px)))
                                .text_color(ms.menu_item_text_color)
                                .opacity(0.5)
                                .child(directory_path),
                        )
                    })
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
            .flex_none()
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
                .flex_none()
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

    fn estimated_context_menu_height(&self, item_count: usize) -> f32 {
        let ms = &self.styles.context_menu;
        item_count as f32 * self.scaled(ms.menu_item_height_px)
            + self.scaled(ms.menu_padding_y_px) * 2.0
    }

    /// Height of the anchor row for input-anchored menus (wiki-link and slash).
    /// `menu.position` for these menus is the bottom of the cursor's visual row,
    /// so when the menu opens upward it must be anchored above the top of that row.
    fn context_menu_anchor_height(&self, kind: &ContextMenuKind) -> f32 {
        let input_anchored = matches!(kind, ContextMenuKind::WikiLink)
            || (matches!(kind, ContextMenuKind::Format) && self.slash_menu_start.is_some());
        if !input_anchored {
            return 0.0;
        }
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        if let Some(Some(info)) = self.line_display_info.get(line) {
            info.line_height
        } else {
            self.visual_height_for_line(line)
        }
    }

    fn context_menu_panel_layout(
        &self,
        position: Point<Pixels>,
        width: f32,
        estimated_menu_height: f32,
    ) -> ContextMenuPanelLayout {
        self.context_menu_panel_layout_with_anchor(position, width, estimated_menu_height, 0.0)
    }

    fn context_menu_panel_layout_with_anchor(
        &self,
        position: Point<Pixels>,
        width: f32,
        estimated_menu_height: f32,
        anchor_height: f32,
    ) -> ContextMenuPanelLayout {
        let ms = &self.styles.context_menu;
        let border_w = self.styles.window.window_border_width_px;
        let overlay_w = self.last_window_width - 2.0 * border_w;
        let overlay_h = self.last_window_height - 2.0 * border_w;
        let pos_x = (f32::from(position.x) - border_w).clamp(0.0, overlay_w);
        let pos_y = (f32::from(position.y) - border_w).clamp(0.0, overlay_h);
        let margin = self.scaled(ms.menu_max_height_margin_px).max(4.0);
        // For input-anchored menus, `pos_y` is the bottom of the anchor line; the
        // top of that line is `pos_y - anchor_height`. Upward-opening menus must
        // be anchored above that top edge so they don't cover the syntax line.
        let anchor_top_y = (pos_y - anchor_height).max(0.0);

        let space_right = overlay_w - pos_x - margin;
        let space_left = pos_x - margin;
        let space_below = overlay_h - pos_y - margin;
        let space_above = anchor_top_y - margin;

        let open_right = space_right >= width || space_right >= space_left;
        let open_down = space_below >= estimated_menu_height || space_below >= space_above;

        let left = if open_right {
            pos_x
        } else {
            (pos_x - width).max(margin)
        };
        let max_height = if open_down {
            (overlay_h - pos_y - margin).max(self.scaled(100.0))
        } else {
            (anchor_top_y - margin).max(self.scaled(100.0))
        };

        ContextMenuPanelLayout {
            left,
            estimated_top: if open_down {
                pos_y
            } else {
                (anchor_top_y - estimated_menu_height).max(margin)
            },
            max_height,
            bottom_anchor: if open_down {
                None
            } else {
                Some(overlay_h - anchor_top_y)
            },
            overlay_width: overlay_w,
            overlay_height: overlay_h,
            margin,
        }
    }

    fn context_menu_layout_top(&self, layout: ContextMenuPanelLayout, panel_height: f32) -> f32 {
        if let Some(bottom_anchor) = layout.bottom_anchor {
            layout.overlay_height - bottom_anchor - panel_height
        } else {
            layout.estimated_top
        }
    }

    fn context_menu_overlay_coordinates(&self, position: Point<Pixels>) -> (f32, f32) {
        let border_w = self.styles.window.window_border_width_px;
        (
            f32::from(position.x) - border_w,
            f32::from(position.y) - border_w,
        )
    }

    fn triangle_sign(point: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
        (point.0 - b.0) * (a.1 - b.1) - (a.0 - b.0) * (point.1 - b.1)
    }

    fn point_in_triangle(
        &self,
        point: (f32, f32),
        a: (f32, f32),
        b: (f32, f32),
        c: (f32, f32),
    ) -> bool {
        let d1 = Self::triangle_sign(point, a, b);
        let d2 = Self::triangle_sign(point, b, c);
        let d3 = Self::triangle_sign(point, c, a);
        let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(has_neg && has_pos)
    }

    fn format_submenu_layout(
        &self,
        menu: &ContextMenu,
        submenu_kind: &ContextMenuKind,
    ) -> Option<ContextMenuPanelLayout> {
        if !matches!(menu.kind, ContextMenuKind::Format) || !self.can_render_format_submenu() {
            return None;
        }

        let ms = &self.styles.context_menu;
        let main_menu_width = self.scaled(ms.menu_format_width_px);
        let main_menu_height = self.format_main_menu_height();
        let submenu_width = self.scaled(ms.menu_format_width_px);
        let submenu_height = self.format_submenu_height(submenu_kind);
        let submenu_gap = self.scaled(ms.menu_submenu_gap_px);
        let main_layout =
            self.context_menu_panel_layout(menu.position, main_menu_width, main_menu_height);
        let main_menu_top = self.context_menu_layout_top(main_layout, main_menu_height);
        let main_menu_bottom = main_menu_top + main_menu_height;
        let right_space = main_layout.overlay_width
            - (main_layout.left + main_menu_width)
            - submenu_gap
            - main_layout.margin;
        let submenu_left = if right_space >= submenu_width {
            main_layout.left + main_menu_width + submenu_gap
        } else {
            (main_layout.left - submenu_width - submenu_gap).max(main_layout.margin)
        };
        let submenu_bottom_anchor = (main_layout.overlay_height - main_menu_bottom).max(0.0);
        let submenu_top = (main_menu_bottom - submenu_height).max(main_layout.margin);

        Some(ContextMenuPanelLayout {
            left: submenu_left,
            estimated_top: submenu_top,
            max_height: (main_layout.overlay_height - 2.0 * main_layout.margin)
                .max(self.scaled(100.0)),
            bottom_anchor: Some(submenu_bottom_anchor),
            overlay_width: main_layout.overlay_width,
            overlay_height: main_layout.overlay_height,
            margin: main_layout.margin,
        })
    }

    fn should_preserve_format_submenu(&self, cursor_position: Point<Pixels>) -> bool {
        let Some(menu) = &self.context_menu else {
            return false;
        };
        if !matches!(menu.kind, ContextMenuKind::Format) || self.context_submenu.is_none() {
            return false;
        }

        let Some(aim) = self.context_submenu_aim else {
            return false;
        };
        let Some(submenu_kind) = self.context_submenu.as_ref() else {
            return false;
        };
        let Some(submenu_layout) = self.format_submenu_layout(menu, submenu_kind) else {
            return false;
        };

        let submenu_width = self.scaled(self.styles.context_menu.menu_format_width_px);
        let submenu_height = self.format_submenu_height(submenu_kind);
        let submenu_top = self.context_menu_layout_top(submenu_layout, submenu_height);
        let (origin_x, origin_y) = self.context_menu_overlay_coordinates(aim.origin);
        let (cursor_x, cursor_y) = self.context_menu_overlay_coordinates(cursor_position);
        let submenu_on_right = submenu_layout.left >= origin_x;
        let submenu_edge_x = if submenu_on_right {
            submenu_layout.left
        } else {
            submenu_layout.left + submenu_width
        };

        if cursor_x < origin_x.min(submenu_edge_x) - 1.0
            || cursor_x > origin_x.max(submenu_edge_x) + 1.0
        {
            return false;
        }

        self.point_in_triangle(
            (cursor_x, cursor_y),
            (origin_x, origin_y),
            (submenu_edge_x, submenu_top),
            (submenu_edge_x, submenu_top + submenu_height),
        )
    }

    fn render_context_menu_panel(
        &self,
        panel_id: &'static str,
        layout: ContextMenuPanelLayout,
        width: f32,
        items: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;

        let mut menu_div = div()
            .id(panel_id)
            .absolute()
            .left(px(layout.left))
            .w(px(width))
            .max_h(px(layout.max_height))
            .opacity(self.context_menu_opacity.clamp(0.0, 1.0))
            // Occlude so hovering a menu item does not also hover the sidebar /
            // file-tree item painted underneath the menu.
            .occlude()
            .overflow_y_scroll()
            .py(px(self.scaled(ms.menu_padding_y_px)))
            .bg(ms.menu_bg)
            .border(px(ms.menu_border_width_px))
            .border_color(ms.menu_border_color)
            .rounded(px(self.scaled(ms.menu_radius_px)));

        if let Some(bottom_anchor) = layout.bottom_anchor {
            menu_div = menu_div.bottom(px(bottom_anchor));
        } else {
            menu_div = menu_div.top(px(layout.estimated_top));
        }

        if ms.menu_shadow {
            menu_div = menu_div.shadow_sm();
        }

        let menu_div = menu_div
            .children(items)
            .cursor(CursorStyle::Arrow)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_this, _event, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|_this, _event, _window, cx| {
                cx.stop_propagation();
            }))
            .on_scroll_wheel(cx.listener(|_this, _event, _window, cx| {
                cx.stop_propagation();
            }));

        let menu_div = if panel_id == "context-menu" {
            menu_div.on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.close_context_menu(cx);
            }))
        } else {
            menu_div
        };

        let menu_div = if panel_id == "context-submenu" {
            menu_div.on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                if *hovered {
                    this.context_submenu_hovered = true;
                    return;
                }

                if this.context_submenu_hovered && this.context_submenu.is_some() {
                    this.context_submenu = None;
                    this.context_submenu_aim = None;
                    this.context_submenu_hovered = false;
                    cx.notify();
                }
            }))
        } else {
            menu_div
        };

        menu_div.into_any_element()
    }

    fn can_render_format_submenu(&self) -> bool {
        self.slash_menu_start.is_none() && self.current_image_menu_target().is_none()
    }

    fn context_menu_separator_block_height(&self) -> f32 {
        let ms = &self.styles.context_menu;
        self.scaled(ms.menu_separator_height_px + ms.menu_separator_margin_y_px * 2.0)
    }

    fn format_main_menu_height(&self) -> f32 {
        let ms = &self.styles.context_menu;
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        let separator_height = self.context_menu_separator_block_height();
        let chat_rows = if self.current_file_path.is_some() {
            2.0
        } else {
            0.0
        };
        let separator_count = if self.current_file_path.is_some() {
            4.0
        } else {
            3.0
        };
        padding
            + item_height * (3.0 + chat_rows + 5.0 + 2.0 + 1.0)
            + separator_height * separator_count
    }

    fn format_more_menu_height(&self) -> f32 {
        let ms = &self.styles.context_menu;
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        let separator_height = self.context_menu_separator_block_height();
        padding + item_height * 12.0 + separator_height * 2.0
    }

    fn format_heading_menu_height(&self) -> f32 {
        let ms = &self.styles.context_menu;
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        padding + item_height * 7.0
    }

    fn format_submenu_height(&self, submenu_kind: &ContextMenuKind) -> f32 {
        match submenu_kind {
            ContextMenuKind::FormatHeading => self.format_heading_menu_height(),
            ContextMenuKind::FormatMore => self.format_more_menu_height(),
            _ => self.format_more_menu_height(),
        }
    }

    pub(crate) fn render_context_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = &self.context_menu else {
            return div().into_any_element();
        };

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
            ContextMenuKind::Indentation => {
                let mut items = Vec::new();
                for (label, settings) in [
                    ("空格: 2", IndentSettings::spaces(2)),
                    ("空格: 4", IndentSettings::spaces(4)),
                    ("制表符: 2", IndentSettings::tabs(2)),
                    ("制表符: 4", IndentSettings::tabs(4)),
                ] {
                    items.push(self.render_context_menu_choice_item(
                        label,
                        self.indent_settings == settings,
                        cx,
                        move |this, _, cx| {
                            this.set_indent_settings(settings, cx);
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
            ContextMenuKind::Image => {
                let mut items = Vec::new();
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::Copy),
                    MENU_COPY_SVG_PATH,
                    false,
                    cx,
                    |this, _, cx| {
                        this.copy_active_image_to_clipboard(cx);
                    },
                ));
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::RevealInFinder),
                    MENU_LOCATE_SVG_PATH,
                    false,
                    cx,
                    |this, _, cx| {
                        if let Some(path) = this.current_file_path.clone() {
                            this.reveal_in_finder(&path);
                        }
                        this.close_context_menu(cx);
                    },
                ));
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_sidebar_width_px),
                )
            }
            ContextMenuKind::Whiteboard => {
                let mut items = Vec::new();
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::Cut),
                    MENU_CLIP_SVG_PATH,
                    false,
                    cx,
                    |this, _, cx| {
                        this.close_context_menu(cx);
                        this.whiteboard_cut(cx);
                    },
                ));
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::Copy),
                    MENU_COPY_SVG_PATH,
                    false,
                    cx,
                    |this, _, cx| {
                        this.close_context_menu(cx);
                        this.whiteboard_copy(cx);
                    },
                ));
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::Paste),
                    MENU_PASTE_SVG_PATH,
                    false,
                    cx,
                    |this, _, cx| {
                        this.close_context_menu(cx);
                        this.whiteboard_paste(cx);
                    },
                ));
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::Delete),
                    MENU_DELETE_SVG_PATH,
                    true,
                    cx,
                    |this, _, cx| {
                        this.close_context_menu(cx);
                        this.whiteboard_delete_selected(cx);
                    },
                ));
                // Copy-as exports (selection, or the whole canvas when empty).
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::WhiteboardCopyAsPng),
                    false,
                    cx,
                    |this, _, cx| {
                        this.close_context_menu(cx);
                        this.whiteboard_copy_as_png(cx);
                    },
                ));
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::WhiteboardCopyAsSvg),
                    false,
                    cx,
                    |this, _, cx| {
                        this.close_context_menu(cx);
                        this.whiteboard_copy_as_svg(cx);
                    },
                ));
                if self.whiteboard_has_selection() {
                    // Grouping
                    if self.whiteboard_can_group() || self.whiteboard_can_ungroup() {
                        items.push(self.render_context_menu_separator());
                        if self.whiteboard_can_group() {
                            items.push(self.render_context_menu_item(
                                self.t(crate::i18n::TextKey::WhiteboardGroup),
                                false,
                                cx,
                                |this, _, cx| {
                                    this.close_context_menu(cx);
                                    this.whiteboard_group_selection(cx);
                                },
                            ));
                        }
                        if self.whiteboard_can_ungroup() {
                            items.push(self.render_context_menu_item(
                                self.t(crate::i18n::TextKey::WhiteboardUngroup),
                                false,
                                cx,
                                |this, _, cx| {
                                    this.close_context_menu(cx);
                                    this.whiteboard_ungroup_selection(cx);
                                },
                            ));
                        }
                    }
                    // Z-ordering
                    items.push(self.render_context_menu_separator());
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::WhiteboardBringForward),
                        false,
                        cx,
                        |this, _, cx| {
                            this.close_context_menu(cx);
                            this.whiteboard_bring_forward(cx);
                        },
                    ));
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::WhiteboardSendBackward),
                        false,
                        cx,
                        |this, _, cx| {
                            this.close_context_menu(cx);
                            this.whiteboard_send_backward(cx);
                        },
                    ));
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::WhiteboardBringToFront),
                        false,
                        cx,
                        |this, _, cx| {
                            this.close_context_menu(cx);
                            this.whiteboard_bring_to_front(cx);
                        },
                    ));
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::WhiteboardSendToBack),
                        false,
                        cx,
                        |this, _, cx| {
                            this.close_context_menu(cx);
                            this.whiteboard_send_to_back(cx);
                        },
                    ));
                    // Mirroring
                    items.push(self.render_context_menu_separator());
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::WhiteboardFlipHorizontal),
                        false,
                        cx,
                        |this, _, cx| {
                            this.close_context_menu(cx);
                            this.whiteboard_flip_horizontal(cx);
                        },
                    ));
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::WhiteboardFlipVertical),
                        false,
                        cx,
                        |this, _, cx| {
                            this.close_context_menu(cx);
                            this.whiteboard_flip_vertical(cx);
                        },
                    ));
                }
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_sidebar_width_px),
                )
            }
            ContextMenuKind::FormatMore => (
                self.render_format_more_context_menu(cx),
                self.scaled(self.styles.context_menu.menu_format_width_px),
            ),
            ContextMenuKind::FormatHeading => (
                self.render_heading_context_menu(cx),
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
            ContextMenuKind::ChatTabCreate => {
                let mut items = Vec::new();
                let mut agent_clis = ChatAgentCli::all();
                agent_clis.sort_by_key(|agent_cli| !matches!(agent_cli, ChatAgentCli::Terminal));
                for agent_cli in agent_clis {
                    items.push(self.render_context_menu_item_with_icon(
                        agent_cli.label(),
                        agent_cli.icon_path(),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.create_chat_tab_with_agent(agent_cli, cx);
                        },
                    ));
                }
                (
                    items,
                    self.scaled(
                        self.styles
                            .context_menu
                            .menu_sidebar_width_px
                            .max(self.styles.context_menu.menu_tab_width_px),
                    ),
                )
            }
            ContextMenuKind::ChatTab { tab_id } => {
                let mut items = Vec::new();
                let (is_pinned, active_agent_cli) = self
                    .chat_tabs
                    .iter()
                    .find(|tab| tab.id == *tab_id)
                    .map(|tab| (tab.is_pinned, tab.agent_cli))
                    .unwrap_or((false, ChatAgentCli::GithubCopilot));
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::Close),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.close_chat_tab(&id, cx);
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
                            this.close_chat_other_tabs(&id, cx);
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
                            this.close_chat_left_tabs(&id, cx);
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
                            this.close_chat_right_tabs(&id, cx);
                        },
                    ));
                }
                items.push(self.render_context_menu_separator());
                let mut agent_clis = ChatAgentCli::all();
                agent_clis.sort_by_key(|agent_cli| !matches!(agent_cli, ChatAgentCli::Terminal));
                for agent_cli in agent_clis {
                    let id = tab_id.clone();
                    let is_active = active_agent_cli == agent_cli;
                    items.push(self.render_context_menu_item_with_icon_and_check(
                        agent_cli.label(),
                        agent_cli.icon_path(),
                        is_active,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.select_chat_tab_agent_cli(&id, agent_cli, cx);
                        },
                    ));
                }
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
                            this.toggle_chat_tab_pin(&id, cx);
                        },
                    ));
                }
                (
                    items,
                    self.scaled(
                        self.styles
                            .context_menu
                            .menu_sidebar_width_px
                            .max(self.styles.context_menu.menu_tab_width_px),
                    ),
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
                let text_color = ms.menu_item_primary_text_color;

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
                items.push(self.render_context_menu_item_with_icon(
                    self.t(crate::i18n::TextKey::HelpManual),
                    DOC_SVG_PATH,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.open_welcome_tab(cx);
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

        let estimated_menu_height = self.estimated_context_menu_height(items.len());
        let anchor_height = self.context_menu_anchor_height(&menu.kind);
        let layout = self.context_menu_panel_layout_with_anchor(
            menu.position,
            width,
            estimated_menu_height,
            anchor_height,
        );
        let main_menu = self.render_context_menu_panel("context-menu", layout, width, items, cx);

        let submenu = if matches!(menu.kind, ContextMenuKind::Format)
            && self.context_submenu.is_some()
            && self.can_render_format_submenu()
        {
            let submenu_kind = self.context_submenu.as_ref().cloned();
            let submenu_items = match submenu_kind.as_ref() {
                Some(ContextMenuKind::FormatHeading) => self.render_heading_context_menu(cx),
                Some(ContextMenuKind::FormatMore) => self.render_format_more_context_menu(cx),
                _ => Vec::new(),
            };
            let submenu_width = self.scaled(self.styles.context_menu.menu_format_width_px);
            let submenu_layout = submenu_kind
                .as_ref()
                .and_then(|submenu_kind| self.format_submenu_layout(menu, submenu_kind))
                .unwrap_or(ContextMenuPanelLayout {
                    left: layout.left + width,
                    estimated_top: layout.estimated_top,
                    max_height: layout.max_height,
                    bottom_anchor: None,
                    overlay_width: layout.overlay_width,
                    overlay_height: layout.overlay_height,
                    margin: layout.margin,
                });
            Some(self.render_context_menu_panel(
                "context-submenu",
                submenu_layout,
                submenu_width,
                submenu_items,
                cx,
            ))
        } else {
            None
        };

        div()
            .id("context-menu-anchor")
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w_full()
            .h_full()
            .child(main_menu)
            .when_some(submenu, |this, submenu| this.child(submenu))
            .into_any_element()
    }
}
