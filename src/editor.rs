use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use gpui::{
    AnyElement, App, BorderStyle, Bounds, ClipboardItem, Context, Corners, CursorStyle, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, Font, FontStyle,
    FontWeight, GlobalElementId, Hsla, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, RenderImage, ScrollWheelEvent, ShapedLine,
    SharedString, StrikethroughStyle, Style, TextAlign, TextRun, Transformation,
    TransformationMatrix, UTF16Selection, Window, WrappedLine, div, fill, img, point, prelude::*,
    px, quad, radians, relative, size, svg,
};
use image::Frame;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, Theme as SyntectTheme, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::actions::*;
use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::markdown::{self, BlockType, InlineKind, LineDecoration, TableLineInfo};
use crate::theme::Theme;

const LINE_HEIGHT_FACTOR: f32 = 1.6;
const BASE_FONT_SIZE: f32 = 15.0;
const PADDING_LEFT: f32 = 40.0;
const PADDING_TOP: f32 = 20.0;
const PADDING_RIGHT: f32 = 40.0;
const PADDING_BOTTOM: f32 = 200.0;
const MAX_EDITOR_WIDTH: f32 = 750.0;
const SIDEBAR_WIDTH: f32 = 280.0;
const KNOWLEDGE_BASE_ROOT: &str = "/Users/imeoer/code/我的知识库";

const CURSOR_BLINK_TICK: Duration = Duration::from_millis(16);
const CURSOR_BLINK_PAUSE_DELAY: Duration = Duration::from_millis(500);
const CURSOR_MIN_OPACITY: f32 = 0.0;
const CURSOR_MAX_OPACITY: f32 = 1.0;
const CURSOR_OPACITY_STEP: f32 = 0.03;
const CODE_BLOCK_TEXT_LEFT: f32 = 10.0;
const TODO_CHECKBOX_SIZE_FACTOR: f32 = 1.2;
const TODO_CHECKBOX_TEXT_GAP: f32 = 24.0;
const TODO_CHECKBOX_CORNER_RADIUS: f32 = 5.0;
const TODO_CHECK_ICON_SCALE: f32 = 0.52;
const TABLE_CELL_TEXT_PADDING_PX: f32 = 5.0;
const GUTTER_FOLD_ICON_SIZE: f32 = 10.0;
const HASH_SVG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/hash.svg");
const COMMENT_SVG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/comment.svg");
const CHECK_SVG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/check.svg");
const FOLD_SVG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/fold.svg");
const OPEN_SVG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/open.svg");
const LOGO_SVG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/logo.svg");
const ROOT_SVG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/filetree/all.svg");
const FILETREE_FOLD_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/fold.svg"
);
const FOLDER_OPEN_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/folder-open.svg"
);
const FOLDER_CLOSED_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/folder-closed.svg"
);
const MARKDOWN_SVG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/filetree/markdown.svg"
);
const TAB_BAR_HEIGHT: f32 = 38.0;
const TAB_CLOSE_BUTTON_SIZE: f32 = 18.0;
const SIDEBAR_ROW_HEIGHT: f32 = 28.0;
const FORMAT_MENU_WIDTH: f32 = 250.0;
const UI_SCALE_STEP: f32 = 0.1;
const MIN_UI_SCALE: f32 = 0.8;
const MAX_UI_SCALE: f32 = 1.6;

#[derive(Clone)]
struct EditorSnapshot {
    text: String,
    cursor_offset: usize,
    cursor_anchor: usize,
}

#[derive(Clone)]
struct OpenTab {
    id: String,
    title: String,
    path: Option<PathBuf>,
    snapshot: EditorSnapshot,
    is_dirty: bool,
}

#[derive(Clone, Debug)]
struct SidebarItem {
    path: PathBuf,
    name: String,
    depth: usize,
    is_dir: bool,
    is_expanded: bool,
}

#[derive(Clone)]
struct ContextMenu {
    position: Point<Pixels>,
    kind: ContextMenuKind,
}

#[derive(Clone)]
enum ContextMenuKind {
    Sidebar { path: PathBuf, is_dir: bool },
    Format,
}

#[derive(Clone, Copy)]
enum FormatMenuAction {
    Cut,
    Copy,
    Paste,
    Paragraph,
    Heading(u8),
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
enum SidebarEditMode {
    Rename { path: PathBuf, value: String },
    CreateFile { parent: PathBuf, value: String },
    CreateDir { parent: PathBuf, value: String },
}

impl SidebarEditMode {
    fn value(&self) -> &str {
        match self {
            Self::Rename { value, .. }
            | Self::CreateFile { value, .. }
            | Self::CreateDir { value, .. } => value,
        }
    }

    fn value_mut(&mut self) -> &mut String {
        match self {
            Self::Rename { value, .. }
            | Self::CreateFile { value, .. }
            | Self::CreateDir { value, .. } => value,
        }
    }
}

#[derive(Clone)]
struct FileClipboard {
    paths: Vec<PathBuf>,
    is_cut: bool,
}

pub struct EditorView {
    pub buffer: Buffer,
    pub cursor: Cursor,
    focus_handle: FocusHandle,
    theme: Theme,
    decorations: Vec<LineDecoration>,
    scroll_offset: f32,
    is_selecting: bool,
    /// Per buffer-line display info, rebuilt every frame for visible lines.
    line_display_info: Vec<Option<LineDisplayInfo>>,
    last_viewport_height: f32,
    last_viewport_width: f32,
    // Cursor blink state
    cursor_opacity: f32,
    cursor_opacity_direction: f32,
    blink_epoch: usize,
    // IME preedit state
    marked_range: Option<Range<usize>>,
    /// Checkbox inline hit areas: (bounds in screen space, buffer line index)
    checkbox_areas: Vec<(Bounds<Pixels>, usize)>,
    hovering_checkbox: bool,
    /// Image preview click areas: (bounds in screen space, buffer line index)
    image_click_areas: Vec<(Bounds<Pixels>, usize)>,
    /// Cached loaded images: path → Some(image_data) or None (not yet loaded)
    image_cache: HashMap<String, Option<Arc<RenderImage>>>,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    sidebar_root: PathBuf,
    expanded_dirs: HashSet<PathBuf>,
    selected_sidebar_path: Option<PathBuf>,
    current_file_path: Option<PathBuf>,
    sidebar_error: Option<String>,
    tabs: Vec<OpenTab>,
    active_tab_id: Option<String>,
    next_tab_id: usize,
    last_editor_left: f32,
    last_editor_width: f32,
    last_content_left: f32,
    last_text_width: f32,
    /// Origin of EditorElement bounds in window space (updated each prepaint).
    content_origin: Point<Pixels>,
    context_menu: Option<ContextMenu>,
    sidebar_edit: Option<SidebarEditMode>,
    file_clipboard: Option<FileClipboard>,
    ui_scale: f32,
    /// Lines (section starters) that are currently folded.
    folded_sections: HashSet<usize>,
    /// Image lines whose rendered preview is hidden.
    folded_image_previews: HashSet<usize>,
    /// Gutter fold icon hit areas: (bounds, line_idx, is_image_fold)
    gutter_fold_areas: Vec<(Bounds<Pixels>, usize, bool)>,
    /// Image URL-edit icon hit areas: (bounds, line_idx)
    image_edit_areas: Vec<(Bounds<Pixels>, usize)>,
    /// Which image-edit icon line is currently hovered (if any).
    hovering_image_edit_line: Option<usize>,
    /// Which gutter fold-icon line is currently hovered (if any).
    hovering_fold_line: Option<usize>,
    /// Custom sidebar width set by drag-resize (in logical pixels, pre-scaled).
    sidebar_custom_width: Option<f32>,
    sidebar_resizing: bool,
}

/// Display info for a single buffer line.
#[derive(Clone)]
struct LineDisplayInfo {
    /// The display text (with syntax markers removed if not cursor line).
    display_text: String,
    /// Mapping from display byte offset to buffer byte offset within the line.
    /// `display_to_buffer[display_byte] = buffer_byte_within_line`
    display_to_buffer: Vec<usize>,
    /// Mapping from buffer byte offset (within line) to display byte offset.
    /// `buffer_to_display[buffer_byte] = display_byte`
    buffer_to_display: Vec<usize>,
    /// Font size for this line.
    font_size: f32,
    /// Line height for this line.
    line_height: f32,
    /// Number of visual (wrapped) lines this buffer line occupies.
    visual_line_count: usize,
    /// Shaped wrapped line for precise position/index conversion.
    wrapped_line: WrappedLine,
    /// Extra height below this line (e.g. for image previews).
    extra_height: f32,
    /// Hanging indent for list continuation lines (in pixels).
    indent_px: f32,
    /// Actual text painting x-origin for this line.
    content_x: f32,
    editor_left: f32,
    editor_width: f32,
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
        let mut this = Self {
            buffer,
            cursor: Cursor::new(),
            focus_handle: cx.focus_handle(),
            theme: Theme::one_dark(),
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
        };
        this.start_blinking(cx);

        // Open EDITOR.md on startup if it exists
        let editor_md = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/EDITOR.md"));
        if editor_md.exists() {
            this.open_sidebar_file(editor_md, cx);
        }

        this
    }

    // ── Cursor blink ────────────────────────────────────────────────

    fn next_blink_epoch(&mut self) -> usize {
        self.blink_epoch += 1;
        self.blink_epoch
    }

    fn scaled(&self, value: f32) -> f32 {
        value * self.ui_scale
    }

    fn base_font_size(&self) -> f32 {
        self.scaled(BASE_FONT_SIZE)
    }

    fn padding_left(&self) -> f32 {
        self.scaled(PADDING_LEFT)
    }

    fn padding_top(&self) -> f32 {
        self.scaled(PADDING_TOP)
    }

    fn padding_right(&self) -> f32 {
        self.scaled(PADDING_RIGHT)
    }

    fn padding_bottom(&self) -> f32 {
        self.scaled(PADDING_BOTTOM)
    }

    fn max_editor_width(&self) -> f32 {
        self.scaled(MAX_EDITOR_WIDTH)
    }

    fn sidebar_width(&self) -> f32 {
        self.sidebar_custom_width
            .unwrap_or_else(|| self.scaled(SIDEBAR_WIDTH))
    }

    fn tab_bar_height(&self) -> f32 {
        self.scaled(TAB_BAR_HEIGHT)
    }

    fn tab_close_button_size(&self) -> f32 {
        self.scaled(TAB_CLOSE_BUTTON_SIZE)
    }

    fn sidebar_row_height(&self) -> f32 {
        self.scaled(SIDEBAR_ROW_HEIGHT)
    }

