mod element;
mod fold;
mod handlers;
mod helpers;
mod input;
mod menus;
mod render;
mod scaling;
mod sidebar;
mod tabs;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    ClipboardItem, Context, FocusHandle, Focusable, Pixels, Point, RenderImage, SharedString,
    WrappedLine,
};

use crate::actions::*;
use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::markdown::{self, BlockType, LineDecoration};
use crate::theme::{Theme, ThemeKind};

// ── Layout constants ────────────────────────────────────────────────

pub(crate) const LINE_HEIGHT_FACTOR: f32 = 1.6;
pub(crate) const BASE_FONT_SIZE: f32 = 15.0;
pub(crate) const PADDING_LEFT: f32 = 40.0;
pub(crate) const PADDING_TOP: f32 = 50.0;
pub(crate) const PADDING_RIGHT: f32 = 40.0;
pub(crate) const PADDING_BOTTOM: f32 = 200.0;
pub(crate) const MAX_EDITOR_WIDTH: f32 = 750.0;
pub(crate) const SIDEBAR_WIDTH: f32 = 280.0;
pub(crate) const KNOWLEDGE_BASE_ROOT: &str = "/Users/imeoer/code/我的知识库";

// ── Cursor blink constants ──────────────────────────────────────────

pub(crate) const CURSOR_BLINK_TICK: Duration = Duration::from_millis(5);
pub(crate) const CURSOR_BLINK_PAUSE_DELAY: Duration = Duration::from_millis(300);
pub(crate) const CURSOR_BLINK_VISIBLE_DURATION: Duration = Duration::from_millis(200);
pub(crate) const CURSOR_BLINK_TRANSPARENT_DURATION: Duration = Duration::from_millis(300);
pub(crate) const CURSOR_MIN_OPACITY: f32 = 0.0;
pub(crate) const CURSOR_MAX_OPACITY: f32 = 1.0;
pub(crate) const CURSOR_OPACITY_STEP: f32 = 0.03;

// ── Editor element constants ────────────────────────────────────────

pub(crate) const CODE_BLOCK_TEXT_LEFT: f32 = 10.0;
pub(crate) const TODO_CHECKBOX_SIZE_FACTOR: f32 = 1.2;
pub(crate) const TODO_CHECKBOX_CORNER_RADIUS: f32 = 5.0;
pub(crate) const TODO_CHECK_ICON_SCALE: f32 = 0.52;
pub(crate) const TABLE_CELL_TEXT_PADDING_PX: f32 = 5.0;
pub(crate) const GUTTER_FOLD_ICON_SIZE: f32 = 10.0;

// ── SVG asset paths ─────────────────────────────────────────────────

pub(crate) const HASH_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/hash.svg");
pub(crate) const COMMENT_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/comment.svg");
pub(crate) const CHECK_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/check.svg");
pub(crate) const FOLD_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/fold.svg");
pub(crate) const OPEN_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/open.svg");
pub(crate) const LOGO_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/logo.svg");
pub(crate) const ROOT_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/filetree/all.svg");
pub(crate) const FILETREE_FOLD_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/fold.svg"
);
pub(crate) const SIDEBAR_TOGGLE_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/sidebar-toggle.svg"
);
pub(crate) const EDITOR_ALIGN_LEFT_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/editor-align-left.svg"
);
pub(crate) const THEME_TOGGLE_SVG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/theme-toggle.svg");
pub(crate) const FOLDER_OPEN_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/folder-open.svg"
);
pub(crate) const FOLDER_CLOSED_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/folder-closed.svg"
);
pub(crate) const MARKDOWN_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/markdown.svg"
);

// ── UI constants ────────────────────────────────────────────────────

pub(crate) const TAB_BAR_HEIGHT: f32 = 38.0;
pub(crate) const SIDEBAR_ROW_HEIGHT: f32 = 28.0;
pub(crate) const FORMAT_MENU_WIDTH: f32 = 250.0;
pub(crate) const UI_SCALE_STEP: f32 = 0.1;
pub(crate) const MIN_UI_SCALE: f32 = 0.8;
pub(crate) const MAX_UI_SCALE: f32 = 1.6;

// ── Data types ──────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct EditorSnapshot {
    pub text: String,
    pub cursor_offset: usize,
    pub cursor_anchor: usize,
}