    fn start_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = CURSOR_MAX_OPACITY;
        self.cursor_opacity_direction = -1.0;
        let epoch = self.next_blink_epoch();
        self.schedule_blink(epoch, cx);
    }

    fn schedule_blink(&self, epoch: usize, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CURSOR_BLINK_TICK).await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.cursor_opacity += CURSOR_OPACITY_STEP * this.cursor_opacity_direction;
                    if this.cursor_opacity <= CURSOR_MIN_OPACITY {
                        this.cursor_opacity = CURSOR_MIN_OPACITY;
                        this.cursor_opacity_direction = 1.0;
                    } else if this.cursor_opacity >= CURSOR_MAX_OPACITY {
                        this.cursor_opacity = CURSOR_MAX_OPACITY;
                        this.cursor_opacity_direction = -1.0;
                    }
                    cx.notify();
                    this.schedule_blink(epoch, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn pause_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = CURSOR_MAX_OPACITY;
        self.cursor_opacity_direction = -1.0;
        let epoch = self.next_blink_epoch();
        self.persist_active_tab_state();
        cx.notify();
        // Resume blinking after a delay
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

    fn sidebar_root_name(&self) -> String {
        self.sidebar_root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.sidebar_root.display().to_string())
    }

    fn next_tab_identity(&mut self) -> String {
        let id = format!("tab-{}", self.next_tab_id);
        self.next_tab_id += 1;
        id
    }

    fn active_tab_index(&self) -> Option<usize> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().position(|tab| &tab.id == id))
    }

    fn persist_active_tab_state(&mut self) {
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].snapshot = self.current_snapshot();
            self.current_file_path = self.tabs[index].path.clone();
        }
    }

    fn activate_tab_by_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.persist_active_tab_state();
        let tab = self.tabs[index].clone();
        self.active_tab_id = Some(tab.id.clone());
        self.current_file_path = tab.path.clone();
        self.buffer = Buffer::from_text(&tab.snapshot.text);
        self.cursor.offset = tab.snapshot.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = tab.snapshot.cursor_anchor.min(self.buffer.len());
        self.marked_range = None;
        self.scroll_offset = 0.0;
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn switch_to_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if self.active_tab_id.as_deref() == Some(tab_id) {
            return;
        }
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) {
            self.activate_tab_by_index(index, cx);
        }
    }

    fn close_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let was_active = self.active_tab_id.as_deref() == Some(tab_id);
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            let id = self.next_tab_identity();
            let text = default_document_text().to_string();
            self.tabs.push(OpenTab {
                id: id.clone(),
                title: "Welcome".to_string(),
                path: None,
                snapshot: EditorSnapshot {
                    text,
                    cursor_offset: 0,
                    cursor_anchor: 0,
                },
                is_dirty: false,
            });
            self.active_tab_id = Some(id);
            self.activate_tab_by_index(0, cx);
            return;
        }
        if was_active {
            let next_index = index.min(self.tabs.len().saturating_sub(1));
            self.activate_tab_by_index(next_index, cx);
        } else {
            cx.notify();
        }
    }

    fn mark_active_tab_dirty(&mut self) {
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].is_dirty = true;
        }
    }

    fn title_for_path(path: &Path) -> String {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn create_scratch_tab(&mut self, cx: &mut Context<Self>) {
        self.persist_active_tab_state();
        let title = format!("Untitled {}", self.next_tab_id);
        let tab = OpenTab {
            id: self.next_tab_identity(),
            title,
            path: None,
            snapshot: EditorSnapshot {
                text: String::new(),
                cursor_offset: 0,
                cursor_anchor: 0,
            },
            is_dirty: false,
        };
        self.tabs.push(tab);
        if let Some(index) = self.tabs.len().checked_sub(1) {
            self.activate_tab_by_index(index, cx);
        }
    }

    fn toggle_sidebar_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.selected_sidebar_path = Some(path.clone());
        if !self.expanded_dirs.insert(path.clone()) {
            self.expanded_dirs.remove(&path);
        }
        cx.notify();
    }

    fn open_sidebar_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !is_markdown_file(&path) {
            self.selected_sidebar_path = Some(path);
            cx.notify();
            return;
        }

        if let Some(index) = self.tabs.iter().position(|tab| {
            tab.path
                .as_ref()
                .is_some_and(|open_path| open_path == &path)
        }) {
            self.selected_sidebar_path = Some(path);
            self.activate_tab_by_index(index, cx);
            return;
        }

        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = if bytes.contains(&0) {
                    format!(
                        "# Binary File\n\n{}\n\nThis file could not be opened as plain text.",
                        path.display()
                    )
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                };
                let tab = OpenTab {
                    id: self.next_tab_identity(),
                    title: Self::title_for_path(&path),
                    path: Some(path.clone()),
                    snapshot: EditorSnapshot {
                        text,
                        cursor_offset: 0,
                        cursor_anchor: 0,
                    },
                    is_dirty: false,
                };
                self.persist_active_tab_state();
                self.tabs.push(tab);
                self.selected_sidebar_path = Some(path.clone());
                self.sidebar_error = None;
                if let Some(index) = self.tabs.len().checked_sub(1) {
                    self.activate_tab_by_index(index, cx);
                }
            }
            Err(error) => {
                self.sidebar_error = Some(format!("Failed to open file: {error}"));
                self.selected_sidebar_path = Some(path);
                cx.notify();
            }
        }
    }

    fn sorted_dir_entries(path: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(Result::ok).collect();
        entries.sort_by(|left, right| {
            let left_is_dir = left.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let right_is_dir = right.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            right_is_dir.cmp(&left_is_dir).then_with(|| {
                left.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&right.file_name().to_string_lossy().to_lowercase())
            })
        });
        Ok(entries)
    }

    fn collect_sidebar_items(&self, path: &Path, depth: usize, items: &mut Vec<SidebarItem>) {
        let Ok(entries) = Self::sorted_dir_entries(path) else {
            return;
        };

        for entry in entries {
            let entry_path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            if !is_dir && !is_markdown_file(&entry_path) {
                continue;
            }
            let is_expanded = is_dir && self.expanded_dirs.contains(&entry_path);
            items.push(SidebarItem {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry_path.clone(),
                depth,
                is_dir,
                is_expanded,
            });
            if is_expanded {
                self.collect_sidebar_items(&entry_path, depth + 1, items);
            }
        }
    }

    fn sidebar_items(&self) -> Vec<SidebarItem> {
        let mut items = Vec::new();
        if self.sidebar_error.is_none() && self.sidebar_root.exists() {
            self.collect_sidebar_items(&self.sidebar_root, 1, &mut items);
        }
        items
    }

    // ── Context menu & file operations ─────────────────────────────

    fn open_sidebar_context_menu(
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

    fn open_format_context_menu(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Format,
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn start_rename(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let current_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.sidebar_edit = Some(SidebarEditMode::Rename {
            path,
            value: current_name,
        });
        self.context_menu = None;
        cx.notify();
    }

    fn start_create_file(&mut self, parent: PathBuf, cx: &mut Context<Self>) {
        self.expanded_dirs.insert(parent.clone());
        self.sidebar_edit = Some(SidebarEditMode::CreateFile {
            parent,
            value: String::new(),
        });
        self.context_menu = None;
        cx.notify();
    }

    fn start_create_dir(&mut self, parent: PathBuf, cx: &mut Context<Self>) {
        self.expanded_dirs.insert(parent.clone());
        self.sidebar_edit = Some(SidebarEditMode::CreateDir {
            parent,
            value: String::new(),
        });
        self.context_menu = None;
        cx.notify();
    }

    fn commit_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.sidebar_edit.take() else {
            return;
        };
        match edit {
            SidebarEditMode::Rename {
                ref path,
                ref value,
            } => {
                let trimmed = value.trim();
                if !trimmed.is_empty()
                    && trimmed != path.file_name().and_then(|n| n.to_str()).unwrap_or("")
                {
                    let new_path = path.with_file_name(trimmed);
                    if let Err(e) = std::fs::rename(path, &new_path) {
                        self.sidebar_error = Some(format!("Rename failed: {e}"));
                    } else {
                        // Update any open tabs that had the old path
                        for tab in &mut self.tabs {
                            if tab.path.as_deref() == Some(path.as_path()) {
                                tab.path = Some(new_path.clone());
                                tab.title = Self::title_for_path(&new_path);
                            }
                        }
                        if self.selected_sidebar_path.as_deref() == Some(path.as_path()) {
                            self.selected_sidebar_path = Some(new_path.clone());
                        }
                        if self.current_file_path.as_deref() == Some(path.as_path()) {
                            self.current_file_path = Some(new_path);
                        }
                    }
                }
            }
            SidebarEditMode::CreateFile {
                ref parent,
                ref value,
            } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let new_path = parent.join(trimmed);
                    if let Err(e) = std::fs::File::create(&new_path) {
                        self.sidebar_error = Some(format!("Create file failed: {e}"));
                    } else if is_markdown_file(&new_path) {
                        self.open_sidebar_file(new_path, cx);
                        return;
                    }
                }
            }
            SidebarEditMode::CreateDir {
                ref parent,
                ref value,
            } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let new_path = parent.join(trimmed);
                    if let Err(e) = std::fs::create_dir(&new_path) {
                        self.sidebar_error = Some(format!("Create folder failed: {e}"));
                    } else {
                        self.expanded_dirs.insert(new_path);
                    }
                }
            }
        }
        cx.notify();
    }

    fn cancel_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_edit.take().is_some() {
            cx.notify();
        }
    }

    fn delete_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(e) = result {
            self.sidebar_error = Some(format!("Delete failed: {e}"));
        } else {
            // Close any tabs that had this path (or paths inside this dir)
            self.tabs
                .retain(|tab| tab.path.as_ref().is_none_or(|p| !p.starts_with(&path)));
            if self.tabs.is_empty() {
                self.create_scratch_tab(cx);
                return;
            }
            // If active tab was deleted, activate first tab
            let active_still_exists = self
                .active_tab_id
                .as_ref()
                .is_some_and(|id| self.tabs.iter().any(|t| &t.id == id));
            if !active_still_exists {
                self.activate_tab_by_index(0, cx);
            }
        }
        self.context_menu = None;
        cx.notify();
    }

    fn cut_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.file_clipboard = Some(FileClipboard {
            paths: vec![path],
            is_cut: true,
        });
        self.context_menu = None;
        cx.notify();
    }

    fn copy_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.file_clipboard = Some(FileClipboard {
            paths: vec![path],
            is_cut: false,
        });
        self.context_menu = None;
        cx.notify();
    }

    fn paste_into_dir(&mut self, target_dir: PathBuf, cx: &mut Context<Self>) {
        let Some(clipboard) = self.file_clipboard.take() else {
            return;
        };
        for src in &clipboard.paths {
            let Some(name) = src.file_name() else {
                continue;
            };
            let dst = target_dir.join(name);
            if clipboard.is_cut {
                if let Err(e) = std::fs::rename(src, &dst) {
                    self.sidebar_error = Some(format!("Move failed: {e}"));
                } else {
                    // Update open tabs
                    for tab in &mut self.tabs {
                        if tab.path.as_deref() == Some(src.as_path()) {
                            tab.path = Some(dst.clone());
                            tab.title = Self::title_for_path(&dst);
                        }
                    }
                }
            } else {
                // Copy: for files only
                if src.is_file() {
                    if let Err(e) = std::fs::copy(src, &dst) {
                        self.sidebar_error = Some(format!("Copy failed: {e}"));
                    }
                }
            }
        }
        self.context_menu = None;
        cx.notify();
    }

    fn reveal_in_finder(&self, path: &Path) {
        let _ = std::process::Command::new("open")
            .arg("--reveal")
            .arg(path)
            .spawn();
    }

    fn copy_selection_to_clipboard(&self, cx: &mut Context<Self>) {
        if self.cursor.has_selection() {
            let range = self.cursor.selection_range();
            let text = self.buffer.text()[range].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut_selection_to_clipboard(&mut self, cx: &mut Context<Self>) {
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

    fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.checkpoint();
            self.cursor.insert(&mut self.buffer, &text);
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
        }
    }

    fn set_heading_level(&mut self, level: u8) {
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let line_start = self.buffer.line_start(line);
        let line_end = self.buffer.line_end(line);
        let line_text = self.buffer.line_content(line).to_string();
        let stripped = line_text.trim_start_matches('#');
        let existing_hashes = line_text.len() - stripped.len();
        let content = if existing_hashes > 0 {
            stripped.strip_prefix(' ').unwrap_or(stripped)
        } else {
            line_text.as_str()
        };
        let replacement = if level == 0 {
            content.to_string()
        } else {
            format!("{} {}", "#".repeat(level as usize), content)
        };
        self.buffer.replace(line_start..line_end, &replacement);
        let new_cursor = (line_start + replacement.len()).min(self.buffer.len());
        self.cursor.move_to(new_cursor, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
    }

    fn toggle_horizontal_rule_at_cursor(&mut self) {
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

    fn set_ui_scale(&mut self, new_scale: f32, cx: &mut Context<Self>) {
        let clamped = new_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        if (clamped - self.ui_scale).abs() < f32::EPSILON {
            return;
        }
        // Scale custom sidebar width proportionally
        if let Some(ref mut w) = self.sidebar_custom_width {
            *w = *w * (clamped / self.ui_scale);
        }
        self.ui_scale = clamped;
        self.clamp_scroll();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        cx.notify();
    }

    // ── Fold/collapse logic ────────────────────────────────────────

    /// Returns the range of buffer lines hidden when `line` is folded, or
    /// `None` if the line does not start a foldable section.
    fn fold_range_for_line(&self, line: usize) -> Option<Range<usize>> {
        let decoration = self.decorations.get(line)?;
        let line_count = self.buffer.line_count();
        match &decoration.block_type {
            BlockType::Heading(level) => {
                let lv = *level;
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::Heading(h)) if *h <= lv
                        )
                    })
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            BlockType::CodeBlockFence => {
                // Only foldable on the opening fence.
                if line > 0
                    && matches!(
                        self.decorations.get(line - 1).map(|d| &d.block_type),
                        Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                    )
                {
                    return None; // This is the closing fence.
                }
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::CodeBlockFence)
                        )
                    })
                    .map(|l| l + 1)
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_) => {
                let content = self.buffer.line_content(line);
                let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
                if line + 1 >= line_count {
                    return None;
                }
                let next_content = self.buffer.line_content(line + 1);
                let next_indent =
                    next_content.len() - next_content.trim_start_matches([' ', '\t']).len();
                if next_indent <= indent {
                    return None;
                }
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        let lc = self.buffer.line_content(l);
                        let li = lc.len() - lc.trim_start_matches([' ', '\t']).len();
                        !lc.trim().is_empty() && li <= indent
                    })
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            _ => None,
        }
    }

    fn toggle_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        if self.folded_sections.contains(&line) {
            self.folded_sections.remove(&line);
        } else if let Some(range) = self.fold_range_for_line(line) {
            self.folded_sections.insert(line);
            // Move cursor out of the folded range.
            let cursor_line = self.cursor_line();
            if range.contains(&cursor_line) {
                let new_offset = self.buffer.line_end(line);
                self.cursor.move_to(new_offset, &self.buffer);
            }
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn toggle_image_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        if !self.folded_image_previews.remove(&line) {
            self.folded_image_previews.insert(line);
        }
        cx.notify();
    }

    fn pick_image_for_line(&self, line: usize, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { pick_image_file_blocking() })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    editor.replace_image_url_on_line(line, &path, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn pick_workspace_folder(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { pick_folder_blocking() })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    editor.sidebar_root = path.clone();
                    editor.expanded_dirs.clear();
                    editor.expanded_dirs.insert(path.clone());
                    editor.sidebar_error = None;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn replace_image_url_on_line(
        &mut self,
        line: usize,
        new_path: &PathBuf,
        cx: &mut Context<Self>,
    ) {
        let line_content = self.buffer.line_content(line).to_string();
        let line_start = self.buffer.line_start(line);
        if let Some(bracket_pos) = line_content.find("](") {
            let url_start = bracket_pos + 2;
            if let Some(close_pos) = line_content[url_start..].find(')') {
                let url_end = url_start + close_pos;
                let old_url = line_content[url_start..url_end].to_string();
                let new_url = new_path.to_string_lossy().to_string();
                self.checkpoint();
                self.buffer
                    .replace(line_start + url_start..line_start + url_end, &new_url);
                self.image_cache.remove(&old_url);
                self.reparse();
                self.pause_blinking(cx);
            }
        }
    }

    fn execute_format_menu_action(&mut self, action: FormatMenuAction, cx: &mut Context<Self>) {
        match action {
            FormatMenuAction::Cut => self.cut_selection_to_clipboard(cx),
            FormatMenuAction::Copy => self.copy_selection_to_clipboard(cx),
            FormatMenuAction::Paste => self.paste_from_clipboard(cx),
            FormatMenuAction::Paragraph => {
                self.checkpoint();
                self.set_heading_level(0);
                self.pause_blinking(cx);
            }
            FormatMenuAction::Heading(level) => {
                self.checkpoint();
                self.set_heading_level(level);
                self.pause_blinking(cx);
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
            FormatMenuAction::Image => {
                self.checkpoint();
                self.insert_link_or_image("![", "](image.png)", "alt");
                self.pause_blinking(cx);
            }
        }
        self.close_context_menu(cx);
    }

    // ── Sidebar input field key handling ───────────────────────────

    fn sidebar_edit_append(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(ref mut edit) = self.sidebar_edit {
            edit.value_mut().push_str(text);
            cx.notify();
        }
    }

    fn sidebar_edit_backspace(&mut self, cx: &mut Context<Self>) {
        if let Some(ref mut edit) = self.sidebar_edit {
            let val = edit.value_mut();
            // Remove last UTF-8 char
            if let Some((last_char_start, _)) = val.char_indices().next_back() {
                val.truncate(last_char_start);
            }
            cx.notify();
        }
    }

    // ── Context menu rendering ──────────────────────────────────────

    fn render_context_menu_item(
        &self,
        label: &str,
        is_destructive: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let text_color = if is_destructive {
            self.theme.danger
        } else {
            self.theme.foreground
        };
        div()
            .w_full()
            .h(px(28.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .text_size(px(13.0))
            .text_color(text_color)
            .hover(|s| s.bg(self.theme.panel_hover))
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

    fn render_format_context_menu_item(
        &self,
        label: &str,
        shortcut: Option<&str>,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .w_full()
            .h(px(self.scaled(28.0)))
            .px(px(self.scaled(12.0)))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(self.scaled(13.0)))
            .text_color(self.theme.foreground)
            .hover(|s| s.bg(self.theme.panel_hover))
            .cursor(CursorStyle::Arrow)
            .child(label.to_string())
            .when_some(shortcut, |this, shortcut| {
                this.child(
                    div()
                        .text_size(px(self.scaled(11.5)))
                        .text_color(self.theme.panel_muted)
                        .child(shortcut.to_string()),
                )
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
        div()
            .w_full()
            .h(px(1.0))
            .my(px(3.0))
            .bg(self.theme.panel_border)
            .into_any_element()
    }

    fn render_sidebar_context_menu(
        &self,
        path: PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let has_clipboard = self.file_clipboard.is_some();
        let parent_dir = if is_dir {
            path.clone()
        } else {
            path.parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| path.clone())
        };

        let mut items: Vec<AnyElement> = Vec::new();

        {
            let pd = parent_dir.clone();
            items.push(
                self.render_context_menu_item("New File", false, cx, move |this, _, cx| {
                    this.start_create_file(pd.clone(), cx);
                }),
            );
        }
        {
            let pd = parent_dir.clone();
            items.push(self.render_context_menu_item(
                "New Folder",
                false,
                cx,
                move |this, _, cx| {
                    this.start_create_dir(pd.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("Rename", false, cx, move |this, _, cx| {
                    this.start_rename(p.clone(), cx);
                }),
            );
        }
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("Delete", true, cx, move |this, _, cx| {
                    this.delete_path(p.clone(), cx);
                }),
            );
        }
        items.push(self.render_context_menu_separator());
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("Cut", false, cx, move |this, _, cx| {
                    this.cut_path(p.clone(), cx);
                }),
            );
        }
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("Copy", false, cx, move |this, _, cx| {
                    this.copy_path(p.clone(), cx);
                }),
            );
        }
        if has_clipboard {
            let pd = parent_dir.clone();
            items.push(
                self.render_context_menu_item("Paste", false, cx, move |this, _, cx| {
                    this.paste_into_dir(pd.clone(), cx);
                }),
            );
        }
        items.push(self.render_context_menu_separator());
        {
            let p = path.clone();
            items.push(self.render_context_menu_item(
                "Reveal in Finder",
                false,
                cx,
                move |this, _, _cx| {
                    this.reveal_in_finder(&p);
                },
            ));
        }

        items
    }

    fn render_format_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut items: Vec<AnyElement> = Vec::new();

        let menu_items = [
            ("Cut", Some("Cmd+X"), Some(FormatMenuAction::Cut)),
            ("Copy", Some("Cmd+C"), Some(FormatMenuAction::Copy)),
            ("Paste", Some("Cmd+V"), Some(FormatMenuAction::Paste)),
            ("", None, None),
            ("Paragraph", None, Some(FormatMenuAction::Paragraph)),
            (
                "Heading 1",
                Some("Cmd+1"),
                Some(FormatMenuAction::Heading(1)),
            ),
            (
                "Heading 2",
                Some("Cmd+2"),
                Some(FormatMenuAction::Heading(2)),
            ),
            (
                "Heading 3",
                Some("Cmd+3"),
                Some(FormatMenuAction::Heading(3)),
            ),
            (
                "Heading 4",
                Some("Cmd+4"),
                Some(FormatMenuAction::Heading(4)),
            ),
            (
                "Heading 5",
                Some("Cmd+5"),
                Some(FormatMenuAction::Heading(5)),
            ),
            (
                "Heading 6",
                Some("Cmd+6"),
                Some(FormatMenuAction::Heading(6)),
            ),
            ("", None, None),
            ("Bold", Some("Cmd+B"), Some(FormatMenuAction::Bold)),
            ("Italic", Some("Cmd+I"), Some(FormatMenuAction::Italic)),
            (
                "Strikethrough",
                Some("Cmd+Shift+S"),
                Some(FormatMenuAction::Strikethrough),
            ),
            (
                "Inline Code",
                Some("Cmd+E"),
                Some(FormatMenuAction::InlineCode),
            ),
            ("", None, None),
            (
                "Code Block",
                Some("Cmd+Alt+C"),
                Some(FormatMenuAction::CodeBlock),
            ),
            (
                "Horizontal Rule",
                Some("Cmd+Alt+H"),
                Some(FormatMenuAction::HorizontalRule),
            ),
            ("Quote", Some("Cmd+Alt+Q"), Some(FormatMenuAction::Quote)),
            (
                "Unordered List",
                Some("Cmd+Alt+U"),
                Some(FormatMenuAction::UnorderedList),
            ),
            (
                "Ordered List",
                Some("Cmd+Alt+O"),
                Some(FormatMenuAction::OrderedList),
            ),
            ("Todo", Some("Cmd+Alt+T"), Some(FormatMenuAction::Todo)),
            ("", None, None),
            ("Link", Some("Cmd+K"), Some(FormatMenuAction::Link)),
            ("Image", Some("Cmd+Shift+I"), Some(FormatMenuAction::Image)),
        ];

        for (label, shortcut, action) in menu_items {
            if let Some(action) = action {
                items.push(self.render_format_context_menu_item(
                    label,
                    shortcut,
                    cx,
                    move |this, _, cx| {
                        this.execute_format_menu_action(action, cx);
                    },
                ));
            } else {
                items.push(self.render_context_menu_separator());
            }
        }

        items
    }

    fn render_context_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = &self.context_menu else {
            return div().into_any_element();
        };
        let pos_x: f32 = menu.position.x.into();
        let pos_y: f32 = menu.position.y.into();

        let (items, width) = match &menu.kind {
            ContextMenuKind::Sidebar { path, is_dir } => (
                self.render_sidebar_context_menu(path.clone(), *is_dir, cx),
                self.scaled(180.0),
            ),
            ContextMenuKind::Format => (
                self.render_format_context_menu(cx),
                self.scaled(FORMAT_MENU_WIDTH),
            ),
        };

        div()
            .absolute()
            .left(px(pos_x))
            .top(px(pos_y))
            .w(px(width))
            .py(px(self.scaled(4.0)))
            .bg(self.theme.panel_surface)
            .border_1()
            .border_color(self.theme.panel_border)
            .rounded(px(self.scaled(6.0)))
            .shadow_md()
            .children(items)
            .cursor(CursorStyle::Arrow)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event, _window, _cx| {
                    // Swallow click so it doesn't propagate to the dismiss handler
                }),
            )
            .into_any_element()
    }

    // ── Sidebar edit row rendering ──────────────────────────────────

    fn render_sidebar_edit_row(
        &self,
        value: &str,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let left_padding = self.scaled(12.0 + depth as f32 * 14.0 + 10.0 + 6.0 + 13.0 + 6.0); // mirror item layout
        div()
            .w_full()
            .h(px(self.sidebar_row_height()))
            .pl(px(self.scaled(8.0)))
            .pr(px(self.scaled(10.0)))
            .flex()
            .items_center()
            .bg(self.theme.panel_selected)
            .child(
                div()
                    .flex_1()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(2.0)))
                    .h_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .h(px(20.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .bg(self.theme.background)
                            .border_1()
                            .border_color(self.theme.accent)
                            .rounded(px(3.0))
                            .text_size(px(13.0))
                            .text_color(self.theme.tab_active_text)
                            .child(div().flex_1().text_ellipsis().child(if value.is_empty() {
                                div().text_color(self.theme.panel_muted).child("Name…")
                            } else {
                                div().child(format!("{value}|"))
                            })),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event, _window, _cx| {
                    // Focus already on editor root; just stop propagation to parent dismiss handler
                }),
            )
            .into_any_element()
    }

    fn render_sidebar_item(&self, item: SidebarItem, cx: &mut Context<Self>) -> AnyElement {
        // If this item is being renamed, show the edit row instead
        if let Some(SidebarEditMode::Rename {
            path: ref rp,
            ref value,
        }) = self.sidebar_edit
        {
            if *rp == item.path {
                return self.render_sidebar_edit_row(value, item.depth, cx);
            }
        }

        let is_selected = self
            .selected_sidebar_path
            .as_ref()
            .is_some_and(|selected| selected == &item.path)
            || self
                .current_file_path
                .as_ref()
                .is_some_and(|current| current == &item.path);
        let is_active_file = self
            .current_file_path
            .as_ref()
            .is_some_and(|current| current == &item.path);
        let path = item.path.clone();
        let is_dir = item.is_dir;
        let left_padding = self.scaled(12.0 + item.depth as f32 * 14.0);
        let row_bg = if is_selected {
            self.theme.panel_selected
        } else {
            gpui::transparent_black()
        };
        let icon_color = if is_selected {
            self.theme.tab_active_text
        } else {
            self.theme.panel_muted
        };
        let label_color = if is_selected {
            self.theme.tab_active_text
        } else {
            self.theme.foreground
        };
        let folder_icon = if item.is_expanded {
            FOLDER_OPEN_SVG_PATH
        } else {
            FOLDER_CLOSED_SVG_PATH
        };

        div()
            .id(SharedString::from(format!("sidebar-{}", path.display())))
            .w_full()
            .h(px(self.scaled(26.0)))
            .flex()
            .items_center()
            .bg(row_bg)
            .hover(|style| style.bg(self.theme.panel_hover))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(8.0)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(4.0)))
                    .child(
                        // Chevron arrow — fold.svg (right-pointing), rotated 90° when expanded
                        div()
                            .w(px(self.scaled(16.0)))
                            .h(px(self.scaled(18.0)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .child(if item.is_dir {
                                svg()
                                    .path(FILETREE_FOLD_SVG_PATH)
                                    .size(px(self.scaled(10.0)))
                                    .text_color(self.theme.panel_muted)
                                    .when(item.is_expanded, |s| {
                                        s.with_transformation(Transformation::rotate(radians(
                                            std::f32::consts::FRAC_PI_2,
                                        )))
                                    })
                                    .into_any_element()
                            } else {
                                div().w(px(self.scaled(10.0))).into_any_element()
                            }),
                    )
                    .child(
                        svg()
                            .path(if item.is_dir {
                                folder_icon
                            } else {
                                MARKDOWN_SVG_PATH
                            })
                            .size(px(self.scaled(14.0)))
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(self.scaled(13.0)))
                            .text_color(label_color)
                            .truncate()
                            .child(item.name),
                    )
                    .when(is_active_file, |this| {
                        this.child(
                            div()
                                .w(px(self.scaled(5.0)))
                                .h(px(self.scaled(5.0)))
                                .rounded_full()
                                .flex_none()
                                .bg(self.theme.accent),
                        )
                    }),
            )
            .on_click({
                let path = path.clone();
                cx.listener(move |this, _, _, cx| {
                    if path.is_dir() {
                        this.toggle_sidebar_directory(path.clone(), cx);
                    } else {
                        this.open_sidebar_file(path.clone(), cx);
                    }
                })
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_sidebar_context_menu(path.clone(), is_dir, event.position, cx);
                }),
            )
            .into_any_element()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_root = self.sidebar_root.exists() && self.sidebar_error.is_none();
        let mut content: Vec<AnyElement> = Vec::new();

        // ── 30px top spacer ────────────────────────────────────────────────
        content.push(
            div()
                .w_full()
                .h(px(self.scaled(30.0)))
                .flex_none()
                .into_any_element(),
        );

        // ── Logo ───────────────────────────────────────────────────────────
        content.push(
            div()
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .py(px(self.scaled(12.0)))
                .child(
                    img(PathBuf::from(LOGO_SVG_PATH))
                        .w(px(self.scaled(55.0)))
                        .h(px(self.scaled(55.0))),
                )
                .into_any_element(),
        );

        // ── Workspace header row ────────────────────────────────────────────
        let root_name = self.sidebar_root_name().to_uppercase();
        let header = div()
            .w_full()
            .h(px(self.scaled(28.0)))
            .px(px(self.scaled(4.0)))
            .pl(px(self.scaled(14.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .flex_none()
            .hover(|s| s.bg(self.theme.panel_hover))
            .child(
                svg()
                    .path(ROOT_SVG_PATH)
                    .size(px(self.scaled(14.0)))
                    .text_color(self.theme.panel_muted),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(self.scaled(12.0)))
                    .font_weight(FontWeight::BOLD)
                    .text_color(self.theme.tab_active_text)
                    .overflow_hidden()
                    .child(root_name),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pick_workspace_folder(cx);
                }),
            )
            .into_any_element();
        content.push(header);

        if let Some(error) = &self.sidebar_error {
            // Error state
            content.push(
                div()
                    .w_full()
                    .px(px(self.scaled(14.0)))
                    .py(px(self.scaled(12.0)))
                    .text_size(px(self.scaled(12.0)))
                    .text_color(self.theme.danger)
                    .child(error.clone())
                    .into_any_element(),
            );
        } else if !has_root {
            // Empty / no-workspace state: hint + open-directory button
            content.push(
                div()
                    .w_full()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(self.scaled(12.0)))
                    .px(px(self.scaled(16.0)))
                    .child(
                        div()
                            .text_size(px(self.scaled(12.0)))
                            .text_color(self.theme.panel_muted)
                            .text_align(gpui::TextAlign::Center)
                            .child("单击或右键\n打开目录"),
                    )
                    .child(
                        div()
                            .px(px(self.scaled(16.0)))
                            .py(px(self.scaled(5.0)))
                            .rounded_full()
                            .border(px(1.5))
                            .border_dashed()
                            .border_color(self.theme.panel_border)
                            .text_size(px(self.scaled(12.0)))
                            .text_color(self.theme.panel_muted)
                            .hover(|s| s.bg(self.theme.panel_hover))
                            .cursor(CursorStyle::Arrow)
                            .child("打开目录")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.pick_workspace_folder(cx);
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        } else {
            // Determine create-mode details for injection
            let create_parent = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { parent, .. })
                | Some(SidebarEditMode::CreateDir { parent, .. }) => Some(parent.clone()),
                _ => None,
            };
            let create_depth = create_parent.as_ref().map(|p| {
                p.strip_prefix(&self.sidebar_root)
                    .map(|rel| rel.components().count() + 1)
                    .unwrap_or(1)
            });
            let create_value = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { value, .. })
                | Some(SidebarEditMode::CreateDir { value, .. }) => Some(value.as_str()),
                _ => None,
            };

            let items = self.sidebar_items();
            let mut injected = false;
            for item in items {
                content.push(self.render_sidebar_item(item.clone(), cx));
                if let Some(ref parent) = create_parent {
                    if !injected && item.is_dir && &item.path == parent {
                        let depth = create_depth.unwrap_or(1);
                        let value = create_value.unwrap_or("");
                        content.push(self.render_sidebar_edit_row(value, depth, cx));
                        injected = true;
                    }
                }
            }
            if !injected {
                if let Some(depth) = create_depth {
                    let value = create_value.unwrap_or("");
                    content.push(self.render_sidebar_edit_row(value, depth, cx));
                }
            }
        }

        // ── 30px bottom spacer ─────────────────────────────────────────────
        content.push(
            div()
                .w_full()
                .h(px(self.scaled(30.0)))
                .flex_none()
                .into_any_element(),
        );

        div()
            .id("sidebar")
            .w(px(self.sidebar_width()))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(self.theme.panel_bg)
            .overflow_y_scroll()
            .children(content)
            .into_any_element()
    }

    fn render_tab(&self, tab: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        let is_active = self.active_tab_id.as_deref() == Some(tab.id.as_str());
        let tab_id = tab.id.clone();
        let close_id = tab.id.clone();
        let title = if tab.is_dirty {
            format!("{} •", tab.title)
        } else {
            tab.title.clone()
        };

        div()
            .id(SharedString::from(format!("tab-{}", tab.id)))
            .h(px(self.tab_bar_height()))
            .min_w(px(self.scaled(60.0)))
            .max_w(px(self.scaled(180.0)))
            .pl(px(self.scaled(12.0)))
            .pr(px(self.scaled(8.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .flex_none()
            .bg(if is_active {
                self.theme.tab_active_bg
            } else {
                gpui::transparent_black()
            })
            .border_r_1()
            .border_color(self.theme.tab_border)
            .hover(|style| style.bg(self.theme.tab_active_bg))
            .child(
                svg()
                    .path(MARKDOWN_SVG_PATH)
                    .size(px(self.scaled(14.0)))
                    .text_color(if is_active {
                        self.theme.panel_muted
                    } else {
                        self.theme.panel_muted
                    })
                    .opacity(if is_active { 0.6 } else { 0.4 }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_size(px(self.scaled(12.0)))
                    .font_weight(if is_active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if is_active {
                        self.theme.tab_active_text
                    } else {
                        self.theme.foreground
                    })
                    .child(title),
            )
            .child(
                div()
                    .w(px(self.scaled(16.0)))
                    .h(px(self.scaled(16.0)))
                    .rounded_full()
                    .flex()
                    .flex_none()
                    .justify_center()
                    .items_center()
                    .text_size(px(self.scaled(10.0)))
                    .text_color(self.theme.foreground)
                    .opacity(if is_active || tab.is_dirty { 1.0 } else { 0.0 })
                    .hover(|style| style.bg(self.theme.panel_hover))
                    .child(if tab.is_dirty { "●" } else { "✕" })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.close_tab(&close_id, cx);
                        }),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.switch_to_tab(&tab_id, cx);
                }),
            )
            .into_any_element()
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut tab_items: Vec<AnyElement> = self
            .tabs
            .iter()
            .map(|tab| self.render_tab(tab, cx))
            .collect();

        // New-tab button
        tab_items.push(
            div()
                .h(px(self.tab_bar_height()))
                .w(px(self.scaled(36.0)))
                .flex_none()
                .flex()
                .justify_center()
                .items_center()
                .text_size(px(self.scaled(16.0)))
                .text_color(self.theme.panel_muted)
                .hover(|style| style.text_color(self.theme.tab_active_text))
                .child("+")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.create_scratch_tab(cx);
                    }),
                )
                .into_any_element(),
        );

        div()
            .id("tab-bar")
            .w_full()
            .h(px(self.tab_bar_height()))
            .flex()
            .items_center()
            .bg(self.theme.tab_bar_bg)
            .border_b_1()
            .border_color(self.theme.tab_border)
            .overflow_x_scroll()
            .children(tab_items)
            .into_any_element()
    }

    fn current_snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.buffer.text().to_string(),
            cursor_offset: self.cursor.offset,
            cursor_anchor: self.cursor.anchor,
        }
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.buffer = Buffer::from_text(&snapshot.text);
        self.cursor.offset = snapshot.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = snapshot.cursor_anchor.min(self.buffer.len());
        self.marked_range = None;
        self.reparse();
        self.scroll_to_cursor();
    }

    fn checkpoint(&mut self) {
        self.undo_stack.push(self.current_snapshot());
        self.redo_stack.clear();
        self.mark_active_tab_dirty();
        if self.undo_stack.len() > 300 {
            self.undo_stack.remove(0);
        }
    }

    fn toggle_current_line_prefix(&mut self, prefix: &str) {
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

    fn indent_current_lines(&mut self, spaces: &str) {
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

    fn insert_link_or_image(&mut self, prefix: &str, suffix: &str, placeholder: &str) {
        let range = self.cursor.selection_range();
        let selected = self.buffer.text()[range.clone()].to_string();
        let replacement = if selected.is_empty() {
            format!("{}{placeholder}{}", prefix, suffix)
        } else {
            format!("{}{}{}", prefix, selected, suffix)
        };
        self.buffer.replace(range.clone(), &replacement);
        let anchor = range.start + prefix.len();
        let offset = if selected.is_empty() {
            anchor + placeholder.len()
        } else {
            anchor + selected.len()
        };
        self.cursor.anchor = anchor;
        self.cursor.offset = offset;
        self.reparse();
        self.scroll_to_cursor();
    }

    fn reparse(&mut self) {
        self.decorations = markdown::parse_decorations(self.buffer.text());
        self.auto_align_table();
    }

    /// Auto-align table pipes so `|` characters line up vertically across rows.
    fn auto_align_table(&mut self) {
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

        // Parse all rows to get cells
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

        // Compute max column count and column widths (ignoring separator rows)
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
        // Minimum column width
        for w in col_widths.iter_mut() {
            *w = (*w).max(3);
        }

        // Track cursor position within its cell
        let cursor_offset = self.cursor.offset;
        let cursor_line_start = self.buffer.line_start(cursor_line);
        let cursor_col_in_line = cursor_offset - cursor_line_start;
        let cursor_row_content = self.buffer.line_content(cursor_line).to_string();

        // Find which cell the cursor is in and its position within that cell
        let (cursor_cell_idx, cursor_pos_in_cell) = {
            let mut cell_idx = 0;
            let mut pos_in_cell = 0;
            let mut col = 0;
            let mut in_cell_start = 0;
            let trimmed = cursor_row_content.trim();
            let offset = cursor_row_content.len() - cursor_row_content.trim_start().len();
            // Skip leading |
            let mut scanning = if trimmed.starts_with('|') {
                offset + 1
            } else {
                offset
            };
            in_cell_start = scanning;
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

        // Build formatted table text
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

        // Compute new cursor position
        let cursor_row_in_table = cursor_line - table_start;
        let new_cursor_line_start = {
            let mut offset = table_text_start;
            for i in 0..cursor_row_in_table {
                offset += new_lines[i].len() + 1; // +1 for \n
            }
            offset
        };

        // Find the cell start in the new formatted line
        let new_line = &new_lines[cursor_row_in_table];
        let new_cursor_offset = {
            let mut cell_count = 0;
            let mut cell_start = 1; // skip leading |
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

    fn toggle_checkbox(&mut self, line: usize) {
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

    fn cursor_line(&self) -> usize {
        self.buffer.offset_to_line_col(self.cursor.offset).0
    }

    fn font_size_for_block(&self, block_type: &BlockType) -> f32 {
        let base = self.base_font_size();
        match block_type {
            BlockType::Heading(1) => base * 1.8,
            BlockType::Heading(2) => base * 1.5,
            BlockType::Heading(3) => base * 1.3,
            BlockType::Heading(4) => base * 1.15,
            BlockType::Heading(5) => base * 1.05,
            BlockType::Heading(6) => base * 1.0,
            _ => base,
        }
    }

    fn line_height_for_block(&self, block_type: &BlockType) -> f32 {
        self.font_size_for_block(block_type) * LINE_HEIGHT_FACTOR
    }

    /// Height of a buffer line in pixels, accounting for wrapped visual lines.
    fn visual_height_for_line(&self, line: usize) -> f32 {
        if let Some(Some(info)) = self.line_display_info.get(line) {
            info.line_height * info.visual_line_count as f32 + info.extra_height
        } else {
            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            self.line_height_for_block(block_type)
        }
    }

    fn line_y_offset(&self, target_line: usize) -> f32 {
        let mut y = self.padding_top();
        for line in 0..target_line.min(self.buffer.line_count()) {
            y += self.visual_height_for_line(line);
        }
        y
    }

    fn total_content_height(&self) -> f32 {
        self.line_y_offset(self.buffer.line_count()) + self.padding_bottom()
    }

    fn line_at_y(&self, y: f32) -> usize {
        let mut current_y = self.padding_top();
        for line in 0..self.buffer.line_count() {
            let lh = self.visual_height_for_line(line);
            if y < current_y + lh {
                return line;
            }
            current_y += lh;
        }
        self.buffer.line_count().saturating_sub(1)
    }

    fn scroll_to_cursor(&mut self) {
        let cursor_line = self.cursor_line();
        let cursor_y = self.line_y_offset(cursor_line);
        let cursor_line_height = self.visual_height_for_line(cursor_line);

        if cursor_y < self.scroll_offset + self.padding_top() {
            self.scroll_offset = (cursor_y - self.padding_top()).max(0.0);
        }
        if cursor_y + cursor_line_height
            > self.scroll_offset + self.last_viewport_height - self.padding_top()
        {
            self.scroll_offset =
                cursor_y + cursor_line_height - self.last_viewport_height + self.padding_top();
        }
    }

    fn clamp_scroll(&mut self) {
        let max_scroll = (self.total_content_height() - self.last_viewport_height).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);
    }

    // ── Visual wrap-line cursor movement helpers ─────────────────────

    fn disp_to_buffer_offset(&self, info: &LineDisplayInfo, line: usize, disp_idx: usize) -> usize {
        let mut dc = disp_idx.min(info.display_text.len());
        while dc > 0 && !info.display_text.is_char_boundary(dc) {
            dc -= 1;
        }
        let buffer_col = if dc < info.display_to_buffer.len() {
            info.display_to_buffer[dc]
        } else if let Some(&last) = info.display_to_buffer.last() {
            last + 1
        } else {
            0
        };
        self.buffer.line_start(line) + buffer_col
    }

    fn offset_for_visual_up(&self, current_offset: usize) -> Option<usize> {
        let (line, col) = self.buffer.offset_to_line_col(current_offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let disp_col = buffer_to_display_offset(&info.buffer_to_display, col);
        let pos = info
            .wrapped_line
            .position_for_index(disp_col, px(info.line_height))
            .unwrap_or_default();
        let cur_visual = (f32::from(pos.y) / info.line_height) as usize;
        let cur_x = pos.x;

        if cur_visual > 0 {
            let target_y = px((cur_visual as f32 - 0.5) * info.line_height);
            let new_disp = match info
                .wrapped_line
                .closest_index_for_position(point(cur_x, target_y), px(info.line_height))
            {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line > 0 {
            let prev_info = self.line_display_info.get(line - 1)?.as_ref()?;
            let last_visual = prev_info.visual_line_count.saturating_sub(1);
            let target_y = px((last_visual as f32 + 0.5) * prev_info.line_height);
            let new_disp = match prev_info
                .wrapped_line
                .closest_index_for_position(point(cur_x, target_y), px(prev_info.line_height))
            {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(prev_info, line - 1, new_disp))
        } else {
            Some(0)
        }
    }

    fn offset_for_visual_down(&self, current_offset: usize) -> Option<usize> {
        let (line, col) = self.buffer.offset_to_line_col(current_offset);
        let info = self.line_display_info.get(line)?.as_ref()?;
        let disp_col = buffer_to_display_offset(&info.buffer_to_display, col);
        let pos = info
            .wrapped_line
            .position_for_index(disp_col, px(info.line_height))
            .unwrap_or_default();
        let cur_visual = (f32::from(pos.y) / info.line_height) as usize;
        let cur_x = pos.x;

        if cur_visual + 1 < info.visual_line_count {
            let target_y = px((cur_visual as f32 + 1.5) * info.line_height);
            let new_disp = match info
                .wrapped_line
                .closest_index_for_position(point(cur_x, target_y), px(info.line_height))
            {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line + 1 < self.buffer.line_count() {
            let next_info = self.line_display_info.get(line + 1)?.as_ref()?;
            let target_y = px(0.5 * next_info.line_height);
            let new_disp = match next_info
                .wrapped_line
                .closest_index_for_position(point(cur_x, target_y), px(next_info.line_height))
            {
                Ok(idx) | Err(idx) => idx,
            };
            Some(self.disp_to_buffer_offset(next_info, line + 1, new_disp))
        } else {
            Some(self.buffer.len())
        }
    }

    // ── Action handlers ─────────────────────────────────────────────

    fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.pause_blinking(cx);
        }
    }

    fn on_escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_edit.is_some() {
            self.cancel_sidebar_edit(cx);
            return;
        }
        if self.context_menu.is_some() {
            self.close_context_menu(cx);
        }
    }

    fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.pause_blinking(cx);
        }
    }

    fn on_backspace(&mut self, _: &Backspace, _window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_edit.is_some() {
            self.sidebar_edit_backspace(cx);
            return;
        }
        self.checkpoint();
        // Special: cursor immediately after a list/quote prefix → delete entire prefix.
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
    }

    fn on_delete(&mut self, _: &Delete, _window: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.cursor.delete(&mut self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let prev_offset = self.cursor.offset;
        let (line, _col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let cell_ranges = self
            .decorations
            .get(line)
            .and_then(|d| d.table_info.as_ref())
            .filter(|ti| !ti.is_separator)
            .map(|ti| ti.cell_ranges.clone());
        self.cursor.move_left(&self.buffer);
        // Table navigation: skip inter-cell gaps (trailing-space + '|' + leading-space).
        if let Some(cell_ranges) = cell_ranges {
            let (_, new_col) = self.buffer.offset_to_line_col(self.cursor.offset);
            // Check internal gaps: cell[i].end+1 .. cell[i+1].start
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
            // Leading zone: before first cell start → jump to first cell start
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

    fn on_right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let prev_offset = self.cursor.offset;
        let (line, _col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let cell_ranges = self
            .decorations
            .get(line)
            .and_then(|d| d.table_info.as_ref())
            .filter(|ti| !ti.is_separator)
            .map(|ti| ti.cell_ranges.clone());
        self.cursor.move_right(&self.buffer);
        // Table navigation: skip inter-cell gaps.
        if let Some(cell_ranges) = cell_ranges {
            let (_, new_col) = self.buffer.offset_to_line_col(self.cursor.offset);
            // Check internal gaps: cell[i].end+1 .. cell[i+1].start
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

    fn on_up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
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

    fn on_down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
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

    fn on_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.select_left(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.select_right(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_for_visual_up(self.cursor.offset) {
            self.cursor.select_to(offset, &self.buffer);
        } else {
            self.cursor.select_up(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_for_visual_down(self.cursor.offset) {
            self.cursor.select_to(offset, &self.buffer);
        } else {
            self.cursor.select_down(&self.buffer);
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.select_all(&self.buffer);
        self.pause_blinking(cx);
    }

    fn on_home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.move_to_line_start(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.move_to_line_end(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.select_to_line_start(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.select_to_line_end(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
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
                        // Empty list item — remove prefix, exit list
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

    fn renumber_ordered_list_after(&mut self, start_line: usize, next_number: u32, indent: usize) {
        let mut items: Vec<(usize, usize, u32)> = Vec::new(); // (num_start, num_end, new_number)
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
        // Renumber bottom-to-top to preserve offsets
        for (num_start, num_end, new_num) in items.into_iter().rev() {
            self.buffer
                .replace(num_start..num_end, &new_num.to_string());
        }
    }

    fn on_tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
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

    fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection_to_clipboard(cx);
    }

    fn on_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        self.cut_selection_to_clipboard(cx);
    }

    fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_from_clipboard(cx);
    }

    fn on_show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn on_move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.move_word_left(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_move_word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor.move_word_right(&self.buffer);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn on_delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.cursor.delete_word_left(&mut self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    // ── Format actions ──────────────────────────────────────────────

    fn on_toggle_bold(&mut self, _: &ToggleBold, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_wrap("**", "**");
        self.pause_blinking(cx);
    }

    fn on_toggle_italic(&mut self, _: &ToggleItalic, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_wrap("*", "*");
        self.pause_blinking(cx);
    }

    fn on_toggle_strikethrough(
        &mut self,
        _: &ToggleStrikethrough,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.apply_wrap("~~", "~~");
        self.pause_blinking(cx);
    }

    fn on_toggle_inline_code(
        &mut self,
        _: &ToggleInlineCode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.apply_wrap("`", "`");
        self.pause_blinking(cx);
    }

    fn on_toggle_link(&mut self, _: &ToggleLink, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.insert_link_or_image("[", "](https://)", "title");
        self.pause_blinking(cx);
    }

    fn on_toggle_image(&mut self, _: &ToggleImage, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.insert_link_or_image("![", "](image.png)", "alt");
        self.pause_blinking(cx);
    }

    fn on_toggle_quote(&mut self, _: &ToggleQuote, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.toggle_current_line_prefix("> ");
        self.pause_blinking(cx);
    }

    fn on_toggle_todo(&mut self, _: &ToggleTodo, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.toggle_current_line_prefix("- [ ] ");
        self.pause_blinking(cx);
    }

    fn on_toggle_unordered_list(
        &mut self,
        _: &ToggleUnorderedList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.toggle_current_line_prefix("- ");
        self.pause_blinking(cx);
    }

    fn on_toggle_ordered_list(
        &mut self,
        _: &ToggleOrderedList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.toggle_current_line_prefix("1. ");
        self.pause_blinking(cx);
    }

    fn on_insert_code_block(
        &mut self,
        _: &InsertCodeBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.apply_wrap("```\n", "\n```");
        self.pause_blinking(cx);
    }

    fn on_insert_horizontal_rule(
        &mut self,
        _: &InsertHorizontalRule,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        self.toggle_horizontal_rule_at_cursor();
        self.pause_blinking(cx);
    }

    fn on_insert_heading1(&mut self, _: &InsertHeading1, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_heading(1);
        self.pause_blinking(cx);
    }

    fn on_insert_heading2(&mut self, _: &InsertHeading2, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_heading(2);
        self.pause_blinking(cx);
    }

    fn on_insert_heading3(&mut self, _: &InsertHeading3, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_heading(3);
        self.pause_blinking(cx);
    }

    fn on_insert_heading4(&mut self, _: &InsertHeading4, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_heading(4);
        self.pause_blinking(cx);
    }

    fn on_insert_heading5(&mut self, _: &InsertHeading5, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_heading(5);
        self.pause_blinking(cx);
    }

    fn on_insert_heading6(&mut self, _: &InsertHeading6, _: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint();
        self.apply_heading(6);
        self.pause_blinking(cx);
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(self.ui_scale + UI_SCALE_STEP, cx);
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(self.ui_scale - UI_SCALE_STEP, cx);
    }

    fn on_zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(1.0, cx);
    }

    fn apply_wrap(&mut self, prefix: &str, suffix: &str) {
        let range = self.cursor.selection_range();
        let (new_text, new_start, new_end) =
            toggle_wrap(self.buffer.text(), range.start, range.end, prefix, suffix);
        self.buffer = Buffer::from_text(&new_text);
        self.cursor.anchor = new_start;
        self.cursor.offset = new_end;
        self.reparse();
        self.scroll_to_cursor();
    }

    fn apply_heading(&mut self, level: u8) {
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

    // ── Mouse handlers ──────────────────────────────────────────────

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Check gutter fold icon clicks.
        if let Some(&(_, line_idx, is_image_fold)) = self
            .gutter_fold_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
        {
            if is_image_fold {
                self.toggle_image_fold(line_idx, cx);
            } else {
                self.toggle_fold(line_idx, cx);
            }
            return;
        }

        // Check image URL-edit icon clicks.
        if let Some(&(_, line_idx)) = self
            .image_edit_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            self.pick_image_for_line(line_idx, cx);
            return;
        }

        // Check image preview clicks — select full image syntax `![alt](url)`
        if let Some(line_idx) = self
            .image_click_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
        {
            if let Some(dec) = self.decorations.get(line_idx) {
                if let Some(span) = dec
                    .inline_spans
                    .iter()
                    .find(|s| s.kind == crate::markdown::InlineKind::Image)
                {
                    let line_start = self.buffer.line_start(line_idx);
                    let full_start = span.range.start.saturating_sub(2);
                    let full_end = dec
                        .syntax_ranges
                        .iter()
                        .filter(|sr| sr.range.start >= span.range.end)
                        .map(|sr| sr.range.end)
                        .max()
                        .unwrap_or(span.range.end);
                    self.cursor.anchor = line_start + full_start;
                    self.cursor.offset = line_start + full_end;
                    cx.notify();
                    return;
                }
            }
        }

        // Check checkbox clicks (don't move cursor for checkbox toggle)
        if let Some(line) = self
            .checkbox_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line)
        {
            self.checkpoint();
            self.toggle_checkbox(line);
            self.reparse();
            self.hovering_checkbox = true;
            cx.notify();
            return;
        }

        self.is_selecting = true;
        let prev_offset = self.cursor.offset;
        let offset = self.offset_for_position(event.position);
        if event.modifiers.shift {
            self.cursor.select_to(offset, &self.buffer);
        } else {
            self.cursor.move_to(offset, &self.buffer);
            self.maybe_auto_select_link_url(prev_offset, true);
        }
        cx.notify();
    }

    /// Auto-select the description or URL text when cursor enters an image/link range from outside.
    /// `also_select_desc` controls whether the description text zone also triggers selection.
    fn maybe_auto_select_link_url(&mut self, prev_offset: usize, also_select_desc: bool) {
        let new_offset = self.cursor.offset;
        let (line, _) = self.buffer.offset_to_line_col(new_offset);
        let line_start = self.buffer.line_start(line);
        if let Some(dec) = self.decorations.get(line) {
            let spans: Vec<_> = dec.inline_spans.clone();
            let syntax_ranges: Vec<_> = dec.syntax_ranges.clone();
            'link_check: for span in &spans {
                if !matches!(span.kind, InlineKind::Link | InlineKind::Image) {
                    continue;
                }
                let prefix_len = if span.kind == InlineKind::Image { 2 } else { 1 };
                let full_link_start = line_start + span.range.start.saturating_sub(prefix_len);
                let url_sr = syntax_ranges.iter().find(|sr| {
                    sr.range.start == span.range.end && sr.range.end > sr.range.start + 1
                });
                let full_link_end = url_sr
                    .map(|sr| line_start + sr.range.end)
                    .unwrap_or(line_start + span.range.end);

                // Skip if cursor was already inside the link/image range.
                if prev_offset >= full_link_start && prev_offset <= full_link_end {
                    continue;
                }

                // Cursor entered URL part?
                if let Some(url_sr) = url_sr {
                    let url_abs_start = line_start + url_sr.range.start + 2;
                    let url_abs_end = line_start + url_sr.range.end.saturating_sub(1);
                    if url_abs_start < url_abs_end
                        && new_offset >= url_abs_start
                        && new_offset <= url_abs_end
                    {
                        self.cursor.anchor = url_abs_start;
                        self.cursor.offset = url_abs_end;
                        self.is_selecting = false;
                        break 'link_check;
                    }
                }

                // Cursor entered description text?
                if also_select_desc {
                    let desc_abs_start = line_start + span.range.start;
                    let desc_abs_end = line_start + span.range.end;
                    if new_offset >= desc_abs_start && new_offset <= desc_abs_end {
                        self.cursor.anchor = desc_abs_start;
                        self.cursor.offset = desc_abs_end;
                        self.is_selecting = false;
                        break 'link_check;
                    }
                }
            }
        }
    }

    fn on_editor_context_menu(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = false;
        let offset = self.offset_for_position(event.position);
        let selection = self.cursor.selection_range();
        let clicked_selected_text =
            self.cursor.has_selection() && offset >= selection.start && offset <= selection.end;
        if !clicked_selected_text {
            self.cursor.move_to(offset, &self.buffer);
        }
        self.open_format_context_menu(event.position, cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _cx: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let hovering_checkbox = self
            .checkbox_areas
            .iter()
            .any(|(bounds, _)| bounds.contains(&event.position));
        if hovering_checkbox != self.hovering_checkbox {
            self.hovering_checkbox = hovering_checkbox;
            cx.notify();
        }

        let hovering_image_edit = self
            .image_edit_areas
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
            .map(|(_, line)| *line);
        if hovering_image_edit != self.hovering_image_edit_line {
            self.hovering_image_edit_line = hovering_image_edit;
            cx.notify();
        }

        let hovering_fold = self
            .gutter_fold_areas
            .iter()
            .find(|(bounds, _, _)| bounds.contains(&event.position))
            .map(|(_, line, _)| *line);
        if hovering_fold != self.hovering_fold_line {
            self.hovering_fold_line = hovering_fold;
            cx.notify();
        }

        if self.is_selecting {
            let pos_y: f32 = event.position.y.into();
            let origin_y: f32 = self.content_origin.y.into();
            let rel_pos_y = pos_y - origin_y;

            // Auto-scroll when dragging near edges
            let scroll_margin = 30.0;
            if rel_pos_y < scroll_margin {
                let speed = ((scroll_margin - rel_pos_y) / scroll_margin * 15.0).max(2.0);
                self.scroll_offset -= speed;
                self.clamp_scroll();
            } else if rel_pos_y > self.last_viewport_height - scroll_margin {
                let speed = ((rel_pos_y - (self.last_viewport_height - scroll_margin))
                    / scroll_margin
                    * 15.0)
                    .max(2.0);
                self.scroll_offset += speed;
                self.clamp_scroll();
            }

            let offset = self.offset_for_position(event.position);
            self.cursor.select_to(offset, &self.buffer);
            cx.notify();
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta_y: f32 = event
            .delta
            .pixel_delta(px(self.base_font_size() * LINE_HEIGHT_FACTOR))
            .y
            .into();
        self.scroll_offset -= delta_y;
        self.clamp_scroll();
        cx.notify();
    }

    fn offset_for_position(&self, position: Point<Pixels>) -> usize {
        let pos_y: f32 = position.y.into();
        let pos_x: f32 = position.x.into();
        let origin_y: f32 = self.content_origin.y.into();
        let y = (pos_y - origin_y) + self.scroll_offset;
        let line = self.line_at_y(y);
        let line_content = self.buffer.line_content(line);
        if line_content.is_empty() {
            return self.buffer.line_start(line);
        }

        // Use display info + shaped line for accurate hit testing
        if let Some(Some(info)) = self.line_display_info.get(line) {
            let line_top_y = self.line_y_offset(line);
            let local_y = (y - line_top_y).max(0.0);
            // Adjust x for list indent on continuation lines
            let visual_line_idx = (local_y / info.line_height) as usize;
            let indent_adjust = if info.indent_px > 0.0 && visual_line_idx > 0 {
                info.indent_px
            } else {
                0.0
            };
            let local_x = (pos_x - info.content_x - indent_adjust).max(0.0);
            let line_end_position = info
                .wrapped_line
                .position_for_index(info.display_text.len(), px(info.line_height))
                .unwrap_or_default();
            if visual_line_idx + 1 >= info.visual_line_count
                && local_x >= f32::from(line_end_position.x)
            {
                return self.buffer.line_start(line) + line_content.len();
            }
            let local_pos = point(px(local_x), px(local_y));
            let display_index = match info
                .wrapped_line
                .closest_index_for_position(local_pos, px(info.line_height))
            {
                Ok(idx) | Err(idx) => idx,
            };
            // Snap to char boundary in display text
            let mut dc = display_index.min(info.display_text.len());
            while dc > 0 && !info.display_text.is_char_boundary(dc) {
                dc -= 1;
            }
            // Map display offset back to buffer offset
            let buffer_col = if dc < info.display_to_buffer.len() {
                info.display_to_buffer[dc]
            } else if let Some(&last) = info.display_to_buffer.last() {
                last + 1
            } else {
                0
            };
            return self.buffer.line_start(line) + buffer_col;
        }

        let x = (pos_x - self.last_content_left).max(0.0);
        let approx_char_width = self.base_font_size() * 0.6;
        let col = (x / approx_char_width) as usize;
        self.buffer.line_col_to_offset(line, col)
    }

    // ── UTF-16 helpers ──────────────────────────────────────────────

    fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.buffer.text().chars() {
            if utf16_count >= utf16_offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, utf8_offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.buffer.text().chars() {
            if utf8_count >= utf8_offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
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
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.cursor.selection_range()),
            reversed: self.cursor.offset < self.cursor.anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Redirect to sidebar input when in edit mode
        if self.sidebar_edit.is_some() {
            self.sidebar_edit_append(new_text, cx);
            return;
        }
        self.checkpoint();
        // Use marked range if present (finalizing IME), otherwise use provided range or selection
        let range = if let Some(marked) = self.marked_range.take() {
            marked
        } else {
            range_utf16
                .as_ref()
                .map(|r| self.range_from_utf16(r))
                .unwrap_or_else(|| self.cursor.selection_range())
        };

        self.buffer.replace(range.clone(), new_text);
        let new_offset = range.start + new_text.len();
        self.cursor.move_to(new_offset, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint();
        // Use currently marked range if present, otherwise use provided range or selection
        let range = if let Some(marked) = self.marked_range.take() {
            marked
        } else {
            range_utf16
                .as_ref()
                .map(|r| self.range_from_utf16(r))
                .unwrap_or_else(|| self.cursor.selection_range())
        };

        self.buffer.replace(range.clone(), new_text);

        // Track the new marked (preedit) range
        let mark_start = range.start;
        let mark_end = range.start + new_text.len();
        if !new_text.is_empty() {
            self.marked_range = Some(mark_start..mark_end);
        }

        let new_offset = new_selected_range_utf16
            .as_ref()
            .map(|r| {
                // new_selected_range_utf16 is an offset within the newly inserted text.
                // Convert from UTF-16 units within new_text to UTF-8 byte offset.
                let mut utf8_pos = 0usize;
                let mut utf16_count = 0usize;
                for ch in new_text.chars() {
                    if utf16_count >= r.end {
                        break;
                    }
                    utf16_count += ch.len_utf16();
                    utf8_pos += ch.len_utf8();
                }
                mark_start + utf8_pos.min(new_text.len())
            })
            .unwrap_or(mark_end);
        self.cursor.move_to(new_offset, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let (line, col) = self.buffer.offset_to_line_col(range.start);
        let origin_y: f32 = self.content_origin.y.into();
        let line_top_y = origin_y + self.line_y_offset(line) - self.scroll_offset;

        if let Some(Some(info)) = self.line_display_info.get(line) {
            let disp_col = buffer_to_display_offset(&info.buffer_to_display, col);
            let pos = info
                .wrapped_line
                .position_for_index(disp_col, px(info.line_height))
                .unwrap_or_default();
            let indent_adjust = indent_for_y(info.indent_px, pos.y, info.line_height);
            let x = info.content_x + indent_adjust + f32::from(pos.x);
            // Return the rect of the character at the cursor; macOS positions IME below it
            let y = line_top_y + f32::from(pos.y);
            Some(Bounds::new(
                point(px(x), px(y)),
                size(px(info.font_size * 0.6), px(info.line_height)),
            ))
        } else {
            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            let lh = self.line_height_for_block(block_type);
            let x = self.last_content_left + col as f32 * self.base_font_size() * 0.6;
            Some(Bounds::new(
                point(px(x), px(line_top_y)),
                size(px(self.base_font_size() * 0.6), px(lh)),
            ))
        }
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _has_context_menu = self.context_menu.is_some();
        let resizing = self.sidebar_resizing;
        div()
            .id("editor-root")
            .key_context("MarkdownEditor")
            .track_focus(&self.focus_handle(cx))
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .bg(self.theme.background)
            .when(resizing, |this| {
                this.cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        if this.sidebar_resizing {
                            let new_width = f32::from(event.position.x);
                            let min_w = this.scaled(140.0);
                            let max_w = this.scaled(600.0);
                            this.sidebar_custom_width = Some(new_width.clamp(min_w, max_w));
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.sidebar_resizing = false;
                            cx.notify();
                        }),
                    )
            })
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_select_up))
            .on_action(cx.listener(Self::on_select_down))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_home))
            .on_action(cx.listener(Self::on_end))
            .on_action(cx.listener(Self::on_select_home))
            .on_action(cx.listener(Self::on_select_end))
            .on_action(cx.listener(Self::on_enter))
            .on_action(cx.listener(Self::on_tab))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_show_character_palette))
            .on_action(cx.listener(Self::on_move_word_left))
            .on_action(cx.listener(Self::on_move_word_right))
            .on_action(cx.listener(Self::on_delete_word_left))
            .on_action(cx.listener(Self::on_toggle_bold))
            .on_action(cx.listener(Self::on_toggle_italic))
            .on_action(cx.listener(Self::on_toggle_strikethrough))
            .on_action(cx.listener(Self::on_toggle_inline_code))
            .on_action(cx.listener(Self::on_toggle_link))
            .on_action(cx.listener(Self::on_toggle_image))
            .on_action(cx.listener(Self::on_toggle_quote))
            .on_action(cx.listener(Self::on_toggle_todo))
            .on_action(cx.listener(Self::on_toggle_unordered_list))
            .on_action(cx.listener(Self::on_toggle_ordered_list))
            .on_action(cx.listener(Self::on_insert_code_block))
            .on_action(cx.listener(Self::on_insert_horizontal_rule))
            .on_action(cx.listener(Self::on_insert_heading1))
            .on_action(cx.listener(Self::on_insert_heading2))
            .on_action(cx.listener(Self::on_insert_heading3))
            .on_action(cx.listener(Self::on_insert_heading4))
            .on_action(cx.listener(Self::on_insert_heading5))
            .on_action(cx.listener(Self::on_insert_heading6))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_escape))
            .child(self.render_sidebar(cx))
            // Drag handle / border between sidebar and editor
            .child({
                let border_color = if self.sidebar_resizing {
                    self.theme.accent
                } else {
                    self.theme.panel_border
                };
                let border_w = if self.sidebar_resizing { 3.0 } else { 1.0 };
                div()
                    .id("sidebar-resize-handle")
                    .w(px(border_w))
                    .h_full()
                    .flex_none()
                    .bg(border_color)
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(|s| s.w(px(3.0)).bg(self.theme.accent))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        if this.sidebar_resizing {
                            let new_width = f32::from(event.position.x);
                            let min_w = this.scaled(140.0);
                            let max_w = this.scaled(600.0);
                            this.sidebar_custom_width = Some(new_width.clamp(min_w, max_w));
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.sidebar_resizing = true;
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.sidebar_resizing = false;
                            cx.notify();
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.sidebar_resizing = false;
                            cx.notify();
                        }),
                    )
            })
            .child(
                div()
                    .id("workspace-pane")
                    .flex_1()
                    .size_full()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .bg(self.theme.background)
                    .child(self.render_tab_bar(cx))
                    .child(
                        div()
                            .id("editor-pane")
                            .relative()
                            .flex_1()
                            .overflow_hidden()
                            .cursor(if self.hovering_checkbox {
                                CursorStyle::Arrow
                            } else {
                                CursorStyle::IBeam
                            })
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(Self::on_editor_context_menu),
                            )
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_move(cx.listener(Self::on_mouse_move))
                            .on_scroll_wheel(cx.listener(Self::on_scroll))
                            .child(EditorElement {
                                editor: cx.entity().clone(),
                            }),
                    ),
            )
            // Full-screen backdrop swallows all events; context menu is a child of the backdrop.
            .when(self.context_menu.is_some(), |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .w_full()
                        .h_full()
                        .cursor(CursorStyle::Arrow)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.close_context_menu(cx);
                            }),
                        )
                        .on_mouse_up(MouseButton::Left, cx.listener(|_, _, _, _| {}))
                        .on_mouse_up(MouseButton::Right, cx.listener(|_, _, _, _| {}))
                        .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, _| {}))
                        .on_mouse_move(cx.listener(|_, _, _, _| {}))
                        .on_scroll_wheel(cx.listener(|_, _, _, _| {}))
                        .child(self.render_context_menu(cx)),
                )
            })
    }
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

fn default_document_text() -> &'static str {
    "# Welcome to Markdown Editor\n\nThis is a **bold** and *italic* demo.\n\nYou can write `inline code` and:\n\n```rust\nfn greet(name: &str) {\n  println!(\"hello, {name}\");\n}\n```\n\n## Features\n\n- Syntax markers hide when cursor leaves the range\n- .md files open into real tabs\n- FileTree now lives in a fixed left pane\n- `inline code` with background\n- Headings with different sizes\n- [Visit Google](https://google.com) for search\n- Check out ![Logo](crates/zed/resources/app-icon.png) inline image\n\n> Blockquotes look like this\n\n### Tasks\n\n- [ ] Unchecked task item\n- [x] Completed task item\n- [ ] Another todo\n\n| Name | Type | Description |\n|------|------|-------------|\n| id | int | **Primary** key |\n| name | string | [User profile](https://example.com) |\n| email | string | `Email` addr |\n\n1. First ordered item\n2. Second ordered item\n\n---\n\nStart typing to edit!\n"
}

// ── Custom Element for multi-line text rendering ────────────────────

pub struct EditorElement {
    editor: Entity<EditorView>,
}

pub struct EditorPrepaintState {
    lines: Vec<PrepaintLine>,
    cursor_quad: Option<PaintQuad>,
    selection_quads: Vec<PaintQuad>,
    line_highlight: Option<PaintQuad>,
    code_block_bgs: Vec<PaintQuad>,
    marked_underlines: Vec<PaintQuad>,
    gutter_quads: Vec<PaintQuad>,
    gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    /// (bounds, svg_path, color, rotation_degrees)
    gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla, f32)>,
    checkbox_hit_areas: Vec<(Bounds<Pixels>, usize)>,
    /// Image preview click areas: (bounds in screen space, buffer line index)
    image_hit_areas: Vec<(Bounds<Pixels>, usize)>,
    /// HR dashed lines: (y_position, start_x, width)
    hr_lines: Vec<(f32, f32, f32)>,
    /// Image previews to paint: (bounds, image_data)
    image_previews: Vec<(Bounds<Pixels>, Arc<RenderImage>)>,
    image_borders: Vec<PaintQuad>,
    /// Table cell borders and backgrounds
    table_borders: Vec<PaintQuad>,
    table_bgs: Vec<PaintQuad>,
    /// Image URL-edit icons: (bounds, line_idx, alpha)
    image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)>,
}