#[derive(Clone)]
pub(crate) struct OpenTab {
    pub id: String,
    pub title: String,
    pub path: Option<PathBuf>,
    pub snapshot: EditorSnapshot,
    pub is_dirty: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SidebarItem {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
}

#[derive(Clone)]
pub(crate) struct ContextMenu {
    pub position: Point<Pixels>,
    pub kind: ContextMenuKind,
}

#[derive(Clone)]
pub(crate) enum ContextMenuKind {
    Sidebar { path: PathBuf, is_dir: bool },
    Format,
    Tab { tab_id: String },
}

#[derive(Clone, Copy)]
pub(crate) enum FormatMenuAction {
    Cut,
    Copy,
    Paste,
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    CodeBlock,
    HorizontalRule,
    Quote,
    UnorderedList,
    OrderedList,
    Todo,
    Link,
    Image,
}

#[derive(Clone)]
pub(crate) enum SidebarEditMode {
    Rename { path: PathBuf, value: String },
    CreateFile { parent: PathBuf, value: String },
    CreateDir { parent: PathBuf, value: String },
}

impl SidebarEditMode {
    pub fn value_mut(&mut self) -> &mut String {
        match self {
            Self::Rename { value, .. }
            | Self::CreateFile { value, .. }
            | Self::CreateDir { value, .. } => value,
        }
    }
}

#[derive(Clone)]
pub(crate) struct FileClipboard {
    pub paths: Vec<PathBuf>,
    pub is_cut: bool,
}

/// Display info for a single buffer line.
#[derive(Clone)]
pub(crate) struct LineDisplayInfo {
    pub display_text: String,
    pub display_to_buffer: Vec<usize>,
    pub buffer_to_display: Vec<usize>,
    pub font_size: f32,
    pub line_height: f32,
    pub visual_line_count: usize,
    pub wrapped_line: WrappedLine,
    pub extra_height: f32,
    pub indent_px: f32,
    pub content_x: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordCharClass {
    AsciiWord,
    Cjk,
    Whitespace,
    Other,
}

// ── EditorView ──────────────────────────────────────────────────────

pub struct EditorView {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) theme_kind: ThemeKind,
    pub(crate) theme: Theme,
    pub(crate) styles: crate::style::Styles,
    pub(crate) decorations: Vec<LineDecoration>,
    pub(crate) scroll_offset: f32,
    pub(crate) is_selecting: bool,
    pub(crate) line_display_info: Vec<Option<LineDisplayInfo>>,
    pub(crate) last_viewport_height: f32,
    pub(crate) last_viewport_width: f32,
    // Cursor blink state
    pub(crate) cursor_opacity: f32,
    pub(crate) cursor_opacity_direction: f32,
    pub(crate) blink_epoch: usize,
    // IME preedit state
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) checkbox_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) hovering_checkbox: bool,
    pub(crate) image_click_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) image_cache: HashMap<String, Option<Arc<RenderImage>>>,
    pub(crate) undo_stack: Vec<EditorSnapshot>,
    pub(crate) redo_stack: Vec<EditorSnapshot>,
    pub(crate) sidebar_root: PathBuf,
    pub(crate) expanded_dirs: HashSet<PathBuf>,
    pub(crate) selected_sidebar_path: Option<PathBuf>,
    pub(crate) current_file_path: Option<PathBuf>,
    pub(crate) sidebar_error: Option<String>,
    pub(crate) tabs: Vec<OpenTab>,
    pub(crate) active_tab_id: Option<String>,
    pub(crate) next_tab_id: usize,
    pub(crate) last_editor_left: f32,
    pub(crate) last_editor_width: f32,
    pub(crate) last_content_left: f32,
    pub(crate) last_text_width: f32,
    pub(crate) content_origin: Point<Pixels>,
    pub(crate) context_menu: Option<ContextMenu>,
    pub(crate) sidebar_edit: Option<SidebarEditMode>,
    pub(crate) file_clipboard: Option<FileClipboard>,
    pub(crate) ui_scale: f32,
    pub(crate) folded_sections: HashSet<usize>,
    pub(crate) folded_image_previews: HashSet<usize>,
    pub(crate) gutter_fold_areas: Vec<(gpui::Bounds<Pixels>, usize, bool)>,
    pub(crate) image_edit_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) hovering_image_edit_line: Option<usize>,
    pub(crate) hovering_fold_line: Option<usize>,
    pub(crate) sidebar_custom_width: Option<f32>,
    pub(crate) sidebar_resizing: bool,
    pub(crate) heading_submenu_open: bool,
    pub(crate) slash_menu_start: Option<usize>,
    pub(crate) slash_filter: String,
    pub(crate) menu_selected_index: usize,
    pub(crate) sidebar_visible: bool,
    pub(crate) editor_left_aligned: bool,
    pub(crate) tab_dragging_id: Option<String>,
}