struct PrepaintLine {
    wrapped: WrappedLine,
    origin: Point<Pixels>,
    line_height: Pixels,
    /// Hanging indent for continuation lines.
    indent_px: f32,
    /// Per-visual-line shaped segments for indented painting.
    /// When Some, paint these instead of the wrapped line.
    segments: Option<Vec<(ShapedLine, Point<Pixels>)>>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some("editor-element".into())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let editor = self.editor.read(cx);
        let theme = &editor.theme;
        let buffer = &editor.buffer;
        let cursor = &editor.cursor;
        let decorations = &editor.decorations;
        let scroll_offset = editor.scroll_offset;
        let cursor_line = editor.cursor_line();

        let viewport_height: f32 = bounds.size.height.into();
        let viewport_width: f32 = bounds.size.width.into();
        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let editor_width = viewport_width.min(editor.max_editor_width());
        let editor_left = origin_x + ((viewport_width - editor_width) / 2.0).max(0.0);
        let text_width = (editor_width - editor.padding_left() - editor.padding_right())
            .max(editor.scaled(120.0));
        let wrap_width = text_width;

        let mut lines = Vec::new();
        let mut selection_quads = Vec::new();
        let mut line_highlight = None;
        let mut code_block_bgs = Vec::new();
        let mut cursor_quad = None;
        let mut marked_underlines = Vec::new();

        let mut y = origin_y + editor.padding_top();
        let line_count = buffer.line_count();

        let sel_range = cursor.selection_range();
        let has_selection = cursor.has_selection();

        let mut gutter_quads = Vec::new();
        let mut gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla, f32)> = Vec::new();
        let mut checkbox_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut image_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut display_info_cache: Vec<Option<LineDisplayInfo>> = Vec::with_capacity(line_count);
        // Cache reuse: if line_count and viewport width match, reuse off-screen line layouts
        let has_valid_cache = editor.line_display_info.len() == line_count
            && (editor.last_viewport_width - viewport_width).abs() < 0.5;
        let mut hr_lines: Vec<(f32, f32, f32)> = Vec::new();
        let mut image_previews: Vec<(Bounds<Pixels>, Arc<RenderImage>)> = Vec::new();
        let mut image_borders: Vec<PaintQuad> = Vec::new();
        let mut table_borders: Vec<PaintQuad> = Vec::new();
        let mut table_bgs: Vec<PaintQuad> = Vec::new();
        let mut image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)> = Vec::new();
        let mut image_edit_hit_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut gutter_fold_hit_areas_local: Vec<(Bounds<Pixels>, usize, bool)> = Vec::new();

        // Compute lines hidden by folded sections.
        let hidden_lines: HashSet<usize> = {
            let mut set = HashSet::new();
            for &fold_line in &editor.folded_sections {
                if let Some(range) = editor.fold_range_for_line(fold_line) {
                    for l in range {
                        set.insert(l);
                    }
                }
            }
            set
        };

        let text_style = window.text_style();
        let mut base_font = text_style.font();
        base_font.family = "Menlo".into();
        let mut heading_font = base_font.clone();
        heading_font.family = "Menlo".into();

        // Measure base monospace char width (used for list indent calculations)
        let base_char_width = {
            let test_run = TextRun {
                len: 1,
                font: base_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(test_lines) = window.text_system().shape_text(
                "x".into(),
                px(editor.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                f32::from(test_lines[0].width())
            } else {
                editor.base_font_size() * 0.6
            }
        };

        let image_cache = &editor.image_cache;
        let table_cell_padding_px = editor.scaled(TABLE_CELL_TEXT_PADDING_PX);
        let table_cell_padding_chars = if table_cell_padding_px <= 0.0 {
            0
        } else {
            (table_cell_padding_px / base_char_width).ceil().max(1.0) as usize
        };

        let is_code_block_type = |i: usize| {
            matches!(
                decorations.get(i).map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            )
        };
        let active_code_block_range: Option<std::ops::RangeInclusive<usize>> =
            if is_code_block_type(cursor_line) {
                let mut start = cursor_line;
                while start > 0 && is_code_block_type(start - 1) {
                    start -= 1;
                }
                let mut end = cursor_line;
                while end + 1 < line_count && is_code_block_type(end + 1) {
                    end += 1;
                }
                Some(start..=end)
            } else {
                None
            };

        for line_idx in 0..line_count {
            // Early fast-path: reuse cached display info for off-screen, non-cursor lines
            let is_cursor_line_early = line_idx == cursor_line;
            if has_valid_cache && !is_cursor_line_early {
                if let Some(Some(cached)) = editor.line_display_info.get(line_idx) {
                    let cached_h =
                        cached.line_height * cached.visual_line_count as f32 + cached.extra_height;
                    let est_top = y - scroll_offset;
                    let est_bottom = est_top + cached_h;
                    if est_bottom < origin_y || est_top > origin_y + viewport_height {
                        display_info_cache.push(Some(cached.clone()));
                        y += cached_h;
                        continue;
                    }
                }
            }

            let decoration = decorations.get(line_idx).cloned().unwrap_or_default();
            let block_type = &decoration.block_type;
            let font_size = editor.font_size_for_block(block_type);
            let line_height = font_size * LINE_HEIGHT_FACTOR;

            // Skip lines that are inside a folded section (zero-height placeholder).
            if hidden_lines.contains(&line_idx) {
                let line_content = buffer.line_content(line_idx);
                let blank_run = TextRun {
                    len: 1,
                    font: base_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|ls| ls.into_iter().next());
                display_info_cache.push(wrapped_line.map(|wl| LineDisplayInfo {
                    display_text: String::new(),
                    display_to_buffer: Vec::new(),
                    buffer_to_display: vec![0; line_content.len()],
                    font_size,
                    line_height: 0.0,
                    visual_line_count: 0,
                    wrapped_line: wl,
                    extra_height: 0.0,
                    indent_px: 0.0,
                    content_x: editor_left + editor.padding_left(),
                    editor_left,
                    editor_width,
                }));
                continue;
            }
            let is_cursor_line = line_idx == cursor_line;

            let line_content = buffer.line_content(line_idx);
            let line_start_offset = buffer.line_start(line_idx);
            let line_end_offset = buffer.line_end(line_idx);

            // Determine cursor column within this line (for per-range folding)
            // For table lines, expand ALL lines in the table when cursor is in any table line
            let cursor_in_table = if let Some(ref ti) = decoration.table_info {
                cursor_line >= ti.table_start_line && cursor_line < ti.table_end_line
            } else {
                false
            };
            let ime_active_on_line = editor.marked_range.as_ref().is_some_and(|marked| {
                marked.start < line_end_offset && marked.end >= line_start_offset
            });
            let cursor_col = if ime_active_on_line {
                Some(line_content.len())
            } else if is_cursor_line {
                Some(cursor.offset.saturating_sub(line_start_offset))
            } else if cursor_in_table {
                Some(line_content.len())
            } else {
                None
            };

            let hide_table_separator = matches!(block_type, BlockType::Table)
                && decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|table_info| table_info.is_separator);
            if hide_table_separator {
                let blank_run = TextRun {
                    len: 1,
                    font: base_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next());
                display_info_cache.push(wrapped_line.map(|wrapped_line| LineDisplayInfo {
                    display_text: String::new(),
                    display_to_buffer: Vec::new(),
                    buffer_to_display: vec![0; line_content.len()],
                    font_size,
                    line_height: 0.0,
                    visual_line_count: 0,
                    wrapped_line,
                    extra_height: 0.0,
                    indent_px: 0.0,
                    content_x: editor_left + editor.padding_left(),
                    editor_left,
                    editor_width,
                }));
                continue;
            }

            // Build display text: fold syntax ranges not containing cursor
            let table_width_chars = if *block_type == BlockType::Table {
                Some((text_width / base_char_width) as usize)
            } else {
                None
            };
            let (display_text, display_to_buffer, buffer_to_display, prefix_collapsed) =
                build_display_text(
                    line_content,
                    &decoration,
                    cursor_col,
                    table_width_chars,
                    table_cell_padding_chars,
                );

            // Build text runs for the display text
            let runs = build_text_runs_for_display(
                &display_text,
                line_content,
                &display_to_buffer,
                &decoration,
                cursor_col,
                prefix_collapsed,
                &base_font,
                &heading_font,
                font_size,
                theme,
            );

            let display_shared: SharedString = if display_text.is_empty() {
                " ".into()
            } else {
                display_text.clone().into()
            };

            // Compute list indent
            let is_list_type = matches!(
                block_type,
                BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
            );
            let indent_px = if is_list_type {
                let prefix_bytes =
                    compute_display_prefix_bytes(&display_text, block_type, prefix_collapsed);
                if prefix_bytes > 0 && prefix_bytes <= display_text.len() {
                    let prefix_chars = display_text
                        .char_indices()
                        .take_while(|(i, _)| *i < prefix_bytes)
                        .count();
                    prefix_chars as f32 * base_char_width
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let effective_wrap_width = if indent_px > 0.0 {
                (wrap_width - indent_px).max(wrap_width * 0.3)
            } else {
                wrap_width
            };

            // Use shape_text for word wrapping
            let Ok(wrapped_lines) = window.text_system().shape_text(
                display_shared,
                px(font_size),
                &runs,
                Some(px(effective_wrap_width)),
                None,
            ) else {
                display_info_cache.push(None);
                y += line_height;
                continue;
            };

            let wrapped = &wrapped_lines[0];
            let visual_line_count = wrapped.wrap_boundaries().len() + 1;
            let text_line_height = line_height * visual_line_count as f32;

            let image_active = decoration.image_path.is_some()
                && is_image_syntax_active(
                    &decoration,
                    line_content,
                    line_start_offset,
                    cursor.offset,
                    &sel_range,
                );

            // Compute extra height for image preview below this line
            let extra_height = if let Some(ref path) = decoration.image_path {
                if editor.folded_image_previews.contains(&line_idx) {
                    0.0 // image preview folded/hidden
                } else if let Some(Some(img_data)) = image_cache.get(path) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0 && img_h > 0.0 {
                        let (_, display_h) = compute_image_preview_size(img_w, img_h, text_width);
                        display_h + 8.0
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let total_line_height = text_line_height + extra_height;

            let line_top = y - scroll_offset;
            let line_bottom = line_top + total_line_height;
            let content_x =
                if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence) {
                    editor_left + editor.padding_left() + editor.scaled(CODE_BLOCK_TEXT_LEFT)
                } else {
                    editor_left + editor.padding_left()
                };

            // Store display info for all lines (needed for scroll calculations)
            display_info_cache.push(Some(LineDisplayInfo {
                display_text: display_text.clone(),
                display_to_buffer: display_to_buffer.clone(),
                buffer_to_display: buffer_to_display.clone(),
                font_size,
                line_height,
                visual_line_count,
                wrapped_line: wrapped.clone(),
                extra_height,
                indent_px,
                content_x,
                editor_left,
                editor_width,
            }));

            if line_bottom < origin_y || line_top > origin_y + viewport_height {
                y += total_line_height;
                continue;
            }

            // Build visual line segments for indented list painting
            let segments = if indent_px > 0.0 && wrapped.wrap_boundaries().len() > 0 {
                let byte_indices = wrap_boundary_byte_indices(wrapped);
                let mut shaped_segments: Vec<(ShapedLine, Point<Pixels>)> = Vec::new();
                let mut seg_start = 0;
                let seg_count = byte_indices.len() + 1;
                for seg_idx in 0..seg_count {
                    let seg_end = if seg_idx < byte_indices.len() {
                        byte_indices[seg_idx]
                    } else {
                        display_text.len()
                    };
                    let seg_text = &display_text[seg_start..seg_end];
                    let seg_runs = extract_runs_for_range(&runs, seg_start, seg_end);
                    let seg_shared: SharedString = if seg_text.is_empty() {
                        " ".into()
                    } else {
                        seg_text.to_string().into()
                    };
                    let seg_shaped =
                        window
                            .text_system()
                            .shape_line(seg_shared, px(font_size), &seg_runs, None);
                    let seg_x = if seg_idx == 0 {
                        content_x
                    } else {
                        content_x + indent_px
                    };
                    let seg_y = line_top + seg_idx as f32 * line_height;
                    shaped_segments.push((seg_shaped, point(px(seg_x), px(seg_y))));
                    seg_start = seg_end;
                }
                Some(shaped_segments)
            } else {
                None
            };

            let origin = point(px(content_x), px(line_top));

            // Code block background (clipped 15px top/bottom at fence lines when cursor not in block)
            if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence) {
                let cursor_in_this_block = active_code_block_range
                    .as_ref()
                    .is_some_and(|r| r.contains(&line_idx));
                let is_start_fence = matches!(block_type, BlockType::CodeBlockFence)
                    && (line_idx == 0 || !is_code_block_type(line_idx - 1));
                let is_end_fence = matches!(block_type, BlockType::CodeBlockFence)
                    && (line_idx + 1 >= line_count || !is_code_block_type(line_idx + 1));
                let clip_top = if !cursor_in_this_block && is_start_fence {
                    15.0
                } else {
                    0.0
                };
                let clip_bottom = if !cursor_in_this_block && is_end_fence {
                    15.0
                } else {
                    0.0
                };
                let bg_height = total_line_height - clip_top - clip_bottom;
                if bg_height > 0.0 {
                    code_block_bgs.push(fill(
                        Bounds::new(
                            point(
                                px(editor_left + editor.padding_left()),
                                px(line_top + clip_top),
                            ),
                            size(px(text_width), px(bg_height)),
                        ),
                        theme.code_block_bg,
                    ));
                }
            }

            // Table rendering: draw borders and background when folded (cursor NOT in table)
            if *block_type == BlockType::Table {
                if let Some(ref ti) = decoration.table_info {
                    let border_color = theme.blockquote_border;
                    let border_w = 1.0_f32;
                    let table_left = editor_left + editor.padding_left();
                    let table_right = editor_left + editor.padding_left() + text_width;
                    let table_width = table_right - table_left;

                    if ti.is_separator {
                        // Separator: horizontal border line
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top)),
                                size(px(table_width), px(border_w)),
                            ),
                            border_color,
                        ));
                    } else {
                        // Background for header rows
                        if ti.is_header {
                            table_bgs.push(fill(
                                Bounds::new(
                                    point(px(table_left), px(line_top)),
                                    size(px(table_width), px(text_line_height)),
                                ),
                                theme.code_block_bg,
                            ));
                        }

                        // Outer border: top edge
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top)),
                                size(px(table_width), px(border_w)),
                            ),
                            border_color,
                        ));
                        // Outer border: bottom edge
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top + text_line_height - border_w)),
                                size(px(table_width), px(border_w)),
                            ),
                            border_color,
                        ));
                        // Outer border: left edge
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top)),
                                size(px(border_w), px(text_line_height)),
                            ),
                            border_color,
                        ));
                        // Outer border: right edge
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_right - border_w), px(line_top)),
                                size(px(border_w), px(text_line_height)),
                            ),
                            border_color,
                        ));

                        // Inner vertical column borders (use same content-adaptive widths as text)
                        let render_col_widths = ti.column_widths.clone();
                        let mut col_x_offset = 0;
                        for (ci, cell) in ti.cells.iter().enumerate() {
                            let cell_chars = cell.trim().chars().count();
                            let col_w = render_col_widths
                                .get(ci)
                                .copied()
                                .unwrap_or(cell_chars)
                                .max(cell_chars);
                            col_x_offset += col_w + table_cell_padding_chars * 2;
                            if ci + 1 < ti.cells.len() {
                                let sep_x =
                                    table_left + border_w + col_x_offset as f32 * base_char_width;
                                table_borders.push(fill(
                                    Bounds::new(
                                        point(px(sep_x), px(line_top)),
                                        size(px(border_w), px(text_line_height)),
                                    ),
                                    border_color,
                                ));
                            }
                        }
                    }
                }
            }

            // HR dashed line when folded (cursor not on line)
            if *block_type == BlockType::HorizontalRule && !is_cursor_line {
                let hr_y = line_top + text_line_height / 2.0;
                hr_lines.push((hr_y, editor_left + editor.padding_left(), text_width));
            }

            // Image preview below the line (skipped if folded).
            if !editor.folded_image_previews.contains(&line_idx) {
                if let Some(ref path) = decoration.image_path {
                    if let Some(Some(img_data)) = image_cache.get(path) {
                        let img_size = img_data.size(0);
                        let img_w = img_size.width.0 as f32;
                        let img_h = img_size.height.0 as f32;
                        if img_w > 0.0 && img_h > 0.0 {
                            let (display_w, display_h) =
                                compute_image_preview_size(img_w, img_h, text_width);
                            let img_top = line_top + text_line_height + 4.0;
                            let img_left = editor_left
                                + editor.padding_left()
                                + ((text_width - display_w) / 2.0).max(0.0);
                            let img_bounds = Bounds::new(
                                point(px(img_left), px(img_top)),
                                size(px(display_w), px(display_h)),
                            );
                            image_previews.push((img_bounds, img_data.clone()));
                            image_hit_areas.push((img_bounds, line_idx));
                            if image_active {
                                image_borders.push(quad(
                                    img_bounds,
                                    Corners::all(px(6.0)),
                                    gpui::transparent_black(),
                                    px(2.0),
                                    theme.cursor,
                                    BorderStyle::default(),
                                ));
                            }
                        }
                    }
                }
            }

            // Gutter decorations (heading badges, blockquote bars)
            let gutter_center_y = line_top + line_height / 2.0;
            match block_type {
                BlockType::Heading(level) => {
                    let prefix_len = (*level as usize) + 1;
                    let heading_prefix_folded = cursor_col
                        .map(|col| prefix_is_folded(0, prefix_len, Some(col)))
                        .unwrap_or(false);
                    // Show hash icons when cursor on heading line AND fold icon not hovered.
                    let fold_hovered_this_line = editor.hovering_fold_line == Some(line_idx);
                    if is_cursor_line && heading_prefix_folded && !fold_hovered_this_line {
                        let icon_size = match *level {
                            1 => 28.0,
                            2 => 22.0,
                            3 => 18.0,
                            _ => 14.0,
                        };
                        let gap = 3.0;
                        let total_width = *level as f32 * icon_size + (*level as f32 - 1.0) * gap;
                        let start_x =
                            editor_left + editor.padding_left() - total_width - editor.scaled(10.0);
                        for icon_index in 0..*level {
                            let icon_x = start_x + icon_index as f32 * (icon_size + gap);
                            let icon_y = gutter_center_y - icon_size / 2.0;
                            gutter_svgs.push((
                                Bounds::new(
                                    point(px(icon_x), px(icon_y)),
                                    size(px(icon_size), px(icon_size)),
                                ),
                                HASH_SVG_PATH,
                                theme.gutter_heading,
                                0.0_f32,
                            ));
                        }
                    }
                }
                BlockType::Blockquote => {
                    let icon_size = 16.0_f32;
                    let icon_x =
                        editor_left + editor.padding_left() - icon_size - editor.scaled(10.0);
                    let icon_y = gutter_center_y - icon_size / 2.0;
                    gutter_svgs.push((
                        Bounds::new(
                            point(px(icon_x), px(icon_y)),
                            size(px(icon_size), px(icon_size)),
                        ),
                        COMMENT_SVG_PATH,
                        theme.blockquote_border,
                        0.0_f32,
                    ));
                }
                _ => {}
            }

            // Checkbox painting for TaskList when prefix is collapsed — gutter-aligned
            if prefix_collapsed {
                if let BlockType::TaskList(checked) = block_type {
                    let cb_size = font_size * TODO_CHECKBOX_SIZE_FACTOR;
                    // Level-1 todo (no indent): checkbox flush with editor content left edge.
                    // Indented (child) todos: checkbox aligned to parent's content indent.
                    let line_content_str = buffer.line_content(line_idx);
                    let leading_ws_bytes = line_content_str.len()
                        - line_content_str.trim_start_matches([' ', '\t']).len();
                    let cb_x = if leading_ws_bytes == 0 {
                        // Level-1: align to content_x (editor content left edge)
                        content_x
                    } else {
                        // Child: align to the text-start of the PARENT's indent level.
                        // Parent indent = half of current indent (2-space indent per level).
                        let parent_indent_bytes =
                            (leading_ws_bytes / 2).min(leading_ws_bytes.saturating_sub(2));
                        content_x + parent_indent_bytes as f32 * base_char_width
                    };
                    let cb_y = line_top + (line_height - cb_size) / 2.0;
                    let cb_bounds =
                        Bounds::new(point(px(cb_x), px(cb_y)), size(px(cb_size), px(cb_size)));
                    let corner_r = px(editor.scaled(TODO_CHECKBOX_CORNER_RADIUS));
                    if *checked {
                        gutter_quads.push(quad(
                            cb_bounds,
                            Corners::all(corner_r),
                            theme.checkbox_checked,
                            px(0.0),
                            gpui::transparent_black(),
                            BorderStyle::default(),
                        ));
                        let icon_size = cb_size * TODO_CHECK_ICON_SCALE;
                        let icon_offset = (cb_size - icon_size) / 2.0;
                        gutter_svgs.push((
                            Bounds::new(
                                point(px(cb_x + icon_offset), px(cb_y + icon_offset)),
                                size(px(icon_size), px(icon_size)),
                            ),
                            CHECK_SVG_PATH,
                            gpui::white(),
                            0.0_f32,
                        ));
                    } else {
                        gutter_quads.push(quad(
                            cb_bounds,
                            Corners::all(corner_r),
                            gpui::transparent_black(),
                            px(1.5),
                            theme.checkbox_border,
                            BorderStyle::default(),
                        ));
                    }
                    let hit_padding = editor.scaled(2.0);
                    let hit_bounds = Bounds::new(
                        point(px(cb_x - hit_padding), px(line_top)),
                        size(px(cb_size + hit_padding * 2.0), px(total_line_height)),
                    );
                    checkbox_hit_areas.push((hit_bounds, line_idx));
                }
            }

            // Gutter fold icon for foldable lines.
            if !hidden_lines.contains(&line_idx) {
                let fi_size = editor.scaled(GUTTER_FOLD_ICON_SIZE);
                let fi_x = content_x - fi_size - editor.scaled(5.0);

                if let Some(_) = editor.fold_range_for_line(line_idx) {
                    let is_folded = editor.folded_sections.contains(&line_idx);
                    let is_hovered = editor.hovering_fold_line == Some(line_idx);
                    // For CodeBlockFence: align fold icon with the first code line (one line below fence)
                    let fi_y = if *block_type == BlockType::CodeBlockFence && !is_folded {
                        line_top + line_height + (line_height - fi_size) / 2.0
                    } else {
                        line_top + (line_height - fi_size) / 2.0
                    };
                    // Only visible when hovered (or already folded for feedback).
                    let fi_alpha = if is_folded {
                        0.75_f32
                    } else if is_hovered {
                        0.7_f32
                    } else {
                        0.0_f32 // invisible when not hovered
                    };
                    if fi_alpha > 0.0 {
                        // Rotation: expanded = 90° CW (pointing down), folded = 0° (pointing right)
                        let rotation_deg = if is_folded { 0.0_f32 } else { 90.0_f32 };
                        let mut fi_color = if is_folded {
                            theme.accent
                        } else {
                            theme.panel_muted
                        };
                        fi_color.a = fi_alpha;
                        gutter_svgs.push((
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size))),
                            FOLD_SVG_PATH,
                            fi_color,
                            rotation_deg,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        Bounds::new(
                            point(px(fi_x - 3.0), px(line_top)),
                            size(px(fi_size + 6.0), px(line_height)),
                        ),
                        line_idx,
                        false, // not image fold
                    ));
                }

                // Image preview fold icon (when image preview is visible/loaded).
                if decoration.image_path.is_some()
                    && image_cache
                        .get(decoration.image_path.as_ref().unwrap())
                        .is_some_and(|v| v.is_some())
                {
                    let is_img_folded = editor.folded_image_previews.contains(&line_idx);
                    let is_hovered = editor.hovering_fold_line == Some(line_idx);
                    let preview_center_y = if is_img_folded {
                        line_top + line_height / 2.0
                    } else {
                        line_top + text_line_height + extra_height / 2.0
                    };
                    let fi_y = preview_center_y - fi_size / 2.0;
                    let fi_alpha = if is_img_folded {
                        0.75_f32
                    } else if is_hovered {
                        0.7_f32
                    } else {
                        0.0_f32
                    };
                    if fi_alpha > 0.0 {
                        let rotation_deg = if is_img_folded { 0.0_f32 } else { 90.0_f32 };
                        let mut fi_color = if is_img_folded {
                            theme.accent
                        } else {
                            theme.panel_muted
                        };
                        fi_color.a = fi_alpha;
                        gutter_svgs.push((
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size))),
                            FOLD_SVG_PATH,
                            fi_color,
                            rotation_deg,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        Bounds::new(
                            point(px(fi_x - 3.0), px(fi_y - 2.0)),
                            size(px(fi_size + 6.0), px(fi_size + 4.0)),
                        ),
                        line_idx,
                        true, // is_image_fold
                    ));
                }
            }

            // Image URL-edit icon (visible when image syntax is expanded).
            if image_active {
                let ie_size = editor.scaled(13.0);
                // Position immediately to the right of the rendered syntax
                let ie_x = editor_left + editor.padding_left() + text_width + editor.scaled(4.0);
                let ie_y = line_top + (line_height - ie_size) / 2.0;
                let ie_bounds =
                    Bounds::new(point(px(ie_x), px(ie_y)), size(px(ie_size), px(ie_size)));
                let is_hovered = editor.hovering_image_edit_line == Some(line_idx);
                let alpha = if is_hovered { 0.55 } else { 0.7 };
                image_edit_icons.push((ie_bounds, line_idx, alpha));
                image_edit_hit_areas_local.push((ie_bounds, line_idx));
            }

            // Line highlight for cursor line — spans full viewport width
            if is_cursor_line && !has_selection {
                line_highlight = Some(fill(
                    Bounds::new(
                        point(px(origin_x), px(line_top)),
                        size(px(viewport_width), px(total_line_height)),
                    ),
                    theme.line_highlight,
                ));
            }

            // Selection: map buffer offsets to display offsets
            if has_selection {
                let sel_start_on_line = sel_range.start.max(line_start_offset);
                let sel_end_on_line = sel_range.end.min(line_end_offset);

                if sel_start_on_line < sel_end_on_line {
                    let buf_start_col = sel_start_on_line - line_start_offset;
                    let buf_end_col = sel_end_on_line - line_start_offset;
                    let disp_start = buffer_to_display_offset(&buffer_to_display, buf_start_col);
                    let disp_end = buffer_to_display_offset(&buffer_to_display, buf_end_col);
                    selection_quads.extend(build_selection_quads(
                        &display_text,
                        &runs,
                        wrapped,
                        disp_start,
                        disp_end,
                        content_x,
                        line_top,
                        line_height,
                        font_size,
                        indent_px,
                        theme.selection,
                        window,
                    ));
                }
                // Selection continuation indicator (newline selected)
                if sel_range.end > line_end_offset && sel_range.start <= line_end_offset {
                    let disp_len = display_text.len();
                    let pos_end = wrapped
                        .position_for_index(disp_len, px(line_height))
                        .unwrap_or_default();
                    let indent_end = indent_for_y(indent_px, pos_end.y, line_height);
                    selection_quads.push(fill(
                        Bounds::from_corners(
                            point(
                                px(content_x + indent_end) + pos_end.x,
                                px(line_top) + pos_end.y,
                            ),
                            point(
                                px(content_x + indent_end)
                                    + pos_end.x
                                    + px(editor.base_font_size() * 0.4),
                                px(line_top) + pos_end.y + px(line_height),
                            ),
                        ),
                        theme.selection,
                    ));
                }
            }

            // Cursor: map buffer offset to display offset
            // Show cursor when focused and (blink visible OR actively selecting/typing)
            if is_cursor_line
                && editor.focus_handle.is_focused(window)
                && (editor.cursor_opacity > 0.0 || editor.is_selecting)
            {
                let buf_cursor_col = cursor.offset.saturating_sub(line_start_offset);
                let buf_cursor_col = buf_cursor_col.min(line_content.len());
                // When cursor is at or past the end of the buffer line, use end of display text.
                let disp_cursor_col = if buf_cursor_col >= line_content.len() {
                    display_text.len()
                } else {
                    buffer_to_display_offset(&buffer_to_display, buf_cursor_col)
                };

                let cursor_pos = wrapped
                    .position_for_index(disp_cursor_col, px(line_height))
                    .unwrap_or_default();

                let cursor_indent = indent_for_y(indent_px, cursor_pos.y, line_height);
                let cursor_top = line_top + f32::from(cursor_pos.y);
                let mut cursor_color = theme.cursor;
                cursor_color.a *= if editor.is_selecting {
                    CURSOR_MAX_OPACITY
                } else {
                    editor.cursor_opacity
                };
                cursor_quad = Some(fill(
                    Bounds::new(
                        point(px(content_x + cursor_indent) + cursor_pos.x, px(cursor_top)),
                        size(px(2.0), px(line_height)),
                    ),
                    cursor_color,
                ));
            }

            // IME marked text underline: map buffer offsets to display offsets
            if let Some(ref marked) = editor.marked_range {
                let mark_start_on_line = marked.start.max(line_start_offset);
                let mark_end_on_line = marked.end.min(line_end_offset);
                if mark_start_on_line < mark_end_on_line {
                    let buf_start = mark_start_on_line - line_start_offset;
                    let buf_end = mark_end_on_line - line_start_offset;
                    let disp_start = buffer_to_display_offset(&buffer_to_display, buf_start);
                    let disp_end = buffer_to_display_offset(&buffer_to_display, buf_end);
                    let pos_start = wrapped
                        .position_for_index(disp_start, px(line_height))
                        .unwrap_or_default();
                    let pos_end = wrapped
                        .position_for_index(disp_end, px(line_height))
                        .unwrap_or_default();
                    let indent_start = indent_for_y(indent_px, pos_start.y, line_height);
                    let underline_y = line_top + f32::from(pos_start.y) + line_height - 2.0;
                    marked_underlines.push(fill(
                        Bounds::from_corners(
                            point(px(content_x + indent_start) + pos_start.x, px(underline_y)),
                            point(
                                px(content_x + indent_start) + pos_end.x,
                                px(underline_y + 1.0),
                            ),
                        ),
                        theme.foreground,
                    ));
                }
            }

            lines.push(PrepaintLine {
                wrapped: wrapped.clone(),
                origin,
                line_height: px(line_height),
                indent_px,
                segments,
            });

            y += total_line_height;
        }

        self.editor.update(cx, |editor, _| {
            editor.line_display_info = display_info_cache;
            editor.last_viewport_height = viewport_height;
            editor.last_viewport_width = viewport_width;
            editor.checkbox_areas = checkbox_hit_areas.clone();
            editor.image_click_areas = image_hit_areas.clone();
            editor.gutter_fold_areas = gutter_fold_hit_areas_local.clone();
            editor.image_edit_areas = image_edit_hit_areas_local.clone();
            editor.last_editor_left = editor_left;
            editor.last_editor_width = editor_width;
            editor.last_content_left = editor_left + editor.padding_left();
            editor.last_text_width = text_width;
            editor.content_origin = bounds.origin;
        });

        // Trigger loading for any image paths not yet in cache
        let decorations_ref = self.editor.read(cx).decorations.clone();
        let mut new_image_paths: Vec<String> = Vec::new();
        for dec in &decorations_ref {
            if let Some(ref path) = dec.image_path {
                let has = self.editor.read(cx).image_cache.contains_key(path);
                if !has {
                    new_image_paths.push(path.clone());
                }
            }
        }
        if !new_image_paths.is_empty() {
            for path in new_image_paths {
                let maybe_image = resolve_local_image_path(&path)
                    .and_then(|resolved| load_render_image_from_path(&resolved).ok())
                    .map(Arc::new);
                self.editor.update(cx, |editor, cx| {
                    editor.image_cache.insert(path, maybe_image);
                    cx.notify();
                });
            }
        }

        EditorPrepaintState {
            lines,
            cursor_quad,
            selection_quads,
            line_highlight,
            code_block_bgs,
            marked_underlines,
            gutter_quads,
            gutter_labels,
            gutter_svgs,
            checkbox_hit_areas,
            image_hit_areas,
            hr_lines,
            image_previews,
            image_borders,
            table_borders,
            table_bgs,
            image_edit_icons,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        for quad in prepaint.code_block_bgs.drain(..) {
            window.paint_quad(quad);
        }

        // Table backgrounds
        for quad in prepaint.table_bgs.drain(..) {
            window.paint_quad(quad);
        }

        if let Some(highlight) = prepaint.line_highlight.take() {
            window.paint_quad(highlight);
        }

        for quad in prepaint.selection_quads.drain(..) {
            window.paint_quad(quad);
        }

        // Gutter decorations
        for quad in prepaint.gutter_quads.drain(..) {
            window.paint_quad(quad);
        }
        for (icon_bounds, icon_path, color, rotation_deg) in &prepaint.gutter_svgs {
            let transform = if *rotation_deg != 0.0 {
                // Rotate around the center of the icon bounds.
                let cx_f: f32 = icon_bounds.origin.x.into();
                let cy_f: f32 = icon_bounds.origin.y.into();
                let w_f: f32 = icon_bounds.size.width.into();
                let h_f: f32 = icon_bounds.size.height.into();
                let angle = gpui::Radians(rotation_deg.to_radians());
                TransformationMatrix::unit()
                    .translate(gpui::point(
                        gpui::ScaledPixels::from(cx_f + w_f * 0.5),
                        gpui::ScaledPixels::from(cy_f + h_f * 0.5),
                    ))
                    .rotate(angle)
                    .translate(gpui::point(
                        gpui::ScaledPixels::from(-w_f * 0.5),
                        gpui::ScaledPixels::from(-h_f * 0.5),
                    ))
            } else {
                TransformationMatrix::default()
            };
            let _ = window.paint_svg(*icon_bounds, (*icon_path).into(), transform, *color, cx);
        }
        for (shaped, origin, line_height) in &prepaint.gutter_labels {
            shaped.paint(*origin, *line_height, window, cx).ok();
        }

        // Table borders
        for quad in prepaint.table_borders.drain(..) {
            window.paint_quad(quad);
        }

        // HR dashed lines
        for (hr_y, hr_start_x, hr_width) in &prepaint.hr_lines {
            let dash_len = 6.0_f32;
            let gap_len = 4.0_f32;
            let mut x = *hr_start_x;
            let end_x = *hr_start_x + hr_width;
            let theme = &self.editor.read(cx).theme;
            while x < end_x {
                let seg_end = (x + dash_len).min(end_x);
                window.paint_quad(fill(
                    Bounds::new(point(px(x), px(*hr_y)), size(px(seg_end - x), px(1.0))),
                    theme.blockquote_border,
                ));
                x = seg_end + gap_len;
            }
        }

        for line in &prepaint.lines {
            if let Some(ref segments) = line.segments {
                for (shaped, origin) in segments {
                    shaped.paint(*origin, line.line_height, window, cx).ok();
                }
            } else {
                line.wrapped
                    .paint(
                        line.origin,
                        line.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
            }
        }

        // Image previews
        for (img_bounds, img_data) in &prepaint.image_previews {
            window
                .paint_image(*img_bounds, Corners::default(), img_data.clone(), 0, false)
                .ok();
        }

        for quad in prepaint.image_borders.drain(..) {
            window.paint_quad(quad);
        }

        // Image URL-edit icons
        for (icon_bounds, _line, alpha) in &prepaint.image_edit_icons {
            let theme = &self.editor.read(cx).theme;
            let mut color = theme.panel_muted;
            color.a = *alpha;
            let _ = window.paint_svg(
                *icon_bounds,
                OPEN_SVG_PATH.into(),
                TransformationMatrix::default(),
                color,
                cx,
            );
        }

        if let Some(cursor) = prepaint.cursor_quad.take() {
            window.paint_quad(cursor);
        }

        for quad in prepaint.marked_underlines.drain(..) {
            window.paint_quad(quad);
        }
    }
}