impl EditorView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let sidebar_root = PathBuf::from(KNOWLEDGE_BASE_ROOT);
        let mut expanded_dirs = HashSet::new();
        expanded_dirs.insert(sidebar_root.clone());
        let sidebar_error = (!sidebar_root.exists())
            .then(|| format!("Directory not found: {}", sidebar_root.display()));
        let initial_text = default_document_text();
        let buffer = Buffer::from_text(initial_text);
        let decorations = markdown::parse_decorations(buffer.text());
        let initial_snapshot = EditorSnapshot {
            text: initial_text.to_string(),
            cursor_offset: 0,
            cursor_anchor: 0,
        };
        let theme_kind = ThemeKind::OneDark;
        let theme = theme_kind.build();
        let styles = crate::style::Styles::from_theme(&theme);
        let mut this = Self {
            buffer,
            cursor: Cursor::new(),
            focus_handle: cx.focus_handle(),
            theme_kind,
            theme,
            styles,
            decorations,
            scroll_offset: 0.0,
            is_selecting: false,
            line_display_info: Vec::new(),
            last_viewport_height: 600.0,
            last_viewport_width: MAX_EDITOR_WIDTH,
            cursor_opacity: CURSOR_MAX_OPACITY,
            cursor_opacity_direction: -1.0,
            blink_epoch: 0,
            marked_range: None,
            checkbox_areas: Vec::new(),
            hovering_checkbox: false,
            image_click_areas: Vec::new(),
            image_cache: HashMap::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            sidebar_root,
            expanded_dirs,
            selected_sidebar_path: None,
            current_file_path: None,
            sidebar_error,
            tabs: vec![OpenTab {
                id: "welcome-1".to_string(),
                title: "Welcome".to_string(),
                path: None,
                snapshot: initial_snapshot,
                is_dirty: false,
            }],
            active_tab_id: Some("welcome-1".to_string()),
            next_tab_id: 2,
            last_editor_left: 0.0,
            last_editor_width: MAX_EDITOR_WIDTH,
            last_content_left: PADDING_LEFT,
            last_text_width: MAX_EDITOR_WIDTH - PADDING_LEFT - PADDING_RIGHT,
            content_origin: Point::default(),
            context_menu: None,
            sidebar_edit: None,
            file_clipboard: None,
            ui_scale: 1.0,
            folded_sections: HashSet::new(),
            folded_image_previews: HashSet::new(),
            gutter_fold_areas: Vec::new(),
            image_edit_areas: Vec::new(),
            hovering_image_edit_line: None,
            hovering_fold_line: None,
            sidebar_custom_width: None,
            sidebar_resizing: false,
            heading_submenu_open: false,
            menu_selected_index: 0,
            slash_menu_start: None,
            slash_filter: String::new(),
            sidebar_visible: true,
            editor_left_aligned: false,
            tab_dragging_id: None,
        };
        this.start_blinking(cx);

        // Open EDITOR.md on startup if it exists
        let editor_md = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/EDITOR.md"));
        if editor_md.exists() {
            this.open_sidebar_file(editor_md, cx);
        }

        this
    }

    // ── Theme toggle ─────────────────────────────────────────────

    pub(crate) fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        self.theme_kind = self.theme_kind.toggle();
        self.theme = self.theme_kind.build();
        self.styles = crate::style::Styles::from_theme(&self.theme);
        cx.notify();
    }

    // ── Cursor blink ────────────────────────────────────────────────

    pub(crate) fn next_blink_epoch(&mut self) -> usize {
        self.blink_epoch += 1;
        self.blink_epoch
    }

    pub(crate) fn start_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = CURSOR_MAX_OPACITY;
        self.cursor_opacity_direction = -1.0;
        let epoch = self.next_blink_epoch();
        self.schedule_blink(epoch, cx);
    }

    pub(crate) fn schedule_blink(&self, epoch: usize, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CURSOR_BLINK_TICK).await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.cursor_opacity += CURSOR_OPACITY_STEP * this.cursor_opacity_direction;
                    if this.cursor_opacity <= CURSOR_MIN_OPACITY {
                        this.cursor_opacity = CURSOR_MIN_OPACITY;
                        this.cursor_opacity_direction = 1.0;
                        cx.notify();
                        this.schedule_blink_pause(epoch, CURSOR_BLINK_TRANSPARENT_DURATION, cx);
                        return;
                    } else if this.cursor_opacity >= CURSOR_MAX_OPACITY {
                        this.cursor_opacity = CURSOR_MAX_OPACITY;
                        this.cursor_opacity_direction = -1.0;
                        cx.notify();
                        this.schedule_blink_pause(epoch, CURSOR_BLINK_VISIBLE_DURATION, cx);
                        return;
                    }
                    cx.notify();
                    this.schedule_blink(epoch, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn schedule_blink_pause(&self, epoch: usize, duration: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.schedule_blink(epoch, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn pause_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = CURSOR_MAX_OPACITY;
        self.cursor_opacity_direction = -1.0;
        let epoch = self.next_blink_epoch();
        self.persist_active_tab_state();
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(CURSOR_BLINK_PAUSE_DELAY)
                .await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.schedule_blink(epoch, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    // ── Shared utilities ────────────────────────────────────────────

    pub(crate) fn reparse(&mut self) {
        self.decorations = markdown::parse_decorations(self.buffer.text());
        self.auto_align_table();
    }

    pub(crate) fn cursor_line(&self) -> usize {
        self.buffer.offset_to_line_col(self.cursor.offset).0
    }

    pub(crate) fn current_snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.buffer.text().to_string(),
            cursor_offset: self.cursor.offset,
            cursor_anchor: self.cursor.anchor,
        }
    }

    pub(crate) fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.buffer = Buffer::from_text(&snapshot.text);
        self.cursor.offset = snapshot.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = snapshot.cursor_anchor.min(self.buffer.len());
        self.marked_range = None;
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn checkpoint(&mut self) {
        self.undo_stack.push(self.current_snapshot());
        self.redo_stack.clear();
        self.mark_active_tab_dirty();
        if self.undo_stack.len() > 300 {
            self.undo_stack.remove(0);
        }
    }

    pub(crate) fn copy_selection_to_clipboard(&self, cx: &mut Context<Self>) {
        if self.cursor.has_selection() {
            let range = self.cursor.selection_range();
            let text = self.buffer.text()[range].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(crate) fn cut_selection_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.cursor.has_selection() {
            self.checkpoint();
            let range = self.cursor.selection_range();
            let text = self.buffer.text()[range.clone()].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.buffer.delete(range.clone());
            self.cursor.move_to(range.start, &self.buffer);
            self.reparse();
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.checkpoint();
            self.cursor.insert(&mut self.buffer, &text);
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn toggle_current_line_prefix(&mut self, prefix: &str) {
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let line_start = self.buffer.line_start(line);
        let line_end = self.buffer.line_end(line);
        let line_text = self.buffer.line_content(line).to_string();
        let trimmed = line_text.trim_start();
        let leading_ws = line_text.len() - trimmed.len();
        let replacement = if trimmed.starts_with(prefix) {
            let mut new_line = String::with_capacity(line_text.len());
            new_line.push_str(&line_text[..leading_ws]);
            new_line.push_str(&trimmed[prefix.len()..]);
            new_line
        } else {
            let mut new_line = String::with_capacity(line_text.len() + prefix.len());
            new_line.push_str(&line_text[..leading_ws]);
            new_line.push_str(prefix);
            new_line.push_str(trimmed);
            new_line
        };

        self.buffer.replace(line_start..line_end, &replacement);
        let new_col = if trimmed.starts_with(prefix) {
            self.cursor.offset.saturating_sub(prefix.len())
        } else {
            self.cursor.offset + prefix.len()
        };
        self.cursor
            .move_to(new_col.min(self.buffer.len()), &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn toggle_horizontal_rule_at_cursor(&mut self) {
        let line = self.cursor_line();
        let line_start = self.buffer.line_start(line);
        let line_end = self.buffer.line_end(line);
        let line_text = self.buffer.line_content(line).to_string();
        let trimmed = line_text.trim();
        let replacement = if matches!(trimmed, "---" | "***" | "___") {
            String::new()
        } else {
            "---".to_string()
        };
        self.buffer.replace(line_start..line_end, &replacement);
        self.cursor.move_to(
            (line_start + replacement.len()).min(self.buffer.len()),
            &self.buffer,
        );
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn set_ui_scale(&mut self, new_scale: f32, cx: &mut Context<Self>) {
        let clamped = new_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        if (clamped - self.ui_scale).abs() < f32::EPSILON {
            return;
        }
        if let Some(ref mut w) = self.sidebar_custom_width {
            *w = *w * (clamped / self.ui_scale);
        }
        self.ui_scale = clamped;
        self.clamp_scroll();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        cx.notify();
    }

    pub(crate) fn indent_current_lines(&mut self, spaces: &str) {
        let selection = self.cursor.selection_range();
        let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
        let end_anchor = selection
            .end
            .saturating_sub(usize::from(selection.end > selection.start));
        let (end_line, _) = self.buffer.offset_to_line_col(end_anchor);

        let mut inserted = 0usize;
        for line in start_line..=end_line {
            let line_start = self.buffer.line_start(line) + inserted;
            self.buffer.insert(line_start, spaces);
            inserted += spaces.len();
        }

        let new_start = selection.start + spaces.len();
        let new_end = selection.end + inserted;
        self.cursor.anchor = new_start.min(self.buffer.len());
        self.cursor.offset = new_end.min(self.buffer.len());
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn insert_link_or_image(&mut self, prefix: &str, suffix: &str, placeholder: &str) {
        let range = self.cursor.selection_range();
        let selected = self.buffer.text()[range.clone()].to_string();
        let replacement = if selected.is_empty() {
            format!("{}{placeholder}{}", prefix, suffix)
        } else {
            format!("{}{}{}", prefix, selected, suffix)
        };
        self.buffer.replace(range.clone(), &replacement);
        let desc_end = if selected.is_empty() {
            range.start + prefix.len() + placeholder.len()
        } else {
            range.start + prefix.len() + selected.len()
        };
        let url_start = desc_end + 2;
        let url_end = desc_end + suffix.len() - 1;
        self.cursor.anchor = url_start;
        self.cursor.offset = url_end;
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn toggle_checkbox(&mut self, line: usize) {
        let line_content = self.buffer.line_content(line).to_string();
        let line_start = self.buffer.line_start(line);
        if let Some(pos) = line_content.find("[ ]") {
            self.buffer
                .replace(line_start + pos..line_start + pos + 3, "[x]");
        } else if let Some(pos) = line_content.find("[x]") {
            self.buffer
                .replace(line_start + pos..line_start + pos + 3, "[ ]");
        }
    }

    pub(crate) fn apply_wrap(&mut self, prefix: &str, suffix: &str) {
        if !self.cursor.has_selection() {
            let offset = self.cursor.offset;
            let (line, col) = self.buffer.offset_to_line_col(offset);
            let line_content = self.buffer.line_content(line).to_string();
            if let Some(dec) = self.decorations.get(line).cloned() {
                for span in &dec.inline_spans {
                    let kind_prefix = match span.kind {
                        crate::markdown::InlineKind::Bold => "**",
                        crate::markdown::InlineKind::Italic => "*",
                        crate::markdown::InlineKind::Strikethrough => "~~",
                        crate::markdown::InlineKind::InlineCode => "`",
                        _ => continue,
                    };
                    let kind_suffix = kind_prefix;
                    if kind_prefix != prefix || kind_suffix != suffix {
                        continue;
                    }
                    let marker_start = span.range.start.saturating_sub(prefix.len());
                    let marker_end = (span.range.end + suffix.len()).min(line_content.len());
                    if marker_start + prefix.len() <= line_content.len()
                        && line_content[marker_start..].starts_with(prefix)
                        && marker_end >= suffix.len()
                        && line_content[marker_end - suffix.len()..marker_end].ends_with(suffix)
                        && col >= marker_start
                        && col <= marker_end
                    {
                        let line_start = self.buffer.line_start(line);
                        let sel_start = line_start + marker_start;
                        let sel_end = line_start + marker_end;
                        let (new_text, new_start, new_end) =
                            toggle_wrap(self.buffer.text(), sel_start, sel_end, prefix, suffix);
                        self.buffer = Buffer::from_text(&new_text);
                        self.cursor.anchor = new_start;
                        self.cursor.offset = new_end;
                        self.reparse();
                        self.scroll_to_cursor();
                        return;
                    }
                }
            }
        }
        let range = self.cursor.selection_range();
        let (new_text, new_start, new_end) =
            toggle_wrap(self.buffer.text(), range.start, range.end, prefix, suffix);
        self.buffer = Buffer::from_text(&new_text);
        self.cursor.anchor = new_start;
        self.cursor.offset = new_end;
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn apply_heading(&mut self, level: u8) {
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let line_start = self.buffer.line_start(line);
        let line_end = self.buffer.line_end(line);
        let (new_text, new_cursor) =
            toggle_heading(self.buffer.text(), line_start, line_end, level);
        self.buffer = Buffer::from_text(&new_text);
        self.cursor.move_to(new_cursor, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
    }

    /// Auto-align table pipes so `|` characters line up vertically across rows.
    pub(crate) fn auto_align_table(&mut self) {
        let cursor_line = self.cursor_line();
        let decoration = match self.decorations.get(cursor_line) {
            Some(d) if d.block_type == BlockType::Table => d,
            _ => return,
        };
        let table_info = match &decoration.table_info {
            Some(ti) => ti.clone(),
            None => return,
        };

        let table_start = table_info.table_start_line;
        let table_end = table_info.table_end_line;

        let mut all_rows: Vec<Vec<String>> = Vec::new();
        let mut separator_indices: Vec<usize> = Vec::new();
        for line_idx in table_start..table_end {
            let content = self.buffer.line_content(line_idx).to_string();
            let cells = markdown::parse_table_cells(&content);
            if markdown::is_separator_row(&cells) {
                separator_indices.push(line_idx - table_start);
            }
            all_rows.push(cells);
        }

        if all_rows.is_empty() {
            return;
        }

        let max_cols = all_rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut col_widths = vec![0usize; max_cols];
        for (ri, row) in all_rows.iter().enumerate() {
            if separator_indices.contains(&ri) {
                continue;
            }
            for (ci, cell) in row.iter().enumerate() {
                let trimmed_len = cell.trim().len();
                if ci < max_cols {
                    col_widths[ci] = col_widths[ci].max(trimmed_len);
                }
            }
        }
        for w in col_widths.iter_mut() {
            *w = (*w).max(3);
        }

        let cursor_offset = self.cursor.offset;
        let cursor_line_start = self.buffer.line_start(cursor_line);
        let cursor_col_in_line = cursor_offset - cursor_line_start;
        let cursor_row_content = self.buffer.line_content(cursor_line).to_string();

        let (cursor_cell_idx, cursor_pos_in_cell) = {
            let mut cell_idx = 0;
            let mut pos_in_cell = 0;
            let trimmed = cursor_row_content.trim();
            let offset = cursor_row_content.len() - cursor_row_content.trim_start().len();
            let scanning = if trimmed.starts_with('|') {
                offset + 1
            } else {
                offset
            };
            let mut in_cell_start = scanning;
            for (i, ch) in cursor_row_content[scanning..].char_indices() {
                let abs_pos = scanning + i;
                if ch == '|' {
                    if cursor_col_in_line >= in_cell_start && cursor_col_in_line <= abs_pos {
                        pos_in_cell = cursor_col_in_line - in_cell_start;
                        break;
                    }
                    cell_idx += 1;
                    in_cell_start = abs_pos + 1;
                }
            }
            if cursor_col_in_line >= in_cell_start {
                pos_in_cell = cursor_col_in_line - in_cell_start;
            }
            (cell_idx, pos_in_cell)
        };

        let table_text_start = self.buffer.line_start(table_start);
        let table_text_end = self.buffer.line_end(table_end.saturating_sub(1));

        let mut new_lines: Vec<String> = Vec::new();
        for (ri, row) in all_rows.iter().enumerate() {
            let is_sep = separator_indices.contains(&ri);
            let mut line = String::from("|");
            for ci in 0..max_cols {
                if is_sep {
                    line.push_str(&"-".repeat(col_widths[ci] + 2));
                } else {
                    let cell = row.get(ci).map(|s| s.trim()).unwrap_or("");
                    line.push(' ');
                    line.push_str(&format!("{:width$}", cell, width = col_widths[ci]));
                    line.push(' ');
                }
                line.push('|');
            }
            new_lines.push(line);
        }

        let new_table_text = new_lines.join("\n");
        let old_table_text = &self.buffer.text()[table_text_start..table_text_end];
        if new_table_text == old_table_text {
            return;
        }

        let cursor_row_in_table = cursor_line - table_start;
        let new_cursor_line_start = {
            let mut offset = table_text_start;
            for i in 0..cursor_row_in_table {
                offset += new_lines[i].len() + 1;
            }
            offset
        };

        let new_line = &new_lines[cursor_row_in_table];
        let new_cursor_offset = {
            let mut cell_count = 0;
            let mut cell_start = 1;
            let mut found_offset = new_cursor_line_start + new_line.len();
            for (i, ch) in new_line[1..].char_indices() {
                if ch == '|' {
                    if cell_count == cursor_cell_idx {
                        let cell_pos = cursor_pos_in_cell.min(i - (cell_start - 1));
                        found_offset = new_cursor_line_start + cell_start + cell_pos;
                        break;
                    }
                    cell_count += 1;
                    cell_start = 1 + i + 1;
                }
            }
            found_offset
        };

        self.buffer
            .replace(table_text_start..table_text_end, &new_table_text);
        self.cursor.offset = new_cursor_offset.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.decorations = markdown::parse_decorations(self.buffer.text());
    }

    pub(crate) fn renumber_ordered_list_after(
        &mut self,
        start_line: usize,
        next_number: u32,
        indent: usize,
    ) {
        let mut items: Vec<(usize, usize, u32)> = Vec::new();
        let mut number = next_number;
        for line in start_line.. {
            if line >= self.buffer.line_count() {
                break;
            }
            let lc = self.buffer.line_content(line).to_string();
            let lws = lc.len() - lc.trim_start_matches([' ', '\t']).len();
            if lws != indent {
                break;
            }
            let trimmed = &lc[lws..];
            let plen = markdown::ordered_list_prefix_len(trimmed);
            if plen < 2 {
                break;
            }
            let current_str = &trimmed[..plen - 2];
            if current_str.parse::<u32>().is_err() {
                break;
            }
            let num_start = self.buffer.line_start(line) + lws;
            let num_end = num_start + current_str.len();
            items.push((num_start, num_end, number));
            number += 1;
        }
        for (num_start, num_end, new_num) in items.into_iter().rev() {
            self.buffer
                .replace(num_start..num_end, &new_num.to_string());
        }
    }
}

// ── Free functions ──────────────────────────────────────────────────

pub(crate) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

pub(crate) fn default_document_text() -> &'static str {
    "# Welcome to Markdown Editor\n\nThis is a **bold** and *italic* demo.\n\nYou can write `inline code` and:\n\n```rust\nfn greet(name: &str) {\n  println!(\"hello, {name}\");\n}\n```\n\n## Features\n\n- Syntax markers hide when cursor leaves the range\n- .md files open into real tabs\n- FileTree now lives in a fixed left pane\n- `inline code` with background\n- Headings with different sizes\n- [Visit Google](https://google.com) for search\n- Check out ![Logo](crates/zed/resources/app-icon.png) inline image\n\n> Blockquotes look like this\n\n### Tasks\n\n- [ ] Unchecked task item\n- [x] Completed task item\n- [ ] Another todo\n\n| Name | Type | Description |\n|------|------|-------------|\n| id | int | **Primary** key |\n| name | string | [User profile](https://example.com) |\n| email | string | `Email` addr |\n\n1. First ordered item\n2. Second ordered item\n\n---\n\nStart typing to edit!\n"
}

pub(crate) fn classify_word_char(ch: char) -> WordCharClass {
    if ch.is_whitespace() {
        return WordCharClass::Whitespace;
    }
    if ch.is_ascii_alphanumeric() || ch == '_' {
        return WordCharClass::AsciiWord;
    }
    let code = ch as u32;
    let is_cjk = (0x3400..=0x4DBF).contains(&code)
        || (0x4E00..=0x9FFF).contains(&code)
        || (0x3040..=0x309F).contains(&code)
        || (0x30A0..=0x30FF).contains(&code)
        || (0xAC00..=0xD7AF).contains(&code)
        || matches!(ch, '々' | '〆' | '〇' | 'ー');
    if is_cjk {
        WordCharClass::Cjk
    } else {
        WordCharClass::Other
    }
}

pub(crate) fn task_prefix_len(trimmed: &str) -> usize {
    for marker in ["- [ ] ", "- [x] ", "* [ ] ", "* [x] ", "+ [ ] ", "+ [x] "] {
        if trimmed.starts_with(marker) {
            return marker.len();
        }
    }
    0
}