/// Build display text by selectively hiding syntax ranges.
/// A syntax range is hidden unless cursor_col falls within its trigger_range.
/// Returns (display_text, display_to_buffer, buffer_to_display, prefix_collapsed).
fn build_display_text(
    line_content: &str,
    decoration: &LineDecoration,
    cursor_col: Option<usize>,
    table_width_chars: Option<usize>,
    table_cell_padding_chars: usize,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    let len = line_content.len();

    // Handle table lines: when folded, build formatted cell display
    if decoration.block_type == BlockType::Table {
        if let Some(ref table_info) = decoration.table_info {
            return build_table_display_text(
                line_content,
                table_info,
                cursor_col,
                table_width_chars,
                table_cell_padding_chars,
            );
        }
    }

    if decoration.block_type == BlockType::CodeBlockFence && cursor_col.is_none() {
        let display_text = line_content
            .chars()
            .map(|ch| if ch == '\t' { '\t' } else { ' ' })
            .collect::<String>();
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let buffer_to_display: Vec<usize> = (0..len).collect();
        return (display_text, display_to_buffer, buffer_to_display, false);
    }

    if decoration.syntax_ranges.is_empty() {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let buffer_to_display: Vec<usize> = (0..len).collect();
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    // Determine which syntax ranges are hidden (cursor NOT in their trigger)
    let hidden: Vec<bool> = decoration
        .syntax_ranges
        .iter()
        .map(|sr| {
            cursor_col.map_or(true, |col| {
                let at_eol = col >= len;
                if at_eol {
                    // At EOL: only keep syntax VISIBLE for block-level constructs where
                    // the entire line is syntax (HR, code-fence). For inline markup on
                    // normal/heading lines the per-range rule still applies so that
                    // moving to EOL doesn't expand every marker on the line.
                    !matches!(
                        decoration.block_type,
                        BlockType::HorizontalRule | BlockType::CodeBlockFence
                    )
                } else {
                    col < sr.trigger_range.start || col >= sr.trigger_range.end
                }
            })
        })
        .collect();

    // If nothing is hidden, return identity mapping
    if !hidden.iter().any(|&h| h) {
        let display_to_buffer: Vec<usize> = (0..len).collect();
        let buffer_to_display: Vec<usize> = (0..len).collect();
        return (
            line_content.to_string(),
            display_to_buffer,
            buffer_to_display,
            false,
        );
    }

    // Build per-byte hidden mask
    let mut is_hidden = vec![false; len];
    for (i, sr) in decoration.syntax_ranges.iter().enumerate() {
        if hidden[i] {
            let start = sr.range.start.min(len);
            let end = sr.range.end.min(len);
            for j in start..end {
                is_hidden[j] = true;
            }
        }
    }

    let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
    let trimmed = &line_content[leading_ws..];

    let prefix_range_and_replacement = match decoration.block_type {
        BlockType::UnorderedList => {
            let fold_len = if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                2
            } else {
                0
            };
            // Expand when cursor is within `- ` (positions 0..fold_len); collapse otherwise.
            (fold_len > 0 && prefix_is_folded(leading_ws, fold_len, cursor_col))
                .then_some((leading_ws..leading_ws + fold_len, "• ".to_string()))
        }
        BlockType::OrderedList => {
            let prefix_len = markdown::ordered_list_prefix_len(trimmed);
            (prefix_len > 0 && prefix_is_folded(leading_ws, prefix_len, cursor_col)).then_some((
                leading_ws..leading_ws + prefix_len,
                trimmed[..prefix_len].to_string(),
            ))
        }
        BlockType::TaskList(_) => {
            let fold_len = task_prefix_len(trimmed);
            let trigger_len = task_syntax_len(trimmed);
            (fold_len > 0 && prefix_is_folded(leading_ws, trigger_len, cursor_col))
                .then_some((leading_ws..leading_ws + fold_len, "   ".to_string()))
        }
        _ => None,
    };

    let prefix_collapsed = prefix_range_and_replacement.is_some();

    let mut display_text = String::with_capacity(len);
    let mut display_to_buffer = Vec::with_capacity(len);
    let mut buffer_to_display = vec![0usize; len];
    let mut display_pos = 0;

    if let Some((prefix_range, replacement)) = &prefix_range_and_replacement {
        for (buf_pos, ch) in line_content[..prefix_range.start].char_indices() {
            let ch_len = ch.len_utf8();
            display_text.push(ch);
            for offset in 0..ch_len {
                display_to_buffer.push(buf_pos + offset);
                buffer_to_display[buf_pos + offset] = display_pos + offset;
            }
            display_pos += ch_len;
        }

        let collapsed_display_start = display_pos;
        display_text.push_str(replacement);
        for _ in 0..replacement.len() {
            display_to_buffer.push(prefix_range.start);
        }
        for buf_i in prefix_range.start..prefix_range.end.min(len) {
            buffer_to_display[buf_i] = collapsed_display_start;
        }
        display_pos += replacement.len();
    }

    // Process remaining characters
    for (buf_pos, ch) in line_content.char_indices() {
        if let Some((prefix_range, _)) = &prefix_range_and_replacement {
            if buf_pos < prefix_range.end {
                continue;
            }
        }

        let ch_len = ch.len_utf8();
        if is_hidden[buf_pos] {
            for offset in 0..ch_len {
                if buf_pos + offset < len {
                    buffer_to_display[buf_pos + offset] = display_pos;
                }
            }
        } else {
            display_text.push(ch);
            for offset in 0..ch_len {
                display_to_buffer.push(buf_pos + offset);
                if buf_pos + offset < len {
                    buffer_to_display[buf_pos + offset] = display_pos + offset;
                }
            }
            display_pos += ch_len;
        }
    }

    (
        display_text,
        display_to_buffer,
        buffer_to_display,
        prefix_collapsed,
    )
}

/// Build display text for a folded table row.
fn build_table_display_text(
    line_content: &str,
    info: &TableLineInfo,
    _cursor_col: Option<usize>,
    _available_chars: Option<usize>,
    table_cell_padding_chars: usize,
) -> (String, Vec<usize>, Vec<usize>, bool) {
    let len = line_content.len();
    let cell_padding = table_cell_padding_chars;

    // Use each column's natural content width for adaptive sizing.
    // This avoids forced equal distribution that causes unnecessary padding or wrapping.
    let balanced_col_widths = info.column_widths.clone();

    if info.is_separator {
        // Separator row stays visually empty; borders are painted separately.
        let total_chars: usize = balanced_col_widths
            .iter()
            .map(|w| w + cell_padding * 2)
            .sum::<usize>();
        let display = " ".repeat(total_chars.max(1));
        let d2b = vec![0; display.len()];
        let b2d = vec![0; len];
        return (display, d2b, b2d, false);
    }

    let mut display = String::new();
    let mut d2b: Vec<usize> = Vec::new();
    let mut b2d = vec![0usize; len];
    let mut consumed = 0usize;
    let mut display_pos = 0usize;

    for (ci, cell) in info.cells.iter().enumerate() {
        let cell_range = info
            .cell_ranges
            .get(ci)
            .cloned()
            .unwrap_or(consumed..consumed);

        for buf_i in consumed..cell_range.start.min(len) {
            b2d[buf_i] = display_pos;
        }

        let cell_decoration = markdown::parse_decorations(cell)
            .into_iter()
            .next()
            .unwrap_or_default();
        let (folded_cell, cell_d2b, cell_b2d, _) =
            build_display_text(cell, &cell_decoration, None, None, 0);
        let cell_content_chars = folded_cell.chars().count();
        let width = balanced_col_widths
            .get(ci)
            .copied()
            .unwrap_or(cell_content_chars)
            .max(cell_content_chars);
        let left_padding = " ".repeat(cell_padding);
        let right_padding_len = width.saturating_sub(cell_content_chars) + cell_padding;
        let right_padding = " ".repeat(right_padding_len);
        let cell_anchor = cell_range.start.min(len.saturating_sub(1));

        display.push_str(&left_padding);
        for _ in 0..left_padding.len() {
            d2b.push(cell_anchor);
        }
        display_pos += left_padding.len();

        display.push_str(&folded_cell);
        for &local_buf in &cell_d2b {
            d2b.push((cell_range.start + local_buf).min(len.saturating_sub(1)));
        }
        for (local_buf, &local_display) in cell_b2d.iter().enumerate() {
            let global = cell_range.start + local_buf;
            if global < len {
                b2d[global] = display_pos + local_display;
            }
        }
        display_pos += folded_cell.len();

        display.push_str(&right_padding);
        // Map right-padding display positions to cell_range.end (the trailing space before '|'),
        // so clicking in the right-padding area places the cursor _after_ the last cell char.
        let right_anchor = cell_range.end.min(len.saturating_sub(1));
        for _ in 0..right_padding.len() {
            d2b.push(right_anchor);
        }
        display_pos += right_padding.len();

        consumed = cell_range.end.min(len);
    }

    for buf_i in consumed..len {
        b2d[buf_i] = display_pos;
    }

    (display, d2b, b2d, false)
}

fn build_selection_quads(
    display_text: &str,
    runs: &[TextRun],
    wrapped: &WrappedLine,
    disp_start: usize,
    disp_end: usize,
    content_x: f32,
    line_top: f32,
    line_height: f32,
    font_size: f32,
    indent_px: f32,
    selection_color: Hsla,
    window: &mut Window,
) -> Vec<PaintQuad> {
    if disp_start >= disp_end {
        return Vec::new();
    }

    let mut quads = Vec::new();
    let mut segment_start = 0usize;
    for (segment_index, segment_end) in wrap_boundary_byte_indices(wrapped)
        .into_iter()
        .chain(std::iter::once(display_text.len()))
        .enumerate()
    {
        let overlap_start = disp_start.max(segment_start);
        let overlap_end = disp_end.min(segment_end);
        if overlap_start < overlap_end {
            let start_x = if overlap_start == segment_start {
                0.0
            } else {
                f32::from(
                    wrapped
                        .position_for_index(overlap_start, px(line_height))
                        .unwrap_or_default()
                        .x,
                )
            };

            let end_x = if overlap_end == segment_end {
                let segment_text = &display_text[segment_start..segment_end];
                let segment_runs = extract_runs_for_range(runs, segment_start, segment_end);
                let shared: SharedString = if segment_text.is_empty() {
                    " ".into()
                } else {
                    segment_text.to_string().into()
                };
                f32::from(
                    window
                        .text_system()
                        .shape_line(shared, px(font_size), &segment_runs, None)
                        .width,
                )
            } else {
                f32::from(
                    wrapped
                        .position_for_index(overlap_end, px(line_height))
                        .unwrap_or_default()
                        .x,
                )
            };

            let indent = if segment_index > 0 { indent_px } else { 0.0 };
            let segment_y = line_top + segment_index as f32 * line_height;
            quads.push(fill(
                Bounds::from_corners(
                    point(px(content_x + indent + start_x), px(segment_y)),
                    point(px(content_x + indent + end_x), px(segment_y + line_height)),
                ),
                selection_color,
            ));
        }
        segment_start = segment_end;
    }

    quads
}

fn compute_image_preview_size(image_width: f32, image_height: f32, max_width: f32) -> (f32, f32) {
    let width_scale = max_width / image_width;
    let height_scale = 400.0 / image_height;
    let scale = width_scale.min(height_scale);
    (image_width * scale, image_height * scale)
}

fn image_syntax_range(line_content: &str) -> Option<Range<usize>> {
    let start = line_content.find("![")?;
    let suffix = &line_content[start..];
    let end = suffix.find(')')? + start + 1;
    Some(start..end)
}

fn is_image_syntax_active(
    decoration: &LineDecoration,
    line_content: &str,
    line_start_offset: usize,
    cursor_offset: usize,
    selection: &Range<usize>,
) -> bool {
    if decoration.image_path.is_none() {
        return false;
    }

    let Some(local_range) = image_syntax_range(line_content) else {
        return false;
    };
    let absolute_range =
        (line_start_offset + local_range.start)..(line_start_offset + local_range.end);

    (cursor_offset >= absolute_range.start && cursor_offset <= absolute_range.end)
        || (selection.start < absolute_range.end && selection.end > absolute_range.start)
}

/// Extract TextRuns covering the byte range [seg_start..seg_end) from the original runs.
fn extract_runs_for_range(runs: &[TextRun], seg_start: usize, seg_end: usize) -> Vec<TextRun> {
    let mut result = Vec::new();
    let mut pos = 0;
    for run in runs {
        let run_end = pos + run.len;
        let overlap_start = seg_start.max(pos);
        let overlap_end = seg_end.min(run_end);
        if overlap_start < overlap_end {
            result.push(TextRun {
                len: overlap_end - overlap_start,
                ..run.clone()
            });
        }
        pos = run_end;
    }
    result
}

/// Get wrap boundary byte indices from the WrappedLine's shaped runs.
fn wrap_boundary_byte_indices(wrapped: &WrappedLine) -> Vec<usize> {
    let runs = wrapped.runs();
    wrapped
        .wrap_boundaries()
        .iter()
        .map(|wb| runs[wb.run_ix].glyphs[wb.glyph_ix].index)
        .collect()
}

/// Return indent_px if the position y indicates a continuation line (y > 0).
fn indent_for_y(indent_px: f32, y: Pixels, line_height: f32) -> f32 {
    if indent_px > 0.0 && f32::from(y) > line_height * 0.5 {
        indent_px
    } else {
        0.0
    }
}

/// Compute the display prefix byte count for list indent.
fn compute_display_prefix_bytes(
    display_text: &str,
    block_type: &BlockType,
    prefix_collapsed: bool,
) -> usize {
    let leading_ws = display_text.len() - display_text.trim_start_matches([' ', '\t']).len();
    let trimmed = &display_text[leading_ws..];
    match block_type {
        BlockType::UnorderedList => {
            if prefix_collapsed {
                leading_ws + "• ".len()
            } else if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                leading_ws + 2
            } else {
                0
            }
        }
        BlockType::TaskList(_) => {
            if prefix_collapsed {
                leading_ws + "   ".len()
            } else {
                trimmed.find("] ").map(|p| leading_ws + p + 2).unwrap_or(0)
            }
        }
        BlockType::OrderedList => {
            if prefix_collapsed {
                trimmed
                    .find(". ")
                    .map(|p| leading_ws + p + 2)
                    .unwrap_or(leading_ws + trimmed.len().min(4))
            } else {
                trimmed.find(". ").map(|p| leading_ws + p + 2).unwrap_or(0)
            }
        }
        _ => 0,
    }
}

fn prefix_is_folded(leading_ws: usize, prefix_len: usize, cursor_col: Option<usize>) -> bool {
    cursor_col.is_none_or(|col| col < leading_ws || col >= leading_ws + prefix_len)
}

fn task_prefix_len(trimmed: &str) -> usize {
    for marker in ["- [ ] ", "- [x] ", "* [ ] ", "* [x] ", "+ [ ] ", "+ [x] "] {
        if trimmed.starts_with(marker) {
            return marker.len();
        }
    }
    0
}

fn task_syntax_len(trimmed: &str) -> usize {
    // Include the trailing space so the cursor *on* the space keeps the prefix expanded.
    task_prefix_len(trimmed)
}

fn trimmed_ordered_prefix_len(display_text: &str) -> usize {
    let leading_ws = display_text.len() - display_text.trim_start_matches([' ', '\t']).len();
    let trimmed = &display_text[leading_ws..];
    markdown::ordered_list_prefix_len(trimmed) + leading_ws
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn syntect_theme() -> &'static SyntectTheme {
    static THEME: OnceLock<SyntectTheme> = OnceLock::new();
    THEME.get_or_init(|| {
        let themes = ThemeSet::load_defaults();
        themes
            .themes
            .get("OneHalfDark")
            .cloned()
            .or_else(|| themes.themes.values().next().cloned())
            .expect("syntect default themes are missing")
    })
}

fn syntect_color_to_hsla(color: SyntectColor) -> Hsla {
    rgba_to_hsla(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    )
}

fn rgba_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f32::EPSILON {
        return Hsla {
            h: 0.0,
            s: 0.0,
            l,
            a,
        };
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < f32::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    Hsla {
        h: h / 6.0,
        s,
        l,
        a,
    }
}

fn apply_code_block_highlighting(
    line_content: &str,
    language: &str,
    colors: &mut [Hsla],
    weights: &mut [FontWeight],
    styles: &mut [FontStyle],
) {
    if line_content.is_empty() {
        return;
    }

    let syntax_set = syntax_set();
    let syntax = syntax_set
        .find_syntax_by_token(language)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, syntect_theme());
    if let Ok(ranges) = highlighter.highlight_line(line_content, syntax_set) {
        let mut byte_pos = 0usize;
        for (style, piece) in ranges {
            let end = (byte_pos + piece.len()).min(colors.len());
            let mut color = syntect_color_to_hsla(style.foreground);
            // Plain text (low saturation): shift to warm yellow; other tokens: brighten
            if color.a > 0.05 {
                color.a = color.a.max(0.9);
                if color.s < 0.15 {
                    // Near-neutral color → warm golden/yellow (#e5c07b-ish)
                    color.h = 0.11; // hue ≈ 40° (yellow-orange)
                    color.s = 0.65;
                    color.l = (color.l * 1.1 + 0.05).clamp(0.55, 0.82);
                } else {
                    color.l = (color.l * 1.15 + 0.05).min(1.0);
                }
            }
            for idx in byte_pos..end {
                colors[idx] = color;
                weights[idx] = if style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::BOLD)
                {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                };
                styles[idx] = if style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::ITALIC)
                {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };
            }
            byte_pos = end;
        }
    }
}

fn resolve_local_image_path(path: &str) -> Option<PathBuf> {
    let path_buf = PathBuf::from(path);
    if path_buf.is_absolute() {
        return path_buf.exists().then_some(path_buf);
    }

    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(&path_buf));
        candidates.push(cwd.join("editor").join(&path_buf));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest_dir.join(&path_buf));
    if let Some(parent) = manifest_dir.parent() {
        candidates.push(parent.join(&path_buf));
    }

    candidates.into_iter().find(|candidate| candidate.exists())
}

fn load_render_image_from_path(path: &PathBuf) -> anyhow::Result<RenderImage> {
    let bytes = std::fs::read(path)?;
    let dynamic = image::load_from_memory(&bytes)?;
    let frame = Frame::new(dynamic.to_rgba8());
    Ok(RenderImage::new([frame]))
}

/// Show a native macOS file picker (via osascript) and return the chosen path.
fn pick_image_file_blocking() -> Option<PathBuf> {
    let script =
        r#"POSIX path of (choose file of type {"public.image"} with prompt "Choose Image")"#;
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

fn pick_folder_blocking() -> Option<PathBuf> {
    let script = r#"POSIX path of (choose folder with prompt "Choose Workspace Folder")"#;
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

/// Map a buffer byte offset (within line) to display byte offset.
fn buffer_to_display_offset(buffer_to_display: &[usize], buf_offset: usize) -> usize {
    if buf_offset >= buffer_to_display.len() {
        // Past end of line: return end of display text
        if let Some(&last) = buffer_to_display.last() {
            last + 1
        } else {
            0
        }
    } else {
        buffer_to_display[buf_offset]
    }
}

/// Build styled TextRuns for the display text.
fn build_text_runs_for_display(
    display_text: &str,
    line_content: &str,
    display_to_buffer: &[usize],
    decoration: &LineDecoration,
    cursor_col: Option<usize>,
    prefix_collapsed: bool,
    base_font: &Font,
    heading_font: &Font,
    _font_size: f32,
    theme: &Theme,
) -> Vec<TextRun> {
    let line_font = if matches!(decoration.block_type, BlockType::Heading(_)) {
        heading_font
    } else {
        base_font
    };
    if display_text.is_empty() {
        return vec![TextRun {
            len: 1,
            font: line_font.clone(),
            color: theme.foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
    }

    let buf_len = line_content.len();
    let disp_len = display_text.len();

    // Build per-buffer-byte styling first
    let mut buf_colors: Vec<Hsla> = vec![theme.foreground; buf_len];
    let mut buf_weights: Vec<FontWeight> = vec![FontWeight::NORMAL; buf_len];
    let mut buf_font_styles: Vec<FontStyle> = vec![FontStyle::Normal; buf_len];
    let mut buf_bg_colors: Vec<Option<Hsla>> = vec![None; buf_len];
    let mut buf_strikethroughs: Vec<bool> = vec![false; buf_len];
    let mut buf_use_line_font: Vec<bool> =
        vec![matches!(decoration.block_type, BlockType::Heading(_)); buf_len];

    // Block-level styling
    match &decoration.block_type {
        BlockType::Heading(_) => {
            for c in buf_colors.iter_mut() {
                *c = theme.heading;
            }
            for w in buf_weights.iter_mut() {
                *w = FontWeight::BOLD;
            }
        }
        BlockType::Blockquote => {
            for c in buf_colors.iter_mut() {
                *c = theme.syntax_marker;
            }
        }
        BlockType::Table => {
            // Bold text in table header rows.
            if decoration
                .table_info
                .as_ref()
                .is_some_and(|ti| ti.is_header)
            {
                for w in buf_weights.iter_mut() {
                    *w = FontWeight::BOLD;
                }
            }
        }
        _ => {}
    }

    if let BlockType::TaskList(true) = decoration.block_type {
        let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        let trimmed = &line_content[leading_ws..];
        let prefix_len = task_prefix_len(trimmed);
        let text_start = (leading_ws + prefix_len).min(buf_len);
        for index in text_start..buf_len {
            buf_colors[index] = theme.strikethrough;
            buf_strikethroughs[index] = true;
        }
    }

    if matches!(decoration.block_type, BlockType::CodeBlock)
        && let Some(language) = decoration.code_language.as_deref()
    {
        apply_code_block_highlighting(
            line_content,
            language,
            &mut buf_colors,
            &mut buf_weights,
            &mut buf_font_styles,
        );
    }

    // Inline spans
    for span in &decoration.inline_spans {
        let start = span.range.start.min(buf_len);
        let end = span.range.end.min(buf_len);
        for i in start..end {
            match span.kind {
                InlineKind::Bold => {
                    buf_colors[i] = theme.bold;
                    buf_weights[i] = FontWeight::BOLD;
                }
                InlineKind::Italic => {
                    buf_colors[i] = theme.italic;
                    buf_font_styles[i] = FontStyle::Italic;
                }
                InlineKind::Strikethrough => {
                    buf_colors[i] = theme.strikethrough;
                    buf_strikethroughs[i] = true;
                }
                InlineKind::InlineCode => {
                    buf_colors[i] = theme.inline_code;
                    buf_bg_colors[i] = Some(theme.inline_code_bg);
                }
                InlineKind::Link => {
                    buf_colors[i] = theme.link;
                }
                InlineKind::Image => {
                    buf_colors[i] = theme.link;
                }
            }
        }
    }

    // Syntax markers on cursor line: show expanded ones dimmed
    if let Some(col) = cursor_col {
        for sr in &decoration.syntax_ranges {
            if col >= sr.trigger_range.start && col < sr.trigger_range.end {
                let start = sr.range.start.min(buf_len);
                let end = sr.range.end.min(buf_len);
                let syntax_color = match decoration.block_type {
                    BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_) => {
                        theme.list_marker
                    }
                    _ => theme.syntax_marker,
                };
                for i in start..end {
                    buf_colors[i] = syntax_color;
                    buf_use_line_font[i] = false;
                }
            }
        }
    }

    // Map buffer styling to display text, then merge into runs
    let mut colors: Vec<Hsla> = Vec::with_capacity(disp_len);
    let mut weights: Vec<FontWeight> = Vec::with_capacity(disp_len);
    let mut font_styles: Vec<FontStyle> = Vec::with_capacity(disp_len);
    let mut bg_colors: Vec<Option<Hsla>> = Vec::with_capacity(disp_len);
    let mut strikethroughs: Vec<bool> = Vec::with_capacity(disp_len);
    let mut use_line_font: Vec<bool> = Vec::with_capacity(disp_len);

    for i in 0..disp_len {
        let buf_idx = if i < display_to_buffer.len() {
            display_to_buffer[i]
        } else {
            buf_len.saturating_sub(1)
        };
        let buf_idx = buf_idx.min(buf_len.saturating_sub(1));
        colors.push(buf_colors[buf_idx]);
        weights.push(buf_weights[buf_idx]);
        font_styles.push(buf_font_styles[buf_idx]);
        bg_colors.push(buf_bg_colors[buf_idx]);
        strikethroughs.push(buf_strikethroughs[buf_idx]);
        use_line_font.push(buf_use_line_font[buf_idx]);
    }

    // Override styling for inline replacement characters (bullet, checkbox placeholder)
    if prefix_collapsed {
        match &decoration.block_type {
            BlockType::UnorderedList => {
                let bullet_len = "• ".len();
                for i in 0..bullet_len.min(disp_len) {
                    colors[i] = theme.list_marker;
                    use_line_font[i] = false;
                }
            }
            BlockType::TaskList(_) => {
                let placeholder_len = "   ".len();
                for i in 0..placeholder_len.min(disp_len) {
                    colors[i] = gpui::transparent_black();
                    use_line_font[i] = false;
                }
            }
            BlockType::OrderedList => {
                let prefix_len = trimmed_ordered_prefix_len(display_text);
                for i in 0..prefix_len.min(disp_len) {
                    colors[i] = theme.list_marker;
                    use_line_font[i] = false;
                }
            }
            _ => {}
        }
    }

    // Merge into TextRuns
    let mut runs = Vec::new();
    let mut run_start = 0;
    while run_start < disp_len {
        let color = colors[run_start];
        let weight = weights[run_start];
        let style = font_styles[run_start];
        let bg = bg_colors[run_start];
        let strike = strikethroughs[run_start];

        let mut run_end = run_start + 1;
        while run_end < disp_len
            && colors[run_end] == color
            && weights[run_end] == weight
            && font_styles[run_end] == style
            && bg_colors[run_end] == bg
            && strikethroughs[run_end] == strike
            && use_line_font[run_end] == use_line_font[run_start]
        {
            run_end += 1;
        }

        while run_end < disp_len && !display_text.is_char_boundary(run_end) {
            run_end += 1;
        }

        let mut font = if use_line_font[run_start] {
            line_font.clone()
        } else {
            base_font.clone()
        };
        font.weight = weight;
        font.style = style;

        runs.push(TextRun {
            len: run_end - run_start,
            font,
            color,
            background_color: bg,
            underline: None,
            strikethrough: if strike {
                Some(StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(theme.strikethrough),
                })
            } else {
                None
            },
        });

        run_start = run_end;
    }

    runs
}
